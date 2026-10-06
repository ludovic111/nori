//! API keys live in the OS keychain (macOS Keychain, Windows Credential Manager, Secret
//! Service on Linux), never in plain files. The lsuite AI token is the shared account file's
//! (`account.rs`), not here.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

const SERVICE: &str = "xyz.lsuite.nori";

/// Where keys are kept.
pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Option<String>;
    fn set(&self, id: &str, key: &str) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

/// Keys in memory (tests, debug builds), seeded from the usual environment variables.
#[derive(Default)]
pub struct MemorySecrets {
    keys: Mutex<HashMap<String, String>>,
}

/// The environment variable a provider's key is read from when it isn't stored.
pub fn env_var(id: &str) -> Option<&'static str> {
    Some(match id {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "openai-compatible" => "OPENAI_COMPATIBLE_API_KEY",
        _ => return None,
    })
}

impl SecretStore for MemorySecrets {
    fn get(&self, id: &str) -> Option<String> {
        self.keys.lock().get(id).cloned().or_else(|| env_var(id).and_then(|v| std::env::var(v).ok()).filter(|k| !k.trim().is_empty()))
    }

    fn set(&self, id: &str, key: &str) -> Result<(), String> {
        self.keys.lock().insert(id.into(), key.into());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        self.keys.lock().remove(id);
        Ok(())
    }
}

/// Keychain-backed store with a cache, so the OS is asked at most once per key per launch.
#[derive(Default)]
pub struct KeychainSecrets {
    cache: Mutex<HashMap<String, Option<String>>>,
}

impl SecretStore for KeychainSecrets {
    fn get(&self, id: &str) -> Option<String> {
        if let Some(v) = self.cache.lock().get(id) {
            return v.clone();
        }
        let v = match keyring::Entry::new(SERVICE, id).and_then(|e| e.get_password()) {
            Ok(k) => Some(k),
            Err(keyring::Error::NoEntry) => None,
            Err(e) => {
                tracing::warn!("couldn't read the {id} key from the keychain: {e}");
                return env_var(id).and_then(|v| std::env::var(v).ok());
            }
        };
        let v = v.or_else(|| env_var(id).and_then(|v| std::env::var(v).ok()).filter(|k| !k.trim().is_empty()));
        self.cache.lock().insert(id.to_string(), v.clone());
        v
    }

    fn set(&self, id: &str, key: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, id).and_then(|e| e.set_password(key)).map_err(|e| format!("Couldn't save the key to the keychain: {e}"))?;
        self.cache.lock().insert(id.to_string(), Some(key.to_string()));
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        match keyring::Entry::new(SERVICE, id).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(format!("Couldn't remove the key: {e}")),
        }
        self.cache.lock().insert(id.to_string(), None);
        Ok(())
    }
}

/// The OS keychain, except in debug builds (an unsigned binary makes macOS ask for the login
/// password each time). `NORI_KEYCHAIN=1` forces it, `NORI_KEYCHAIN=0` turns it off.
pub fn default_store() -> Arc<dyn SecretStore> {
    let on = match std::env::var("NORI_KEYCHAIN").ok().as_deref() {
        Some("1" | "on" | "true" | "yes") => true,
        Some(_) => false,
        None => !cfg!(debug_assertions),
    };
    if on { Arc::new(KeychainSecrets::default()) } else { Arc::new(MemorySecrets::default()) }
}
