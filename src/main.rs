mod audit;
mod config;
mod container;
mod destination;
mod identity;
mod keys;
mod proxy;
mod secrets;

use config::Config;
use destination::ServiceRegistry;
use keys::KeyStore;
use proxy::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("serve") => {
            let config_path = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| "config.toml".to_string());
            run_serve(config_path)
        }
        Some("keys") => match args.get(2).map(String::as_str) {
            Some("add") => cmd_keys_add(&args[3..]),
            Some("list") => cmd_keys_list(&args[3..]),
            Some("revoke") => cmd_keys_revoke(&args[3..]),
            _ => {
                eprintln!("Usage: proxium keys <add|list|revoke> [options]");
                eprintln!("  keys add <user_id> [--expires <ISO_DATE>] [--keys-file <path>]");
                eprintln!("  keys list [--keys-file <path>]");
                eprintln!("  keys revoke <key_id> [--keys-file <path>]");
                std::process::exit(1);
            }
        },
        Some(arg) if !arg.starts_with('-') => {
            // Legacy: treat first arg as config path
            run_serve(arg.to_string())
        }
        None => run_serve("config.toml".to_string()),
        _ => {
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    eprintln!("Usage: proxium <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  serve [config.toml]          Start the proxy server");
    eprintln!("  keys add <user_id>            Create a new API key");
    eprintln!("       [--expires <ISO_DATE>]");
    eprintln!("       [--keys-file <path>]");
    eprintln!("  keys list [--keys-file <path>]  List all API keys");
    eprintln!("  keys revoke <key_id>           Revoke an API key");
    eprintln!("       [--keys-file <path>]");
}

fn run_serve(config_path: String) -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Runtime::new()?.block_on(serve(config_path))
}

async fn serve(config_path: String) -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("proxium=info".parse()?))
        .json()
        .init();

    let config = Config::load(&config_path)?;

    info!(
        mode = ?config.deployment.mode,
        services = config.services.len(),
        "proxium starting"
    );

    let registry = ServiceRegistry::from_config(&config.services);

    let containers = if config.services.iter().any(|s| s.ephemeral) {
        Some(container::ContainerManager::new()?)
    } else {
        None
    };

    let key_store = if config.deployment.mode == config::DeploymentMode::Apikey {
        let path = config.deployment.key_store_path.as_deref();
        let store = KeyStore::load(path)?;
        info!(
            keys = store.keys.len(),
            path = path.unwrap_or(KeyStore::DEFAULT_PATH),
            "loaded API key store"
        );
        Some(store)
    } else {
        None
    };

    let state = Arc::new(AppState {
        registry,
        containers,
        deployment_mode: config.deployment.mode,
        key_store,
    });

    let app = axum::Router::new()
        .fallback(axum::routing::any(proxy::handle_request))
        .with_state(state);

    let addr: SocketAddr = config.server.listen.parse()?;
    info!(%addr, "listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// Parse `--keys-file <path>` from args, returning the path and remaining args.
fn parse_keys_file<'a>(args: &'a [String]) -> (Option<&'a str>, Vec<&'a str>) {
    let mut keys_file: Option<&str> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--keys-file" {
            i += 1;
            if i < args.len() {
                keys_file = Some(args[i].as_str());
            }
        } else {
            rest.push(args[i].as_str());
        }
        i += 1;
    }
    (keys_file, rest)
}

fn cmd_keys_add(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (keys_file, rest) = parse_keys_file(args);

    // Parse positional: user_id; optional: --expires <ISO_DATE>
    let mut user_id: Option<&str> = None;
    let mut expires_at: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--expires" {
            i += 1;
            if i < rest.len() {
                expires_at = Some(
                    chrono::DateTime::parse_from_rfc3339(rest[i])
                        .map_err(|e| format!("invalid --expires date: {}", e))?
                        .with_timezone(&chrono::Utc),
                );
            }
        } else if user_id.is_none() {
            user_id = Some(rest[i]);
        }
        i += 1;
    }

    let user_id = user_id.ok_or("Usage: proxium keys add <user_id> [--expires <ISO_DATE>]")?;

    let mut store = KeyStore::load(keys_file)?;
    let key = store.add(user_id, expires_at);
    store.save(keys_file)?;

    println!("key_id: {}", key.id);
    println!("secret: {}", key.secret);
    if let Some(exp) = key.expires_at {
        println!("expires_at: {}", exp.to_rfc3339());
    }

    Ok(())
}

fn cmd_keys_list(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (keys_file, _) = parse_keys_file(args);
    let store = KeyStore::load(keys_file)?;
    let keys = store.list();

    if keys.is_empty() {
        println!("No API keys found.");
        return Ok(());
    }

    println!("{:<36}  {:<20}  {}", "key_id", "user_id", "expires_at");
    println!("{}", "-".repeat(72));
    for key in keys {
        let expiry = key
            .expires_at
            .map(|e| e.to_rfc3339())
            .unwrap_or_else(|| "never".to_string());
        println!("{:<36}  {:<20}  {}", key.id, key.user_id, expiry);
    }

    Ok(())
}

fn cmd_keys_revoke(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (keys_file, rest) = parse_keys_file(args);
    let key_id = rest
        .first()
        .copied()
        .ok_or("Usage: proxium keys revoke <key_id>")?;

    let mut store = KeyStore::load(keys_file)?;
    store.revoke(key_id).map_err(|e| e.as_str().to_string())?;
    store.save(keys_file)?;

    println!("OK");

    Ok(())
}
