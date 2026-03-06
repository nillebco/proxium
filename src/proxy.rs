use axum::body::Body;
use axum::extract::State;
use axum::http::header::HeaderName;
use axum::http::{HeaderMap, Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

use crate::audit;
use crate::config::DeploymentMode;
use crate::container::ContainerManager;
use crate::destination::ServiceRegistry;
use crate::identity::{self, Identity, IdentityError};
use crate::secrets;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

pub struct AppState {
    pub registry: ServiceRegistry,
    pub containers: Option<ContainerManager>,
    pub deployment_mode: DeploymentMode,
}

pub async fn handle_request(
    State(state): State<Arc<AppState>>,
    peer_addr: axum::extract::ConnectInfo<SocketAddr>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let peer = peer_addr.0;

    // Extract headers for identity resolution (avoids borrowing req across await)
    let headers = req.headers().clone();

    // 1. Resolve identity
    let identity = resolve_identity(&state.deployment_mode, &headers, Some(peer))
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "identity resolution failed");
            StatusCode::UNAUTHORIZED
        })?;

    // 2. Resolve destination from Host header
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .map(|h| h.split(':').next().unwrap_or(h))
        .ok_or(StatusCode::BAD_REQUEST)?;

    let destination = state.registry.resolve(host).ok_or_else(|| {
        tracing::warn!(host = host, "no service configured for host");
        StatusCode::NOT_FOUND
    })?;

    // 3. If ephemeral, ensure container is running
    if destination.ephemeral {
        if let (Some(cm), Some(image)) = (&state.containers, &destination.container_image) {
            cm.ensure_ready(
                &destination.service_name,
                image,
                Duration::from_secs(destination.idle_timeout_secs),
            )
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "container start failed");
                StatusCode::SERVICE_UNAVAILABLE
            })?;
        }
    }

    // 4. Inject secret (API key)
    let mut req = req;
    if let Some(key_ref) = &destination.api_key_ref {
        let secret = secrets::resolve(key_ref).map_err(|e| {
            tracing::error!(error = %e, "secret resolution failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        let header_value = format!("{}{}", destination.inject_prefix, secret);
        let header_name: HeaderName = destination
            .inject_header
            .parse()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        req.headers_mut().insert(
            header_name,
            header_value
                .parse()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        );
    }

    // 5. Forward request to upstream
    let method = req.method().clone();
    let path = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let upstream_uri = format!("{}{}", destination.upstream_url, path)
        .parse::<Uri>()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let path_str = path.to_string();

    // Build forwarded request
    let (mut parts, body) = req.into_parts();
    parts.uri = upstream_uri;
    parts.headers.remove("host");

    let forwarded_req = Request::from_parts(parts, body);

    let https = hyper_tls::HttpsConnector::new();
    let client = Client::builder(TokioExecutor::new()).build(https);

    let resp = client.request(forwarded_req).await.map_err(|e| {
        tracing::error!(error = %e, "upstream request failed");
        StatusCode::BAD_GATEWAY
    })?;

    let status = resp.status().as_u16();

    // Convert hyper::body::Incoming to axum::body::Body
    let (mut parts, incoming) = resp.into_parts();
    // Strip upstream Set-Cookie headers — the proxy shouldn't leak upstream cookies
    parts.headers.remove("set-cookie");
    let resp = Response::from_parts(parts, Body::new(incoming));

    // 6. Audit log
    audit::log_request(&identity, destination, method.as_str(), &path_str, status);

    Ok(resp)
}

async fn resolve_identity(
    mode: &DeploymentMode,
    headers: &HeaderMap,
    peer_addr: Option<SocketAddr>,
) -> Result<Identity, IdentityError> {
    match mode {
        DeploymentMode::None => Ok(Identity {
            name: "local".to_string(),
            login: "local".to_string(),
            source: identity::IdentitySource::Local,
        }),
        DeploymentMode::Tailscale => identity::resolve_tailscale(headers, peer_addr).await,
        DeploymentMode::Apikey => identity::resolve_apikey(headers).await,
        DeploymentMode::Oidc => Err(IdentityError::Denied("OIDC not yet implemented".into())),
    }
}
