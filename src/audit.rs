use crate::destination::Destination;
use crate::identity::Identity;
use tracing::info;

pub fn log_request(
    identity: &Identity,
    destination: &Destination,
    method: &str,
    path: &str,
    status: u16,
) {
    info!(
        caller = %identity,
        service = %destination.service_name,
        upstream = %destination.upstream_url,
        method = method,
        path = path,
        status = status,
        "request proxied"
    );
}
