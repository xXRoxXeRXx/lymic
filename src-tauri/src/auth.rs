use keyring::Entry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthConfig {
    pub server_url: String,
    pub api_key: String,
}

const SERVICE_NAME: &str = "lymic-sync";
const ACCOUNT_NAME: &str = "api-credentials";

pub fn store_credentials(server_url: &str, api_key: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, ACCOUNT_NAME).map_err(|e| e.to_string())?;
    let config = AuthConfig {
        server_url: server_url.to_string(),
        api_key: api_key.to_string(),
    };
    let json = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    entry.set_password(&json).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_credentials() -> Result<Option<AuthConfig>, String> {
    let entry = Entry::new(SERVICE_NAME, ACCOUNT_NAME).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(json) => {
            let config: AuthConfig = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            Ok(Some(config))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn delete_credentials() -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, ACCOUNT_NAME).map_err(|e| e.to_string())?;
    // NoEntry is the desired end-state; treat it as success.
    match entry.delete_password() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
