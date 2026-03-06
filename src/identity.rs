use axum::http::HeaderMap;
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct Identity {
    pub name: String,
    pub login: String,
    pub source: IdentitySource,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum IdentitySource {
    Local,
    Tailscale,
    Oidc,
    ApiKey,
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} <{}> ({})", self.name, self.login, self.source)
    }
}

impl std::fmt::Display for IdentitySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdentitySource::Local => write!(f, "local"),
            IdentitySource::Tailscale => write!(f, "tailscale"),
            IdentitySource::Oidc => write!(f, "oidc"),
            IdentitySource::ApiKey => write!(f, "apikey"),
        }
    }
}

pub async fn resolve_tailscale(
    headers: &HeaderMap,
    peer_addr: Option<SocketAddr>,
) -> Result<Identity, IdentityError> {
    // Try Tailscale-injected headers first (from tailscale serve)
    if let Some(login) = headers.get("Tailscale-User-Login") {
        let login = login.to_str().map_err(|_| IdentityError::InvalidHeader)?;
        let name = headers
            .get("Tailscale-User-Name")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(login);
        return Ok(Identity {
            name: name.to_string(),
            login: login.to_string(),
            source: IdentitySource::Tailscale,
        });
    }

    // Fall back to localapi whois
    let peer_ip = peer_addr
        .ok_or(IdentityError::NoPeerAddress)?
        .ip()
        .to_string();

    let socket_path = std::env::var("TAILSCALE_SOCKET")
        .unwrap_or_else(|_| "/var/run/tailscale/tailscaled.sock".to_string());

    let client = reqwest::Client::new();

    let url = format!(
        "http://local-tailscaled.sock/localapi/v0/whois?addr={}",
        peer_ip
    );
    let resp = client
        .get(&url)
        .header("Host", "local-tailscaled.sock")
        .send()
        .await
        .map_err(|e| IdentityError::Transport(format!("whois call to {socket_path}: {e}")))?;

    if !resp.status().is_success() {
        return Err(IdentityError::Denied(format!(
            "whois returned {}",
            resp.status()
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| IdentityError::Transport(e.to_string()))?;

    let login = body["UserProfile"]["LoginName"]
        .as_str()
        .ok_or(IdentityError::Denied("no LoginName in whois".into()))?
        .to_string();
    let name = body["UserProfile"]["DisplayName"]
        .as_str()
        .unwrap_or(&login)
        .to_string();

    Ok(Identity {
        name,
        login,
        source: IdentitySource::Tailscale,
    })
}

pub async fn resolve_apikey(headers: &HeaderMap) -> Result<Identity, IdentityError> {
    let key = headers
        .get("X-Api-Key")
        .ok_or(IdentityError::Denied("missing X-Api-Key header".into()))?
        .to_str()
        .map_err(|_| IdentityError::InvalidHeader)?;

    Ok(Identity {
        name: format!("apikey:{}", &key[..8.min(key.len())]),
        login: key.to_string(),
        source: IdentitySource::ApiKey,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("access denied: {0}")]
    Denied(String),
    #[error("invalid header encoding")]
    InvalidHeader,
    #[error("no peer address")]
    NoPeerAddress,
    #[error("transport error: {0}")]
    Transport(String),
}
