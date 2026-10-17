//! Bibliothèque personnelle (backlog) — CONCEPTION PROPRE.
//!
//! Aucun point d'accès authentifié de Backloggd n'a pu être observé (aucun compte utilisé) : ni le
//! modèle de stockage des notes, ni le protocole de synchronisation du site ne sont connus.
//! Ce module propose donc un modèle minimal et vérifiable : une entrée par jeu, avec un tampon
//! de Lamport `(compteur, réplica)` ; la fusion « dernier écrivain gagnant » est commutative,
//! associative et idempotente (tests dans `tests/sync_properties.rs`).

use crate::error::{Error, Result};
use crate::rating::{Histogram, Rating};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Backlog,
    Playing,
    Played,
    Completed,
    Abandoned,
    Wishlist,
}

impl Status {
    /// Le jeu n'est plus en cours : terminé, joué ou abandonné (ces statuts portent une date de fin).
    pub fn is_ended(self) -> bool {
        matches!(self, Status::Completed | Status::Played | Status::Abandoned)
    }

    /// Libellé affiché dans l'interface et dans l'export CSV.
    pub fn label_fr(self) -> &'static str {
        match self {
            Status::Backlog => "Backlog",
            Status::Playing => "En cours",
            Status::Played => "Joué",
            Status::Completed => "Terminé",
            Status::Abandoned => "Abandonné",
            Status::Wishlist => "Envie",
        }
    }
}

impl FromStr for Status {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s.to_lowercase().as_str() {
            "backlog" => Status::Backlog,
            "playing" => Status::Playing,
            "played" => Status::Played,
            "completed" => Status::Completed,
            "abandoned" => Status::Abandoned,
            "wishlist" => Status::Wishlist,
            other => return Err(Error::InvalidStatus(other.to_string())),
        })
    }
}

/// Tampon de Lamport ; l'ordre dérivé (compteur puis réplica) est total.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Stamp {
    pub counter: u64,
    pub replica: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub game_id: u64,
    pub status: Status,
    pub rating: Option<Rating>,
    /// Plateforme sur laquelle l'utilisateur joue (ou a joué) à ce jeu.
    #[serde(default)]
    pub platform: Option<String>,
    /// Commentaire personnel (texte libre).
    #[serde(default)]
    pub comment: Option<String>,
    /// Temps de jeu en minutes (saisi par l'utilisateur).
    #[serde(default)]
    pub minutes: Option<u32>,
    /// Date d'ajout à la bibliothèque (horodatage Unix) ; absente pour les jeux ajoutés avant cette fonction.
    #[serde(default)]
    pub added: Option<i64>,
    /// Date de fin (terminé, joué ou abandonné), posée automatiquement au changement de statut.
    #[serde(default)]
    pub finished: Option<i64>,
    /// Pierre tombale : la suppression doit aussi se synchroniser.
    pub deleted: bool,
    pub stamp: Stamp,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// Entrées distantes plus récentes, adoptées.
    pub adopted: usize,
    /// Entrées locales plus récentes ou identiques, conservées.
    pub kept: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub replica: String,
    pub clock: u64,
    pub entries: BTreeMap<u64, Entry>,
}

/// Date de fin après un statut : conservée si le jeu est déjà fini (changer « Terminé » en « Abandonné » ne la
/// déplace pas), posée à `now` en entrant dans un statut de fin, effacée si le jeu reprend ou retourne en attente.
fn finish_for(status: Status, current: Option<i64>, now: i64) -> Option<i64> {
    status.is_ended().then(|| current.unwrap_or(now))
}

impl Library {
    pub fn new(replica: &str) -> Self {
        Library { replica: replica.to_string(), clock: 0, entries: BTreeMap::new() }
    }

    fn tick(&mut self) -> Stamp {
        self.clock += 1;
        Stamp { counter: self.clock, replica: self.replica.clone() }
    }

    /// Crée ou remplace l'entrée (réactive une entrée supprimée). Plateforme et commentaire d'une
    /// entrée vivante sont conservés ; une entrée supprimée repart de zéro.
    pub fn upsert(&mut self, game_id: u64, status: Status, rating: Option<Rating>) {
        let (platform, comment, minutes) = self.get(game_id).map(|e| (e.platform.clone(), e.comment.clone(), e.minutes)).unwrap_or_default();
        self.upsert_full(game_id, status, rating, platform, comment, minutes);
    }

    /// Les dates : l'entrée vivante garde les siennes (une entrée absente ou supprimée est « ajoutée maintenant ») ;
    /// la date de fin suit le statut (voir `finish_for`).
    pub fn upsert_full(&mut self, game_id: u64, status: Status, rating: Option<Rating>, platform: Option<String>, comment: Option<String>, minutes: Option<u32>) {
        let now = crate::store::now_secs();
        let prior = self.get(game_id).cloned();
        let added = prior.as_ref().map_or(Some(now), |e| e.added);
        let finished = finish_for(status, prior.as_ref().and_then(|e| e.finished), now);
        let stamp = self.tick();
        self.entries.insert(game_id, Entry { game_id, status, rating, platform, comment, minutes, added, finished, deleted: false, stamp });
    }

    /// Modifie une entrée vivante et avance son tampon.
    fn update(&mut self, game_id: u64, f: impl FnOnce(&mut Entry)) -> Result<()> {
        let mut entry = self.get(game_id).ok_or(Error::NotInLibrary(game_id))?.clone();
        f(&mut entry);
        entry.stamp = self.tick();
        self.entries.insert(game_id, entry);
        Ok(())
    }

    /// Un vrai changement de statut met la date de fin à jour ; re-choisir le même statut ne touche à rien.
    pub fn set_status(&mut self, game_id: u64, status: Status) -> Result<()> {
        self.update(game_id, |e| {
            if e.status != status {
                e.finished = finish_for(status, e.finished, crate::store::now_secs());
            }
            e.status = status;
        })
    }

    /// Fixe les dates à la main. `None` : inchangée ; `Some(None)` : effacée.
    pub fn set_dates(&mut self, game_id: u64, added: Option<Option<i64>>, finished: Option<Option<i64>>) -> Result<()> {
        self.update(game_id, |e| {
            if let Some(a) = added {
                e.added = a;
            }
            if let Some(f) = finished {
                e.finished = f;
            }
        })
    }

    pub fn set_rating(&mut self, game_id: u64, rating: Option<Rating>) -> Result<()> {
        self.update(game_id, |e| e.rating = rating)
    }

    pub fn set_platform(&mut self, game_id: u64, platform: Option<String>) -> Result<()> {
        self.update(game_id, |e| e.platform = platform)
    }

    pub fn set_minutes(&mut self, game_id: u64, minutes: Option<u32>) -> Result<()> {
        self.update(game_id, |e| e.minutes = minutes)
    }

    pub fn set_comment(&mut self, game_id: u64, comment: Option<String>) -> Result<()> {
        self.update(game_id, |e| e.comment = comment)
    }

    pub fn remove(&mut self, game_id: u64) -> Result<()> {
        let mut entry = self.get(game_id).ok_or(Error::NotInLibrary(game_id))?.clone();
        entry.deleted = true;
        entry.stamp = self.tick();
        self.entries.insert(game_id, entry);
        Ok(())
    }

    /// Entrée vivante (non supprimée).
    pub fn get(&self, game_id: u64) -> Option<&Entry> {
        self.entries.get(&game_id).filter(|e| !e.deleted)
    }

    pub fn live(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values().filter(|e| !e.deleted)
    }

    pub fn histogram(&self) -> Histogram {
        let mut h = Histogram::default();
        for r in self.live().filter_map(|e| e.rating) {
            h.add(r);
        }
        h
    }

    /// Fusion « dernier écrivain gagnant » par entrée. À tampon égal, l'entrée locale est conservée
    /// (ne se produit que si deux appareils partagent le même nom de réplica : à éviter).
    pub fn merge(&mut self, other: &Library) -> MergeReport {
        let mut report = MergeReport::default();
        for (id, theirs) in &other.entries {
            match self.entries.get(id) {
                Some(mine) if mine.stamp >= theirs.stamp => report.kept += 1,
                _ => {
                    self.entries.insert(*id, theirs.clone());
                    report.adopted += 1;
                }
            }
        }
        self.clock = self.clock.max(other.clock);
        report
    }

    pub fn load(path: &Path) -> Result<Self> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: f64) -> Option<Rating> {
        Some(Rating::from_stars(s).unwrap())
    }

    #[test]
    fn cycle_de_vie_d_une_entree() {
        let mut l = Library::new("a");
        assert_eq!(l.set_rating(1, r(4.0)), Err(Error::NotInLibrary(1)));
        l.upsert(1, Status::Backlog, None);
        l.set_status(1, Status::Playing).unwrap();
        l.set_rating(1, r(4.5)).unwrap();
        assert_eq!(l.get(1).unwrap().status, Status::Playing);
        assert_eq!(l.get(1).unwrap().rating, r(4.5));
        l.set_rating(1, None).unwrap();
        assert_eq!(l.get(1).unwrap().rating, None);
        l.remove(1).unwrap();
        assert!(l.get(1).is_none());
        assert_eq!(l.remove(1), Err(Error::NotInLibrary(1)));
        l.upsert(1, Status::Wishlist, None);
        assert!(l.get(1).is_some());
        assert_eq!(l.clock, 6);
    }

    #[test]
    fn statut_depuis_texte() {
        assert_eq!("Playing".parse::<Status>().unwrap(), Status::Playing);
        assert!("xyz".parse::<Status>().is_err());
    }

    #[test]
    fn histogramme_ignore_supprimes_et_non_notes() {
        let mut l = Library::new("a");
        l.upsert(1, Status::Played, r(5.0));
        l.upsert(2, Status::Played, r(5.0));
        l.upsert(3, Status::Backlog, None);
        l.remove(2).unwrap();
        let h = l.histogram();
        assert_eq!(h.total(), 1);
        assert_eq!(h.counts[9], 1);
    }

    #[test]
    fn fusion_garde_le_plus_recent() {
        let mut a = Library::new("a");
        let mut b = Library::new("b");
        a.upsert(1, Status::Playing, None);
        b.merge(&a);
        b.set_rating(1, r(3.0)).unwrap();
        a.set_status(1, Status::Completed).unwrap();
        let (mut a2, mut b2) = (a.clone(), b.clone());
        a2.merge(&b);
        b2.merge(&a);
        assert_eq!(a2.entries, b2.entries);
        assert_eq!(a2.get(1).unwrap().stamp.counter, 2);
    }

    fn near_now(t: Option<i64>) -> bool {
        t.is_some_and(|t| (crate::store::now_secs() - t).abs() < 60)
    }

    #[test]
    fn dates_d_ajout_et_de_fin_automatiques() {
        let mut l = Library::new("a");
        l.upsert(1, Status::Backlog, None);
        let e = l.get(1).unwrap();
        assert!(near_now(e.added) && e.finished.is_none(), "ajout daté, pas de fin");
        l.set_dates(1, Some(Some(1_000_000_000)), None).unwrap();
        l.set_status(1, Status::Playing).unwrap();
        assert!(l.get(1).unwrap().finished.is_none());
        l.set_status(1, Status::Completed).unwrap();
        assert!(near_now(l.get(1).unwrap().finished), "fin posée en passant à Terminé");
        assert_eq!(l.get(1).unwrap().added, Some(1_000_000_000), "la date d'ajout ne bouge pas");
        // la fin n'est pas déplacée en passant d'un statut de fin à un autre, ni en re-choisissant le même
        l.set_dates(1, None, Some(Some(1_100_000_000))).unwrap();
        l.set_status(1, Status::Abandoned).unwrap();
        l.set_status(1, Status::Abandoned).unwrap();
        l.set_status(1, Status::Played).unwrap();
        assert_eq!(l.get(1).unwrap().finished, Some(1_100_000_000));
        // reprendre ou remettre en attente efface la fin ; une nouvelle fin sera datée à nouveau
        l.set_status(1, Status::Playing).unwrap();
        assert_eq!(l.get(1).unwrap().finished, None);
        l.set_status(1, Status::Completed).unwrap();
        assert!(near_now(l.get(1).unwrap().finished));
        // effacement manuel
        l.set_dates(1, Some(None), Some(None)).unwrap();
        let e = l.get(1).unwrap();
        assert_eq!((e.added, e.finished), (None, None));
        assert_eq!(l.set_dates(9, Some(None), None), Err(Error::NotInLibrary(9)));
    }

    #[test]
    fn re_ajouter_un_jeu_retire_repart_de_zero_mais_pas_un_jeu_present() {
        let mut l = Library::new("a");
        l.upsert(1, Status::Completed, None);
        l.set_dates(1, Some(Some(1_000_000_000)), Some(Some(1_100_000_000))).unwrap();
        l.upsert(1, Status::Completed, None); // jeu présent : dates conservées
        assert_eq!((l.get(1).unwrap().added, l.get(1).unwrap().finished), (Some(1_000_000_000), Some(1_100_000_000)));
        l.remove(1).unwrap();
        l.upsert(1, Status::Backlog, None); // jeu retiré puis ré-ajouté : nouvelle date d'ajout
        assert!(near_now(l.get(1).unwrap().added));
    }

    #[test]
    fn anciennes_entrees_sans_dates_se_lisent() {
        let old = r#"{"game_id":1,"status":"completed","rating":null,"deleted":false,"stamp":{"counter":3,"replica":"a"}}"#;
        let e: Entry = serde_json::from_str(old).unwrap();
        assert_eq!((e.added, e.finished), (None, None));
        let mut l = Library::new("a");
        l.entries.insert(1, e);
        l.set_status(1, Status::Completed).unwrap(); // même statut : on n'invente pas de date
        assert_eq!(l.get(1).unwrap().finished, None);
    }
}
