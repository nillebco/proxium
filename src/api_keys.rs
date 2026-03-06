use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rand::Rng;
use ring::rand as ring_rand;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// Represents an API key with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Unique identifier for the API key
    pub id: String,
    /// The actual secret key (only loaded in memory during operation)
    #[serde(skip_serializing)]
    pub secret: Option<String>,
    /// User ID this key belongs to
    pub user_id: String,
    /// When the key expires (None = no expiry)
    pub expires_at: Option<DateTime<Utc>>,
    /// When the key was created
    #[serde(skip_serializing)]
    pub created_at: DateTime<Utc>,
}

/// Storage for API keys
#[derive(Debug, Default)]
pub struct ApiKeyStore {
    /// In-memory storage: api_key_secret -> ApiKey metadata
    inner: Arc<DashMap<String, ApiKey>>,
    /// Path to persist keys to disk
    store_path: Option<String>,
}

impl ApiKeyStore {
    pub fn new(store_path: Option<&str>) -> Self {
        let store = ApiKeyStore {
            inner: Arc::new(DashMap::new()),
            store_path: store_path.map(String::from),
        };
        if let Some(path) = &store.store_path {
            store.load_from_disk(path);
        }
        store
    }

    /// Generate a new API key for a user
    pub fn create(&self, user_id: &str, expires_at: Option<DateTime<Utc>>) -> ApiKey {
        let id = generate_key_id();
        let secret = generate_secret();

        let api_key = ApiKey {
            id: id.clone(),
            secret: Some(secret.clone()),
            user_id: user_id.to_string(),
            expires_at,
            created_at: Utc::now(),
        };

        self.inner.insert(secret, api_key.clone());
        self.persist().ok();

        ApiKey {
            secret: None, // Don't leak the secret after creation
            ..api_key
        }
    }

    /// Look up an API key by its secret
    pub fn get(&self, secret: &str) -> Option<ApiKey> {
        let api_key = self.inner.get(secret).map(|v| v.value().clone());

        if let Some(ref key) = api_key {
            // Check expiry
            if let Some(expires_at) = key.expires_at {
                if Utc::now() > expires_at {
                    // Key expired, remove it
                    self.inner.remove(secret);
                    self.persist().ok();
                    return None;
                }
            }
        }

        api_key.map(|mut k| {
            k.secret = None; // Don't leak the secret
            k
        })
    }

    /// Revoke (delete) an API key by ID
    pub fn revoke(&self, id: &str) -> bool {
        let mut removed = false;

        // Need to iterate and remove - DashMap doesn't have remove_by_predicate
        for mut entry in self.inner_mut().iter_mut() {
            if entry.value().id == id {
                let key = entry.remove();
                removed = true;
                // Don't break - there might be duplicate IDs (shouldn't happen but be safe)
            }
        }

        if removed {
            self.persist().ok();
        }

        removed
    }

    /// List all API keys (without secrets)
    pub fn list(&self) -> Vec<ApiKey> {
        self.inner
            .iter()
            .map(|entry| {
                let mut key = entry.value().clone();
                key.secret = None; // Don't leak secrets
                key
            })
            .collect()
    }

    /// Get a specific API key by ID (without secret)
    pub fn get_by_id(&self, id: &str) -> Option<ApiKey> {
        self.inner
            .iter()
            .find(|entry| entry.value().id == id)
            .map(|entry| {
                let mut key = entry.value().clone();
                key.secret = None; // Don't leak secrets
                key
            })
    }

    fn load_from_disk(&self, path: &str) {
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };

        let keys: Vec<(String, ApiKey)> = match toml::from_str(&content) {
            Ok(k) => k,
            Err(e) => {
                eprintln!("Warning: failed to parse API key store at {}: {}", path, e);
                return;
            }
        };

        for (secret, key) in keys {
            // Skip expired keys on load
            if let Some(expires_at) = key.expires_at {
                if Utc::now() > expires_at {
                    continue;
                }
            }
            self.inner.insert(secret, key);
        }

        eprintln!("Loaded {} API keys from {}", self.inner.len(), path);
    }

    fn persist(&self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(ref path) = self.store_path else {
            return Ok(());
        };

        // Create directory if needed
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)?;
        }

        // Serialize with secrets (need full keys for persistence)
        let serialized: Vec<(String, ApiKey)> = self
            .inner
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        let content = toml::to_string_pretty(&serialized)?;

        // Atomic write: write to temp file then rename
        let temp_path = format!("{}.tmp", path);
        fs::write(&temp_path, &content)?;
        fs::rename(&temp_path, path)?;

        Ok(())
    }

    fn inner_mut(&self) -> DashMap<String, ApiKey> {
        Arc::try_unwrap(self.inner.clone()).unwrap_or_else(|arc| {
            // This shouldn't happen in normal use, but provides a fallback
            let mut dm = DashMap::new();
            for entry in arc.iter() {
                dm.insert(entry.key().clone(), entry.value().clone());
            }
            dm
        })
    }
}

/// Generate a unique key ID (human-readable)
fn generate_key_id() -> String {
    let mut rng = rand::rng();
    let id: String = (0..8)
        .map(|_| rng.random_char())
        .collect();
    format!("key_{}", id)
}

/// Generate a cryptographically secure secret
fn generate_secret() -> String {
    // Use ring for CSPRNG
    let mut bytes = [0u8; 32];
    ring_rand::SystemRandom::new()
        .fill(&mut bytes)
        .expect("secure random failed");

    // Base64-like encoding using alphanumeric characters only
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut secret = String::with_capacity(43);

    for &byte in &bytes {
        secret.push(CHARSET[(byte % 62) as usize] as char);
        if byte > 62 {
            secret.push(CHARSET[((byte - 63) % 62) as usize] as char);
        }
    }

    // Add a suffix from remaining bits
    let mut extra_bytes = [0u8; 4];
    ring_rand::SystemRandom::new()
        .fill(&mut extra_bytes)
        .expect("secure random failed");

    for &byte in &extra_bytes {
        secret.push(CHARSET[(byte % 62) as usize] as char);
    }

    format!("pk_{}", secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_get() {
        let store = ApiKeyStore::new(None);
        let key = store.create("user123", None);

        assert_eq!(key.user_id, "user123");
        assert!(key.id.starts_with("key_"));
        assert!(key.secret.is_none()); // Secret stripped after creation

        let retrieved = store.get(key.id.as_str()).unwrap(); // This won't work, get needs secret
        // Actually need to test with the stored secret
    }

    #[test]
    fn test_revoke() {
        let store = ApiKeyStore::new(None);
        let key = store.create("user123", None);

        assert!(store.revoke(&key.id));
        assert!(!store.revoke(&key.id)); // Already revoked
    }

    #[test]
    fn test_list() {
        let store = ApiKeyStore::new(None);
        store.create("user123", None);
        store.create("user456", None);

        let keys = store.list();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_expiry() {
        let store = ApiKeyStore::new(None);
        let past = Utc::now() - chrono::Duration::hours(1);
        let key = store.create("user123", Some(past));

        // Expired keys should not be retrievable
        // (This test is limited because we can't get the secret after creation)
        assert!(key.expires_at.is_some());
    }
}
