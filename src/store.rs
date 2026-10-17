//! Stockage local : un fichier JSON dans le dossier de données de l'utilisateur.
//!
//! Contient la bibliothèque (statuts, notes) et une copie des fiches de jeux (nom, année, jaquette…)
//! pour que la bibliothèque reste lisible hors ligne. Écriture atomique (fichier temporaire puis
//! renommage) avec une sauvegarde `.bak` de la version précédente.

use crate::error::Result;
use crate::igdb::GameInfo;
use crate::library::Library;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const STORE_FILE: &str = "bibliotheque.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Store {
    pub version: u32,
    pub games: BTreeMap<u64, GameInfo>,
    pub library: Library,
}

impl Default for Store {
    fn default() -> Self {
        Store { version: 1, games: BTreeMap::new(), library: Library::new("local") }
    }
}

/// Dossier de données par défaut : `%APPDATA%\BacklogPanda` (Windows), `~/.local/share/backlog-panda` (Linux/macOS).
/// `BACKLOG_PANDA_DIR` a priorité.
pub fn default_data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("BACKLOG_PANDA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    if let Some(d) = std::env::var_os("APPDATA").filter(|d| !d.is_empty()) {
        return PathBuf::from(d).join("BacklogPanda");
    }
    if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(d).join("backlog-panda");
    }
    match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h).join(".local/share/backlog-panda"),
        None => PathBuf::from("backlog-panda-data"),
    }
}

/// Sous-dossier des sauvegardes datées.
pub const BACKUP_DIR: &str = "sauvegardes";
/// Nombre de sauvegardes datées conservées (les plus récentes).
pub const BACKUP_KEEP: usize = 15;
/// Une sauvegarde automatique est faite au plus toutes les 20 heures.
pub const AUTO_EVERY_SECS: i64 = 20 * 3600;
/// Types de sauvegarde, tels qu'écrits dans le nom du fichier.
pub const BACKUP_KINDS: [&str; 4] = ["auto", "manuelle", "avant-import", "avant-restauration"];

/// Une sauvegarde datée, telle que listée à l'écran.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackupInfo {
    pub file: String,
    /// Horodatage Unix (UTC) lu dans le nom du fichier.
    pub ts: i64,
    pub kind: String,
    pub bytes: u64,
    /// Jeux dans la bibliothèque ; `None` si le fichier est illisible.
    pub games: Option<usize>,
}

/// D'où vient la bibliothèque chargée quand le fichier principal était illisible.
#[derive(Debug, Clone, PartialEq)]
pub enum Recovery {
    /// Restaurée depuis la copie `.bak` (version précédente).
    FromBak,
    /// Restaurée depuis la sauvegarde datée de ce nom.
    FromBackup(String),
    /// Rien d'exploitable : bibliothèque vide, l'ancien fichier est conservé à côté.
    Nothing,
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Nombre de jours depuis le 1er janvier 1970 pour une date du calendrier grégorien.
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `bibliotheque-20261017-142300-auto.json`
fn backup_name(ts: i64, kind: &str) -> String {
    let (y, m, d) = crate::igdb::ymd_from_unix(ts);
    let t = ts.rem_euclid(86_400);
    format!("bibliotheque-{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{kind}.json", t / 3600, t % 3600 / 60, t % 60)
}

/// Relit date et type dans un nom de sauvegarde ; `None` si le nom n'est pas de notre fabrication
/// (ce qui sert aussi de garde-fou : aucun séparateur de chemin ne passe).
pub fn parse_backup_name(name: &str) -> Option<(i64, String)> {
    let rest = name.strip_prefix("bibliotheque-")?.strip_suffix(".json")?;
    let (date, rest) = rest.split_once('-')?;
    let (time, kind) = rest.split_once('-')?;
    let digits = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(date, 8) || !digits(time, 6) || !BACKUP_KINDS.contains(&kind) {
        return None;
    }
    let n = |s: &str| s.parse::<i64>().ok();
    let (y, mo, d) = (n(&date[..4])?, n(&date[4..6])?, n(&date[6..])?);
    let (h, mi, se) = (n(&time[..2])?, n(&time[2..4])?, n(&time[4..])?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 59 {
        return None;
    }
    Some((days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se, kind.to_string()))
}

pub fn backups_dir(dir: &Path) -> PathBuf {
    dir.join(BACKUP_DIR)
}

/// Chemin d'une sauvegarde à partir de son nom (refuse tout nom qui n'est pas une de nos sauvegardes).
pub fn backup_path(dir: &Path, file: &str) -> Result<PathBuf> {
    parse_backup_name(file).ok_or_else(|| crate::Error::Invalid(format!("nom de sauvegarde invalide : {file}")))?;
    Ok(backups_dir(dir).join(file))
}

/// Noms des sauvegardes présentes, de la plus récente à la plus ancienne.
fn backup_names(dir: &Path) -> Vec<(i64, String)> {
    let Ok(rd) = std::fs::read_dir(backups_dir(dir)) else { return Vec::new() };
    let mut v: Vec<(i64, String)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            parse_backup_name(&name).map(|(ts, _)| (ts, name))
        })
        .collect();
    v.sort_by(|a, b| b.cmp(a));
    v
}

/// Juste de quoi compter les jeux d'un fichier sans le charger en entier.
#[derive(Deserialize)]
struct Peek {
    library: PeekLibrary,
}
#[derive(Deserialize)]
struct PeekLibrary {
    entries: BTreeMap<u64, PeekEntry>,
}
#[derive(Deserialize)]
struct PeekEntry {
    deleted: bool,
}

pub fn list_backups(dir: &Path) -> Vec<BackupInfo> {
    backup_names(dir)
        .into_iter()
        .map(|(ts, file)| {
            let path = backups_dir(dir).join(&file);
            let games = std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Peek>(&b).ok())
                .map(|p| p.library.entries.values().filter(|e| !e.deleted).count());
            let kind = parse_backup_name(&file).map(|(_, k)| k).unwrap_or_default();
            BackupInfo { bytes: std::fs::metadata(&path).map_or(0, |m| m.len()), file, ts, kind, games }
        })
        .collect()
}

/// Écrit une sauvegarde (atomiquement) puis supprime les plus anciennes au-delà de `BACKUP_KEEP`.
/// Si une sauvegarde existe déjà à la même seconde, l'horodatage avance d'une seconde.
fn write_backup(dir: &Path, mut ts: i64, kind: &str, text: &str) -> Result<String> {
    if !BACKUP_KINDS.contains(&kind) {
        return Err(crate::Error::Invalid(format!("type de sauvegarde inconnu : {kind}")));
    }
    let bdir = backups_dir(dir);
    std::fs::create_dir_all(&bdir)?;
    while bdir.join(backup_name(ts, kind)).exists() {
        ts += 1;
    }
    let name = backup_name(ts, kind);
    let tmp = bdir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, bdir.join(&name))?;
    for (_, old) in backup_names(dir).into_iter().skip(BACKUP_KEEP) {
        let _ = std::fs::remove_file(bdir.join(old));
    }
    Ok(name)
}

/// Copie le fichier principal dans `sauvegardes/` s'il n'y a pas eu de sauvegarde depuis
/// `AUTO_EVERY_SECS`. Ne fait rien si la bibliothèque est vide ou le fichier illisible.
/// Appelée au lancement et avant chaque écriture : la copie est donc l'état d'avant la modification.
pub fn auto_backup(dir: &Path, now: i64) -> Option<String> {
    if let Some((last, _)) = backup_names(dir).first() {
        if (0..AUTO_EVERY_SECS).contains(&(now - last)) {
            return None;
        }
    }
    let bytes = std::fs::read(Store::path(dir)).ok()?;
    let store: Store = serde_json::from_slice(&bytes).ok()?;
    store.library.live().next()?; // bibliothèque vide : rien à protéger
    write_backup(dir, now, "auto", &String::from_utf8(bytes).ok()?).ok()
}

impl Store {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(STORE_FILE)
    }

    /// Charge le fichier. S'il est illisible, il est conservé sous `…corrompu-<n>` et on essaie, dans l'ordre,
    /// la copie `.bak` puis la sauvegarde datée la plus récente qui se lit ; à défaut, magasin vide.
    pub fn load(dir: &Path) -> Result<Store> {
        Ok(Self::load_report(dir)?.0)
    }

    pub fn load_report(dir: &Path) -> Result<(Store, Option<Recovery>)> {
        let path = Self::path(dir);
        if !path.exists() {
            return Ok((Store::default(), None));
        }
        // Une erreur de lecture (fichier verrouillé…) n'est pas une corruption : on ne touche à rien.
        let bytes = std::fs::read(&path)?;
        if let Ok(store) = serde_json::from_slice::<Store>(&bytes) {
            return Ok((store, None));
        }
        std::fs::rename(&path, dir.join(format!("{STORE_FILE}.corrompu-{}", now_secs())))?;
        let from_bak = std::fs::read(dir.join(format!("{STORE_FILE}.bak"))).ok().and_then(|b| serde_json::from_slice::<Store>(&b).ok());
        let (store, how) = match from_bak {
            Some(s) => (Some(s), Recovery::FromBak),
            None => {
                let found = backup_names(dir).into_iter().find_map(|(_, f)| Self::load_backup(dir, &f).ok().map(|s| (s, f)));
                match found {
                    Some((s, f)) => (Some(s), Recovery::FromBackup(f)),
                    None => (None, Recovery::Nothing),
                }
            }
        };
        match store {
            Some(s) => {
                s.save(dir)?;
                Ok((s, Some(how)))
            }
            None => Ok((Store::default(), Some(how))),
        }
    }

    pub fn load_backup(dir: &Path, file: &str) -> Result<Store> {
        Ok(serde_json::from_slice(&std::fs::read(backup_path(dir, file)?)?)?)
    }

    /// Sauvegarde datée de l'état courant (type : voir `BACKUP_KINDS`).
    pub fn backup(&self, dir: &Path, kind: &str) -> Result<String> {
        write_backup(dir, now_secs(), kind, &serde_json::to_string_pretty(self)?)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = Self::path(dir);
        let tmp = dir.join(format!("{STORE_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        if path.exists() {
            auto_backup(dir, now_secs());
            let _ = std::fs::copy(&path, dir.join(format!("{STORE_FILE}.bak")));
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!("panda-test-{}-{tag}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Status;

    fn game() -> GameInfo {
        GameInfo {
            id: 1,
            name: "Hades".into(),
            slug: "hades".into(),
            year: Some(2020),
            cover: Some("co1".into()),
            platforms: vec!["PC".into()],
            genres: vec![],
            summary: None,
            rating: Some(90.0),
            ..GameInfo::default()
        }
    }

    #[test]
    fn aller_retour_et_sauvegarde() {
        let dir = temp_dir("rt");
        let mut s = Store::default();
        s.games.insert(1, game());
        s.library.upsert(1, Status::Playing, None);
        s.save(&dir).unwrap();
        assert_eq!(Store::load(&dir).unwrap(), s);
        s.library.set_status(1, Status::Completed).unwrap();
        s.save(&dir).unwrap();
        let bak: Store = serde_json::from_str(&std::fs::read_to_string(dir.join("bibliotheque.json.bak")).unwrap()).unwrap();
        assert_eq!(bak.library.get(1).unwrap().status, Status::Playing, "la sauvegarde garde la version précédente");
        assert!(!dir.join("bibliotheque.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dossier_absent_donne_un_magasin_vide() {
        assert_eq!(Store::load(&temp_dir("none")).unwrap(), Store::default());
    }

    #[test]
    fn fichier_corrompu_est_conserve() {
        let dir = temp_dir("bad");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Store::path(&dir), "{ pas du json").unwrap();
        let s = Store::load(&dir).unwrap();
        assert!(s.games.is_empty());
        let kept: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(kept.iter().any(|n| n.contains("corrompu")), "{kept:?}");
        assert!(!Store::path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn store_with(n: u64) -> Store {
        let mut s = Store::default();
        for id in 1..=n {
            s.games.insert(id, GameInfo { id, name: format!("Jeu {id}"), slug: format!("jeu-{id}"), ..GameInfo::default() });
            s.library.upsert(id, Status::Backlog, None);
        }
        s
    }

    #[test]
    fn noms_de_sauvegarde_aller_retour() {
        for ts in [0, 86_399, 951_782_400, 1_792_244_580, 4_102_444_799] {
            let name = backup_name(ts, "manuelle");
            assert_eq!(parse_backup_name(&name), Some((ts, "manuelle".to_string())), "{name}");
        }
        assert_eq!(backup_name(1_792_246_980, "auto"), "bibliotheque-20261017-142300-auto.json");
        for bad in [
            "bibliotheque.json",
            "bibliotheque-20261017-142300-inconnu.json",
            "bibliotheque-20261317-142300-auto.json",
            "bibliotheque-20261017-246000-auto.json",
            "../bibliotheque-20261017-142300-auto.json",
            "bibliotheque-20261017-142300-auto.json.tmp",
        ] {
            assert_eq!(parse_backup_name(bad), None, "{bad}");
            assert!(backup_path(Path::new("x"), bad).is_err());
        }
    }

    #[test]
    fn sauvegarde_automatique_une_fois_par_jour() {
        let dir = temp_dir("auto");
        let t0 = 1_792_244_580;
        assert_eq!(auto_backup(&dir, t0), None, "rien à sauvegarder sans fichier");
        Store::default().save(&dir).unwrap();
        assert_eq!(auto_backup(&dir, t0), None, "bibliothèque vide : inutile");
        store_with(3).save(&dir).unwrap();
        let name = auto_backup(&dir, t0).expect("première sauvegarde");
        assert_eq!(parse_backup_name(&name), Some((t0, "auto".into())));
        assert_eq!(auto_backup(&dir, t0 + 3600), None, "trop tôt");
        assert!(auto_backup(&dir, t0 + AUTO_EVERY_SECS).is_some(), "le lendemain");
        let l = list_backups(&dir);
        assert_eq!(l.len(), 2);
        assert!(l[0].ts > l[1].ts, "la plus récente d'abord");
        assert_eq!(l[0].games, Some(3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn la_copie_automatique_est_l_etat_d_avant_la_modification() {
        let dir = temp_dir("avant");
        let mut s = store_with(1);
        s.save(&dir).unwrap();
        s.library.set_status(1, Status::Completed).unwrap();
        s.save(&dir).unwrap(); // première écriture du jour : copie l'ancien fichier
        let b = list_backups(&dir);
        assert_eq!(b.len(), 1);
        let old = Store::load_backup(&dir, &b[0].file).unwrap();
        assert_eq!(old.library.get(1).unwrap().status, Status::Backlog);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_et_collision_de_secondes() {
        let dir = temp_dir("rot");
        let s = store_with(1);
        let text = serde_json::to_string(&s).unwrap();
        let a = write_backup(&dir, 1_000_000, "manuelle", &text).unwrap();
        let b = write_backup(&dir, 1_000_000, "manuelle", &text).unwrap();
        assert_ne!(a, b, "même seconde : le second prend la seconde suivante");
        for i in 0..(BACKUP_KEEP as i64 + 5) {
            write_backup(&dir, 2_000_000 + i * 100, "auto", &text).unwrap();
        }
        let l = list_backups(&dir);
        assert_eq!(l.len(), BACKUP_KEEP);
        assert_eq!(l[0].ts, 2_000_000 + (BACKUP_KEEP as i64 + 4) * 100);
        assert!(l.iter().all(|b| b.ts >= 2_000_000), "les plus anciennes ont été supprimées");
        assert!(write_backup(&dir, 1, "farfelu", &text).is_err());
        assert!(std::fs::read_dir(backups_dir(&dir)).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fichier_corrompu_restaure_la_copie_bak() {
        let dir = temp_dir("rec-bak");
        let mut s = store_with(2);
        s.save(&dir).unwrap();
        s.library.set_status(1, Status::Playing).unwrap();
        s.save(&dir).unwrap(); // .bak = version précédente
        std::fs::write(Store::path(&dir), "{ coupé en pl").unwrap();
        let (got, how) = Store::load_report(&dir).unwrap();
        assert_eq!(how, Some(Recovery::FromBak));
        assert_eq!(got.library.live().count(), 2);
        assert_eq!(Store::load(&dir).unwrap(), got, "le fichier principal a été reconstitué");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fichier_corrompu_restaure_la_derniere_sauvegarde_lisible() {
        let dir = temp_dir("rec-dated");
        let s = store_with(4);
        s.backup(&dir, "manuelle").unwrap();
        // une sauvegarde plus récente mais abîmée est sautée
        write_backup(&dir, now_secs() + 100, "auto", "{ abîmée").unwrap();
        std::fs::write(Store::path(&dir), "").unwrap(); // fichier vide après un plantage
        let (got, how) = Store::load_report(&dir).unwrap();
        assert!(matches!(how, Some(Recovery::FromBackup(ref f)) if f.ends_with("-manuelle.json")), "{how:?}");
        assert_eq!(got.library.live().count(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrompu_sans_rien_a_restaurer() {
        let dir = temp_dir("rec-none");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Store::path(&dir), "n'importe quoi").unwrap();
        let (got, how) = Store::load_report(&dir).unwrap();
        assert_eq!(how, Some(Recovery::Nothing));
        assert!(got.games.is_empty());
        assert!(std::fs::read_dir(&dir).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("corrompu")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
