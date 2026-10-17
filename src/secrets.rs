//! Coffre pour les clés d'API : le gestionnaire d'identifiants du système plutôt qu'un fichier en clair.
//!
//! * Windows : Gestionnaire d'identifiants (chiffré par Windows, lié à ta session) ;
//! * macOS : Trousseau d'accès ;
//! * ailleurs : pas de coffre, les clés restent dans `reglages.json` (droits 0600 sous Unix).
//!
//! `BACKLOG_PANDA_NO_KEYRING=1` force le repli sur le fichier (utile en mode portable).
//! Ce coffre protège contre la copie ou la lecture du dossier de données ; il ne protège pas d'un
//! programme malveillant qui tournerait déjà sous ta session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Nom du service sous lequel les entrées apparaissent dans le coffre du système.
pub const SERVICE: &str = "Backlog Panda";

pub trait SecretStore: Send + Sync {
    /// Nom lisible du coffre, pour l'interface.
    fn label(&self) -> &'static str;
    fn get(&self, name: &str) -> Result<Option<String>, String>;
    fn set(&self, name: &str, value: &str) -> Result<(), String>;
    /// Supprimer une entrée absente n'est pas une erreur.
    fn delete(&self, name: &str) -> Result<(), String>;
}

/// Coffre du système d'exploitation (via la bibliothèque `keyring`).
pub struct OsStore {
    label: &'static str,
}

impl OsStore {
    fn entry(name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, name).map_err(|e| e.to_string())
    }
}

impl SecretStore for OsStore {
    fn label(&self) -> &'static str {
        self.label
    }
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        match Self::entry(name)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        Self::entry(name)?.set_password(value).map_err(|e| e.to_string())
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Coffre en mémoire (tests). `failing()` refuse toute écriture.
#[derive(Default)]
pub struct MemoryStore {
    items: Mutex<HashMap<String, String>>,
    fail_writes: bool,
}

impl MemoryStore {
    pub fn new() -> Arc<MemoryStore> {
        Arc::new(MemoryStore::default())
    }
    pub fn failing() -> Arc<MemoryStore> {
        Arc::new(MemoryStore { fail_writes: true, ..MemoryStore::default() })
    }
    pub fn peek(&self, name: &str) -> Option<String> {
        self.items.lock().ok().and_then(|m| m.get(name).cloned())
    }
}

impl SecretStore for MemoryStore {
    fn label(&self) -> &'static str {
        "le coffre de test"
    }
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        Ok(self.peek(name))
    }
    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        if self.fail_writes {
            return Err("coffre verrouillé".into());
        }
        self.items.lock().map_err(|e| e.to_string())?.insert(name.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        if self.fail_writes {
            return Err("coffre verrouillé".into());
        }
        self.items.lock().map_err(|e| e.to_string())?.remove(name);
        Ok(())
    }
}

/// Coffre choisi pour cette exécution (peut être absent).
#[derive(Clone, Default)]
pub struct Vault(Option<Arc<dyn SecretStore>>);

impl Vault {
    pub fn none() -> Vault {
        Vault(None)
    }
    pub fn with(store: Arc<dyn SecretStore>) -> Vault {
        Vault(Some(store))
    }
    pub fn store(&self) -> Option<&Arc<dyn SecretStore>> {
        self.0.as_ref()
    }

    /// Coffre du système, sauf plateforme sans coffre géré, ou `BACKLOG_PANDA_NO_KEYRING` défini.
    /// Les tests unitaires n'y touchent jamais (ils ne doivent pas écrire dans tes identifiants).
    pub fn system() -> Vault {
        #[cfg(all(not(test), any(windows, target_os = "macos")))]
        {
            if std::env::var("BACKLOG_PANDA_NO_KEYRING").map(|v| !v.trim().is_empty() && v != "0").unwrap_or(false) {
                return Vault::none();
            }
            let label = if cfg!(windows) { "le Gestionnaire d'identifiants de Windows" } else { "le Trousseau d'accès de macOS" };
            Vault(Some(Arc::new(OsStore { label })))
        }
        #[cfg(not(all(not(test), any(windows, target_os = "macos"))))]
        {
            Vault::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coffre_en_memoire() {
        let s = MemoryStore::new();
        assert_eq!(s.get("a"), Ok(None));
        s.set("a", "1").unwrap();
        assert_eq!(s.get("a"), Ok(Some("1".into())));
        s.delete("a").unwrap();
        s.delete("a").unwrap();
        assert_eq!(s.get("a"), Ok(None));
        assert!(MemoryStore::failing().set("a", "1").is_err());
    }

    /// Sur une plateforme sans coffre natif, `keyring` retombe sur une entrée factice qui ne retient rien :
    /// c'est pourquoi le code ne s'en sert que sous Windows et macOS, et relit toujours avant de supprimer le fichier.
    #[test]
    fn le_coffre_systeme_est_absent_en_test() {
        assert!(Vault::system().store().is_none());
    }
}
