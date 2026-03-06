mod audit;
mod config;
mod container;
mod destination;
mod identity;
mod proxy;
mod secrets;

use axum::routing::any;
use axum::Router;
use config::Config;
use destination::ServiceRegistry;
use proxy::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("proxium=info".parse()?))
        .json()
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());

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

    let state = Arc::new(AppState {
        registry,
        containers,
        deployment_mode: config.deployment.mode,
    });

    let app = Router::new()
        .fallback(any(proxy::handle_request))
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
