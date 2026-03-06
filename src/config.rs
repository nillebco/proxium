use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub deployment: DeploymentConfig,
    pub server: ServerConfig,
    pub services: Vec<ServiceConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DeploymentConfig {
    #[serde(default)]
    pub mode: DeploymentMode,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentMode {
    None,
    Tailscale,
    Oidc,
    Apikey,
}

impl Default for DeploymentMode {
    fn default() -> Self {
        DeploymentMode::None
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub listen: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceConfig {
    pub hostname: String,
    pub upstream: String,
    pub api_key_ref: Option<String>,
    #[serde(default = "default_inject_header")]
    pub inject_header: String,
    #[serde(default)]
    pub inject_prefix: String,
    #[serde(default)]
    pub ephemeral: bool,
    pub container_image: Option<String>,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
}

fn default_inject_header() -> String {
    "Authorization".to_string()
}

fn default_idle_timeout() -> u64 {
    300
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
