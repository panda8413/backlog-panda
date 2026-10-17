//! Export (JSON, CSV), import avec aperçu et dossiers de destination.
//!
//! Le format JSON est celui du fichier de données lui-même : un export, une sauvegarde datée et
//! `bibliotheque.json` se lisent tous de la même façon, donc tout export est réimportable.

use crate::error::{Error, Result};
use crate::igdb::{ymd_from_unix, GameInfo};
use crate::text::normalize;
use crate::library::Entry;
use crate::store::Store;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Plus gros fichier accepté à l'import (celui du serveur local est du même ordre).
pub const MAX_IMPORT_BYTES: usize = 30 << 20;
/// Nombre de titres cités en exemple dans l'aperçu.
const SAMPLES: usize = 6;

/// Dossier où atterrissent les exports : `BACKLOG_PANDA_EXPORT_DIR`, sinon « Téléchargements »
/// de l'utilisateur s'il existe, sinon `exports/` dans le dossier de données.
pub fn system_export_dir(data_dir: &Path) -> PathBuf {
    if let Some(d) = std::env::var_os("BACKLOG_PANDA_EXPORT_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("USERPROFILE").filter(|d| !d.is_empty()).or_else(|| std::env::var_os("HOME").filter(|d| !d.is_empty()));
    if let Some(dl) = home.map(|h| PathBuf::from(h).join("Downloads")).filter(|p| p.is_dir()) {
        return dl;
    }
    data_dir.join("exports")
}

/// `dir/stem.ext`, ou `dir/stem-2.ext`, `-3`… si le nom est déjà pris : un export n'écrase jamais un fichier.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("{stem}-{n}.{ext}"));
    }
    path
}

/// `backlog-panda-2026-10-17`
pub fn export_stem(now: i64) -> String {
    let (y, m, d) = ymd_from_unix(now);
    format!("backlog-panda-{y:04}-{m:02}-{d:02}")
}

/// Ouvre un dossier (ou sélectionne un fichier) dans l'explorateur du système. Sans attendre.
pub fn reveal(path: &Path) -> Result<()> {
    let mut cmd;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd = std::process::Command::new("explorer.exe");
        if path.is_file() {
            cmd.raw_arg(format!("/select,\"{}\"", path.display()));
        } else {
            cmd.arg(path);
        }
    }
    #[cfg(target_os = "macos")]
    {
        cmd = std::process::Command::new("open");
        if path.is_file() {
            cmd.arg("-R");
        }
        cmd.arg(path);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        cmd = std::process::Command::new("xdg-open");
        cmd.arg(if path.is_file() { path.parent().unwrap_or(path) } else { path });
    }
    let mut child = cmd.spawn().map_err(|e| Error::Io(format!("impossible d'ouvrir le dossier : {e}")))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

// ---------------------------------------------------------------- CSV

/// Une cellule texte : entre guillemets si nécessaire, et neutralisée si Excel pourrait la lire
/// comme une formule (`=`, `+`, `-`, `@` en tête).
fn cell(text: &str) -> String {
    let t = text.replace(['\r', '\n'], " ");
    let t = if t.starts_with(['=', '+', '-', '@', '\t']) { format!("'{t}") } else { t };
    if t.contains([';', '"']) || t != t.trim() {
        format!("\"{}\"", t.replace('"', "\"\""))
    } else {
        t
    }
}

fn decimal(x: f64) -> String {
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').replace('.', ",")
}

const CSV_HEADER: [&str; 14] = [
    "Titre", "Année", "Statut", "Note (sur 5)", "Plateforme jouée", "Heures jouées", "Commentaire", "Genres", "Développeurs", "Éditeurs", "Date de sortie", "Ajouté le", "Terminé le", "Identifiant IGDB",
];

/// `2026-10-17` (UTC) ou vide.
fn iso_date(t: Option<i64>) -> String {
    t.map_or_else(String::new, |t| {
        let (y, m, d) = ymd_from_unix(t);
        format!("{y:04}-{m:02}-{d:02}")
    })
}

/// Tableau pour Excel / LibreOffice : séparateur « ; », nombres à virgule, UTF-8 avec BOM, lignes CRLF.
pub fn to_csv(store: &Store) -> String {
    let mut rows: Vec<(String, String)> = store
        .library
        .live()
        .map(|e| {
            let info = store.games.get(&e.game_id);
            let name = info.map_or_else(|| format!("Jeu {}", e.game_id), |g| g.name.clone());
            (normalize(&name), csv_row(e, info))
        })
        .collect();
    rows.sort();
    let mut out = String::from("\u{feff}");
    out.push_str(&CSV_HEADER.join(";"));
    out.push_str("\r\n");
    for (_, line) in rows {
        out.push_str(&line);
        out.push_str("\r\n");
    }
    out
}

fn csv_row(e: &Entry, info: Option<&GameInfo>) -> String {
    let list = |v: Option<&Vec<String>>| v.map_or_else(String::new, |v| v.join(", "));
    let cols = [
        cell(&info.map_or_else(|| format!("Jeu {}", e.game_id), |g| g.name.clone())),
        info.and_then(|g| g.year).map_or_else(String::new, |y| y.to_string()),
        cell(e.status.label_fr()),
        e.rating.map_or_else(String::new, |r| decimal(r.stars())),
        cell(e.platform.as_deref().unwrap_or("")),
        e.minutes.map_or_else(String::new, |m| decimal(f64::from(m) / 60.0)),
        cell(e.comment.as_deref().unwrap_or("")),
        cell(&list(info.map(|g| &g.genres))),
        cell(&list(info.map(|g| &g.developers))),
        cell(&list(info.map(|g| &g.publishers))),
        iso_date(info.and_then(|g| g.released)),
        iso_date(e.added),
        iso_date(e.finished),
        e.game_id.to_string(),
    ];
    cols.join(";")
}

// ---------------------------------------------------------------- Import

/// Lit un fichier d'export / de sauvegarde. Les entrées dont la fiche de jeu manque sont écartées
/// (leur nombre est renvoyé) : sans fiche, la bibliothèque ne saurait pas les afficher.
pub fn parse_import(text: &str) -> Result<(Store, usize)> {
    if text.len() > MAX_IMPORT_BYTES {
        return Err(Error::Invalid("fichier trop volumineux pour une bibliothèque Backlog Panda".into()));
    }
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Err(Error::Invalid("le fichier est vide".into()));
    }
    let mut store: Store = serde_json::from_str(text).map_err(|_| {
        Error::Invalid("ce fichier n'est pas un export Backlog Panda (le format attendu est le .json créé par « Exporter »)".into())
    })?;
    if store.version > 1 {
        return Err(Error::Invalid("ce fichier vient d'une version plus récente de Backlog Panda".into()));
    }
    let before = store.library.entries.len();
    let games = &store.games;
    store.library.entries.retain(|id, e| e.deleted || games.contains_key(id));
    let skipped = before - store.library.entries.len();
    Ok((store, skipped))
}

fn same_content(a: &Entry, b: &Entry) -> bool {
    a.status == b.status
        && a.rating == b.rating
        && a.platform == b.platform
        && a.comment == b.comment
        && a.minutes == b.minutes
        && a.added == b.added
        && a.finished == b.finished
}

/// Effet d'un mode d'import sur les jeux « vivants » de la bibliothèque.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Effect {
    pub added: usize,
    pub added_names: Vec<String>,
    pub updated: usize,
    pub updated_names: Vec<String>,
    pub removed: usize,
    pub removed_names: Vec<String>,
}

/// Ce que l'import changerait, avant de toucher à quoi que ce soit. Chaque effet est obtenu en
/// appliquant réellement le mode sur une copie : l'aperçu ne peut pas contredire le résultat.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportPlan {
    /// Jeux dans le fichier / dans la bibliothèque actuelle.
    pub file_games: usize,
    pub local_games: usize,
    /// Entrées du fichier écartées faute de fiche de jeu.
    pub skipped: usize,
    pub add: Effect,
    pub merge: Effect,
    pub replace: Effect,
}

fn name_of(store: &Store, id: u64) -> String {
    store.games.get(&id).map_or_else(|| format!("Jeu {id}"), |g| g.name.clone())
}

fn push_sample(v: &mut Vec<String>, name: String) {
    if v.len() < SAMPLES {
        v.push(name);
    }
}

fn effect(before: &Store, after: &Store) -> Effect {
    let mut fx = Effect::default();
    for e in after.library.live() {
        match before.library.get(e.game_id) {
            None => {
                fx.added += 1;
                push_sample(&mut fx.added_names, name_of(after, e.game_id));
            }
            Some(old) if !same_content(old, e) => {
                fx.updated += 1;
                push_sample(&mut fx.updated_names, name_of(after, e.game_id));
            }
            Some(_) => {}
        }
    }
    for e in before.library.live().filter(|e| after.library.get(e.game_id).is_none()) {
        fx.removed += 1;
        push_sample(&mut fx.removed_names, name_of(before, e.game_id));
    }
    fx
}

pub fn plan_import(current: &Store, incoming: &Store, skipped: usize) -> ImportPlan {
    let run = |mode| {
        let mut copy = current.clone();
        apply_import(&mut copy, incoming, mode);
        effect(current, &copy)
    };
    ImportPlan {
        file_games: incoming.library.live().count(),
        local_games: current.library.live().count(),
        skipped,
        add: run(ImportMode::Add),
        merge: run(ImportMode::Merge),
        replace: run(ImportMode::Replace),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    /// N'ajoute que les jeux absents ; rien d'existant n'est modifié.
    Add,
    /// Dernière modification gagnante, jeu par jeu.
    Merge,
    /// Le fichier devient la bibliothèque.
    Replace,
}

impl ImportMode {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "add" => Ok(Self::Add),
            "merge" => Ok(Self::Merge),
            "replace" => Ok(Self::Replace),
            other => Err(Error::Invalid(format!("mode d'import inconnu : {other}"))),
        }
    }
}

/// Fiche à garder pour un jeu touché : celle du fichier si elle est plus détaillée (ou si on n'en a pas),
/// en gardant la traduction déjà faite ici si le fichier n'en a pas.
fn adopt_info(current: &mut Store, incoming: &Store, id: u64) {
    let Some(theirs) = incoming.games.get(&id) else { return };
    match current.games.get(&id) {
        Some(mine) if mine.rev >= theirs.rev => {}
        mine => {
            let mut g = theirs.clone();
            if g.summary_fr.is_none() {
                g.summary_fr = mine.filter(|m| m.summary == g.summary).and_then(|m| m.summary_fr.clone());
            }
            current.games.insert(id, g);
        }
    }
}

/// Applique l'import à `current`. L'appelant a déjà fait la sauvegarde d'avant-import.
pub fn apply_import(current: &mut Store, incoming: &Store, mode: ImportMode) {
    match mode {
        ImportMode::Replace => *current = incoming.clone(),
        ImportMode::Add => {
            for theirs in incoming.library.live() {
                let absent = current.library.get(theirs.game_id).is_none();
                if absent {
                    current.library.upsert_full(theirs.game_id, theirs.status, theirs.rating, theirs.platform.clone(), theirs.comment.clone(), theirs.minutes);
                    adopt_info(current, incoming, theirs.game_id);
                }
            }
        }
        ImportMode::Merge => {
            let before: std::collections::HashMap<u64, crate::library::Stamp> =
                current.library.entries.iter().map(|(i, e)| (*i, e.stamp.clone())).collect();
            current.library.merge(&incoming.library);
            let touched: Vec<u64> = current
                .library
                .entries
                .iter()
                .filter(|(i, e)| !e.deleted && before.get(i) != Some(&e.stamp))
                .map(|(i, _)| *i)
                .collect();
            for id in touched {
                adopt_info(current, incoming, id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cellules_csv() {
        assert_eq!(cell("Hades"), "Hades");
        assert_eq!(cell("a;b"), "\"a;b\"");
        assert_eq!(cell("il a dit \"oui\""), "\"il a dit \"\"oui\"\"\"");
        assert_eq!(cell("ligne 1\nligne 2"), "ligne 1 ligne 2");
        assert_eq!(cell("=1+1"), "'=1+1");
        assert_eq!(cell("-5 points"), "'-5 points");
        assert_eq!(cell("@lien"), "'@lien");
        assert_eq!(cell(" espace"), "\" espace\"");
        assert_eq!((decimal(4.5), decimal(12.0), decimal(12.0 + 1.0 / 3.0), decimal(0.75)), ("4,5".into(), "12".into(), "12,33".into(), "0,75".into()));
    }

    #[test]
    fn noms_d_export_sans_ecrasement() {
        let dir = crate::store::temp_dir("uniq");
        std::fs::create_dir_all(&dir).unwrap();
        let a = unique_path(&dir, "x", "csv");
        assert_eq!(a.file_name().unwrap(), "x.csv");
        std::fs::write(&a, "1").unwrap();
        let b = unique_path(&dir, "x", "csv");
        assert_eq!(b.file_name().unwrap(), "x-2.csv");
        std::fs::write(&b, "2").unwrap();
        assert_eq!(unique_path(&dir, "x", "csv").file_name().unwrap(), "x-3.csv");
        assert_eq!(export_stem(1_792_246_980), "backlog-panda-2026-10-17");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_ecarte_les_entrees_sans_fiche() {
        let mut s = Store::default();
        s.games.insert(1, GameInfo { id: 1, name: "Hades".into(), slug: "hades".into(), ..GameInfo::default() });
        s.library.upsert(1, crate::library::Status::Backlog, None);
        s.library.upsert(2, crate::library::Status::Backlog, None); // pas de fiche
        let text = format!("\u{feff}{}", serde_json::to_string(&s).unwrap());
        let (got, skipped) = parse_import(&text).unwrap();
        assert_eq!((got.library.live().count(), skipped), (1, 1));
        assert!(parse_import("{\"version\":1}").is_err());
    }
}
