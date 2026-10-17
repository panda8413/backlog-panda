//! Réglages locaux : clés IGDB (identifiant et secret client Twitch) et clé DeepL (facultative).
//!
//! Le secret IGDB et la clé DeepL vont dans le coffre du système (voir `secrets`) quand il existe :
//! `reglages.json` ne garde alors que l'identifiant client, qui n'est pas confidentiel. Sans coffre,
//! tout reste dans le fichier (droits 0600 sous Unix). Les variables d'environnement `IGDB_CLIENT_ID`,
//! `IGDB_CLIENT_SECRET` et `DEEPL_API_KEY` ont priorité sur les deux.

use crate::error::Result;
use crate::secrets::Vault;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "reglages.json";

/// Où sont rangés les secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    /// Dans le coffre du système, chiffrés.
    Os,
    /// En clair dans `reglages.json` (pas de coffre, ou le coffre a refusé).
    File,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub igdb_client_id: String,
    #[serde(default)]
    pub igdb_client_secret: String,
    /// Clé DeepL (offre gratuite ou Pro) pour traduire les résumés ; facultative.
    #[serde(default)]
    pub deepl_key: String,
}

impl Config {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(CONFIG_FILE)
    }

    /// Lecture sans coffre : tout vient du fichier.
    pub fn load(dir: &Path) -> Config {
        Self::load_with(dir, &Vault::none()).0
    }

    /// Lit le fichier, puis complète avec le coffre. Un secret encore en clair dans le fichier est
    /// déplacé dans le coffre ; le fichier n'est nettoyé qu'après avoir relu le coffre et trouvé la même valeur.
    pub fn load_with(dir: &Path, vault: &Vault) -> (Config, Storage) {
        let mut cfg: Config = std::fs::read_to_string(Self::path(dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let Some(store) = vault.store() else { return (cfg, Storage::File) };
        let mut storage = Storage::Os;
        let mut migrated = false;
        for (name, field) in cfg.secrets_mut() {
            let in_file = field.trim().to_string();
            if in_file.is_empty() {
                match store.get(name) {
                    Ok(Some(v)) => *field = v,
                    Ok(None) => {}
                    Err(_) => storage = Storage::File,
                }
            } else if store.set(name, &in_file).is_ok() && store.get(name) == Ok(Some(in_file)) {
                migrated = true;
            } else {
                storage = Storage::File;
            }
        }
        if migrated && storage == Storage::Os {
            // Réécrit le fichier sans les secrets ; en cas d'échec on garde l'ancien fichier tel quel.
            if let Ok(s) = cfg.save_with(dir, vault) {
                storage = s;
            }
        }
        (cfg, storage)
    }

    /// Écriture sans coffre : tout va dans le fichier.
    pub fn save(&self, dir: &Path) -> Result<()> {
        self.save_with(dir, &Vault::none()).map(|_| ())
    }

    /// Range les secrets dans le coffre (en relisant pour vérifier), le reste dans le fichier.
    /// Si le coffre refuse un secret, celui-ci reste dans le fichier plutôt que d'être perdu.
    pub fn save_with(&self, dir: &Path, vault: &Vault) -> Result<Storage> {
        let mut on_disk = self.clone();
        let mut storage = if vault.store().is_some() { Storage::Os } else { Storage::File };
        if let Some(store) = vault.store() {
            for (name, field) in on_disk.secrets_mut() {
                let value = field.trim().to_string();
                let stored = if value.is_empty() { store.delete(name) } else { store.set(name, &value).and_then(|()| (store.get(name) == Ok(Some(value))).then_some(()).ok_or_else(|| "relecture différente".to_string())) };
                match stored {
                    Ok(()) => field.clear(),
                    Err(_) => storage = Storage::File,
                }
            }
        }
        std::fs::create_dir_all(dir)?;
        let path = Self::path(dir);
        std::fs::write(&path, serde_json::to_string_pretty(&on_disk)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(storage)
    }

    /// Les champs confidentiels, avec le nom de leur entrée dans le coffre.
    fn secrets_mut(&mut self) -> [(&'static str, &mut String); 2] {
        [("igdb_client_secret", &mut self.igdb_client_secret), ("deepl_key", &mut self.deepl_key)]
    }

    /// Valeurs effectives : l'environnement l'emporte. Le booléen indique si l'une vient de l'environnement.
    pub fn effective(&self) -> (Config, bool) {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let (id, secret) = (env("IGDB_CLIENT_ID"), env("IGDB_CLIENT_SECRET"));
        let deepl = env("DEEPL_API_KEY");
        let from_env = id.is_some() || secret.is_some();
        (
            Config {
                igdb_client_id: id.unwrap_or_else(|| self.igdb_client_id.clone()),
                igdb_client_secret: secret.unwrap_or_else(|| self.igdb_client_secret.clone()),
                deepl_key: deepl.unwrap_or_else(|| self.deepl_key.clone()),
            },
            from_env,
        )
    }

    pub fn is_complete(&self) -> bool {
        !self.igdb_client_id.trim().is_empty() && !self.igdb_client_secret.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{MemoryStore, SecretStore};
    use crate::store::temp_dir;

    #[test]
    fn aller_retour_et_valeurs_par_defaut() {
        let dir = temp_dir("cfg");
        assert_eq!(Config::load(&dir), Config::default());
        let c = Config { igdb_client_id: "a".into(), igdb_client_secret: "b".into(), deepl_key: "k:fx".into() };
        c.save(&dir).unwrap();
        assert_eq!(Config::load(&dir), c);
        assert!(c.is_complete());
        assert!(!Config::default().is_complete());
        std::fs::write(Config::path(&dir), "pas du json").unwrap();
        assert_eq!(Config::load(&dir), Config::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn file_text(dir: &Path) -> String {
        std::fs::read_to_string(Config::path(dir)).unwrap_or_default()
    }

    fn sample() -> Config {
        Config { igdb_client_id: "mon-id".into(), igdb_client_secret: "SECRET-IGDB".into(), deepl_key: "CLE-DEEPL:fx".into() }
    }

    #[test]
    fn les_secrets_vont_dans_le_coffre_pas_dans_le_fichier() {
        let dir = temp_dir("cfg-vault");
        let store = MemoryStore::new();
        let vault = Vault::with(store.clone());
        assert_eq!(sample().save_with(&dir, &vault).unwrap(), Storage::Os);
        let text = file_text(&dir);
        assert!(text.contains("mon-id") && !text.contains("SECRET-IGDB") && !text.contains("CLE-DEEPL"), "{text}");
        assert_eq!(store.peek("igdb_client_secret").as_deref(), Some("SECRET-IGDB"));
        let (back, storage) = Config::load_with(&dir, &vault);
        assert_eq!((back, storage), (sample(), Storage::Os));
        // effacer une clé la retire aussi du coffre
        let mut c = sample();
        c.deepl_key.clear();
        c.save_with(&dir, &vault).unwrap();
        assert_eq!(store.peek("deepl_key"), None);
        assert_eq!(Config::load_with(&dir, &vault).0.deepl_key, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_d_un_ancien_fichier_en_clair() {
        let dir = temp_dir("cfg-migrate");
        sample().save(&dir).unwrap();
        assert!(file_text(&dir).contains("SECRET-IGDB"), "point de départ : fichier en clair");
        let store = MemoryStore::new();
        let vault = Vault::with(store.clone());
        let (cfg, storage) = Config::load_with(&dir, &vault);
        assert_eq!((cfg, storage), (sample(), Storage::Os), "rien ne se perd à la migration");
        let text = file_text(&dir);
        assert!(!text.contains("SECRET-IGDB") && !text.contains("CLE-DEEPL") && text.contains("mon-id"), "{text}");
        assert_eq!(store.peek("deepl_key").as_deref(), Some("CLE-DEEPL:fx"));
        // deuxième lancement : tout se relit depuis le coffre
        assert_eq!(Config::load_with(&dir, &vault).0, sample());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coffre_refusant_l_ecriture_rien_n_est_perdu() {
        let dir = temp_dir("cfg-locked");
        sample().save(&dir).unwrap();
        let vault = Vault::with(MemoryStore::failing());
        let (cfg, storage) = Config::load_with(&dir, &vault);
        assert_eq!((cfg, storage), (sample(), Storage::File));
        assert!(file_text(&dir).contains("SECRET-IGDB"), "le fichier garde les clés tant que le coffre ne les a pas");
        // une sauvegarde qui échoue dans le coffre garde aussi la clé dans le fichier
        assert_eq!(sample().save_with(&dir, &vault).unwrap(), Storage::File);
        assert_eq!(Config::load_with(&dir, &vault).0, sample());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sans_coffre_comportement_inchange() {
        let dir = temp_dir("cfg-none");
        assert_eq!(sample().save_with(&dir, &Vault::none()).unwrap(), Storage::File);
        assert!(file_text(&dir).contains("SECRET-IGDB"));
        assert_eq!(Config::load_with(&dir, &Vault::none()), (sample(), Storage::File));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_coffre_qui_oublie_ne_fait_pas_perdre_le_fichier() {
        /// Simule une plateforme où `set` réussit mais ne retient rien (entrée factice).
        struct Amnesic;
        impl SecretStore for Amnesic {
            fn label(&self) -> &'static str { "amnésique" }
            fn get(&self, _: &str) -> std::result::Result<Option<String>, String> { Ok(None) }
            fn set(&self, _: &str, _: &str) -> std::result::Result<(), String> { Ok(()) }
            fn delete(&self, _: &str) -> std::result::Result<(), String> { Ok(()) }
        }
        let dir = temp_dir("cfg-amnesic");
        sample().save(&dir).unwrap();
        let vault = Vault::with(std::sync::Arc::new(Amnesic));
        let (cfg, storage) = Config::load_with(&dir, &vault);
        assert_eq!((cfg, storage), (sample(), Storage::File));
        assert!(file_text(&dir).contains("SECRET-IGDB"), "relecture différente : le fichier n'est pas nettoyé");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
