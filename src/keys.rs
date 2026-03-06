use chrono::{DateTime, Utc};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub user_id: String,
    pub secret: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KeyStore {
    pub keys: Vec<ApiKey>,
}

impl KeyStore {
    pub const DEFAULT_PATH: &'static str = "api_keys.json";

    pub fn load(path: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        let file_path = path.unwrap_or(Self::DEFAULT_PATH);
        if !Path::new(file_path).exists() {
            return Ok(KeyStore { keys: Vec::new() });
        }
        let content = fs::read_to_string(file_path)?;
        if content.trim().is_empty() {
            return Ok(KeyStore { keys: Vec::new() });
        }
        let store: KeyStore = serde_json::from_str(&content)?;
        Ok(store)
    }

    pub fn save(&self, path: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let file_path = path.unwrap_or(Self::DEFAULT_PATH);
        if let Some(parent) = Path::new(file_path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(file_path, content)?;
        Ok(())
    }

    pub fn add(&mut self, user_id: &str, expires_at: Option<DateTime<Utc>>) -> ApiKey {
        let id = generate_key_id();
        let secret = generate_secret();
        let key = ApiKey {
            id,
            user_id: user_id.to_string(),
            secret,
            created_at: Utc::now(),
            expires_at,
        };
        self.keys.push(key.clone());
        key
    }

    pub fn list(&self) -> Vec<&ApiKey> {
        self.keys.iter().collect()
    }

    pub fn revoke(&mut self, key_id: &str) -> Result<(), String> {
        let pos = self
            .keys
            .iter()
            .position(|k| k.id == key_id)
            .ok_or_else(|| format!("API key {} not found", key_id))?;
        self.keys.remove(pos);
        Ok(())
    }

    pub fn verify(&self, secret: &str) -> Option<&ApiKey> {
        self.keys
            .iter()
            .find(|k| k.secret == secret && !k.is_expired())
    }
}

impl ApiKey {
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }
}

fn generate_key_id() -> String {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 8];
    rng.fill(&mut bytes).expect("secure random failed");
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!("key_{}", hex)
}

fn generate_secret() -> String {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes).expect("secure random failed");
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let secret: String = bytes
        .iter()
        .map(|&b| CHARSET[(b as usize) % 62] as char)
        .collect();
    format!("pk_{}", secret)
}
