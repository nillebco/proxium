use crate::config::ServiceConfig;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Destination {
    pub service_name: String,
    pub upstream_url: String,
    pub api_key_ref: Option<String>,
    pub inject_header: String,
    pub inject_prefix: String,
    pub ephemeral: bool,
    pub container_image: Option<String>,
    pub idle_timeout_secs: u64,
}

pub struct ServiceRegistry {
    services: HashMap<String, Destination>,
}

impl ServiceRegistry {
    pub fn from_config(services: &[ServiceConfig]) -> Self {
        let mut map = HashMap::new();
        for svc in services {
            map.insert(
                svc.hostname.clone(),
                Destination {
                    service_name: svc.hostname.clone(),
                    upstream_url: svc.upstream.clone(),
                    api_key_ref: svc.api_key_ref.clone(),
                    inject_header: svc.inject_header.clone(),
                    inject_prefix: svc.inject_prefix.clone(),
                    ephemeral: svc.ephemeral,
                    container_image: svc.container_image.clone(),
                    idle_timeout_secs: svc.idle_timeout_secs,
                },
            );
        }
        ServiceRegistry { services: map }
    }

    pub fn resolve(&self, hostname: &str) -> Option<&Destination> {
        self.services.get(hostname)
    }
}
