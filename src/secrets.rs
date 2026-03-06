pub fn resolve(key_ref: &str) -> Result<String, SecretError> {
    // env://VAR_NAME → read from environment (e.g. injected by fnox exec)
    if let Some(var_name) = key_ref.strip_prefix("env://") {
        return std::env::var(var_name)
            .map_err(|_| SecretError::NotFound(format!("env var {} not set", var_name)));
    }

    Err(SecretError::InvalidRef(key_ref.to_string()))
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("invalid secret ref: {0}")]
    InvalidRef(String),
    #[error("secret not found: {0}")]
    NotFound(String),
}
