//! Reconstruction d'un catalogue à partir des réponses `/autocomplete.json` déjà capturées
//! (`docs/evidence/E05_autocomplete__*.json`). Aucun accès réseau : l'application ne contacte jamais Backloggd.

use crate::catalog::Catalog;
use crate::error::Result;
use crate::search::AutocompleteResponse;
use std::path::{Path, PathBuf};

/// Jumelage explicite pour la requête « zelda links » : la capture `fe_true` correspondante n'a pas été
/// enregistrée à part ; sa réponse était identique (2594 octets, constaté à la capture) à celle de
/// « Zelda: Link's » (même requête une fois normalisée : « zeldalinks »).
const TWIN_ALIASES: [(&str, &str); 1] =
    [("E05_autocomplete__zelda_links__fe_false.json", "E05_autocomplete__Zelda_3A_Link_27s__fe_true.json")];

pub fn read_response(path: &Path) -> Result<AutocompleteResponse> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

/// Fichiers `E05_autocomplete__*.json` d'un dossier, triés.
pub fn autocomplete_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("E05_autocomplete__") && n.ends_with(".json"))
        })
        .collect();
    files.sort();
    Ok(files)
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Les fichiers `fe_false` d'abord (identifiants authentiques, sans réécriture par `filter_editions`),
/// puis les autres ; un identifiant déjà connu n'est jamais écrasé. Les éditions sont détectées par
/// appariement `fe_false` / `fe_true`.
pub fn import_catalog(dir: &Path) -> Result<Catalog> {
    let files = autocomplete_files(dir)?;
    let (falses, others): (Vec<_>, Vec<_>) = files.iter().partition(|p| file_name(p).contains("__fe_false"));
    let mut catalog = Catalog::new();
    for p in falses.iter().chain(others.iter()) {
        catalog.absorb(&read_response(p)?.suggestions);
    }
    for f in &falses {
        let name = file_name(f);
        let twin_name = TWIN_ALIASES
            .iter()
            .find(|(k, _)| *k == name)
            .map_or_else(|| name.replace("__fe_false", "__fe_true"), |(_, v)| v.to_string());
        let twin = dir.join(twin_name);
        if twin.exists() {
            catalog.mark_editions(&read_response(f)?.suggestions, &read_response(&twin)?.suggestions);
        }
    }
    Ok(catalog)
}
