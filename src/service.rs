//! Service applicatif : toute la logique métier derrière l'interface (fenêtre Tauri ou navigateur local).
//!
//! L'interface n'appelle que `Service::call(méthode, arguments)` et reçoit du JSON
//! `{"ok":true,"result":…}` ou `{"ok":false,"error":"…"}`. Méthodes :
//! `status`, `search`, `library`, `add`, `set_status`, `set_rating`, `set_platform`, `set_comment`,
//! `remove`, `refresh`, `hltb_url`, `translate`, `stats`, `get_settings`, `save_settings`, `test_igdb`,
//! `sync_metadata`, `export`.

use crate::catalog::{Catalog, Game};
use crate::config::{Config, Storage};
use crate::csv_import::{self, fold, CsvRow};
use crate::secrets::Vault;
use crate::deepl::Translator;
use crate::error::{Error, Result};
use crate::igdb::{GameInfo, IgdbClient, TimeToBeat, Transport, REV_DETAILS, REV_FULL};
use crate::l10n::{date_fr, duration_fr, estimate_fr, parse_duration, tag_fr};
use crate::library::{Entry, Status};
use crate::rating::{Histogram, Rating};
use crate::search::autocomplete;
use crate::store::{self, Recovery, Store};
use crate::transfer::{self, ImportMode};
use crate::text::{normalize, url_encode};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const STATUSES: [Status; 6] =
    [Status::Backlog, Status::Playing, Status::Completed, Status::Played, Status::Abandoned, Status::Wishlist];
const SEARCH_LIMIT: usize = 24;
const RECENT_MAX: usize = 600;
const COMMENT_MAX_CHARS: usize = 10_000;
const PLATFORM_MAX_CHARS: usize = 60;
const STATS_GROUP_MAX: usize = 8;
const FACET_MAX: usize = 80;

fn status_key(s: Status) -> &'static str {
    match s {
        Status::Backlog => "backlog",
        Status::Playing => "playing",
        Status::Completed => "completed",
        Status::Played => "played",
        Status::Abandoned => "abandoned",
        Status::Wishlist => "wishlist",
    }
}

#[derive(Serialize)]
struct EntryView {
    status: Status,
    half: Option<u8>,
    stars: Option<f64>,
    updated: u64,
    platform: Option<String>,
    comment: Option<String>,
    minutes: Option<u32>,
    hours_text: Option<String>,
    /// Dates (secondes Unix) d'ajout et de fin.
    added: Option<i64>,
    finished: Option<i64>,
}

/// Durées « time to beat » prêtes à afficher : secondes pour le tri, texte arrondi pour la carte.
#[derive(Serialize)]
struct TtbView {
    hastily: Option<u32>,
    normally: Option<u32>,
    completely: Option<u32>,
    hastily_text: Option<String>,
    normally_text: Option<String>,
    completely_text: Option<String>,
    count: u32,
    /// Moins de 5 joueurs : la durée est à prendre avec prudence.
    few_votes: bool,
}

impl TtbView {
    fn new(t: &TimeToBeat) -> Self {
        let text = |v: Option<u32>| v.map(estimate_fr);
        TtbView {
            hastily: t.hastily,
            normally: t.normally,
            completely: t.completely,
            hastily_text: text(t.hastily),
            normally_text: text(t.normally),
            completely_text: text(t.completely),
            count: t.count,
            few_votes: t.count < 5,
        }
    }
}

/// Lien de recherche HowLongToBeat (hôte fixe, nom encodé) pour comparer les durées.
pub fn hltb_url(name: &str) -> String {
    format!("https://howlongtobeat.com/?q={}", url_encode(name))
}

#[derive(Serialize)]
struct Card {
    id: u64,
    name: String,
    slug: String,
    year: Option<i32>,
    released: Option<String>,
    cover_url: Option<String>,
    cover_url_2x: Option<String>,
    platforms: Vec<String>,
    genres: Vec<String>,
    themes: Vec<String>,
    modes: Vec<String>,
    perspectives: Vec<String>,
    developers: Vec<String>,
    publishers: Vec<String>,
    franchise: Option<String>,
    summary: Option<String>,
    summary_fr: Option<String>,
    igdb_rating: Option<f64>,
    /// Niveau de détail : 3 = fiche complète avec durées ; 2 = détaillée sans durées ; moins = à compléter avec IGDB.
    rev: u8,
    ttb: Option<TtbView>,
    /// Durée de référence en secondes (normale, sinon rapide, sinon complète).
    ttb_ref: Option<u32>,
    ttb_text: Option<String>,
    hltb_url: String,
    entry: Option<EntryView>,
}

fn fr_list(v: &[String]) -> Vec<String> {
    v.iter().map(|t| tag_fr(t)).collect()
}

fn card(info: &GameInfo, entry: Option<&Entry>) -> Card {
    let ttb_ref = info.time_to_beat.as_ref().and_then(TimeToBeat::reference);
    Card {
        id: info.id,
        name: info.name.clone(),
        slug: info.slug.clone(),
        year: info.year,
        released: info.released.map(date_fr),
        cover_url: info.cover_url("cover_big"),
        cover_url_2x: info.cover_url("cover_big_2x"),
        platforms: info.platforms.clone(),
        genres: fr_list(&info.genres),
        themes: fr_list(&info.themes),
        modes: fr_list(&info.modes),
        perspectives: fr_list(&info.perspectives),
        developers: info.developers.clone(),
        publishers: info.publishers.clone(),
        franchise: info.franchise.clone(),
        summary: info.summary.clone(),
        summary_fr: info.summary_fr.clone(),
        igdb_rating: info.rating,
        rev: info.rev,
        ttb: info.time_to_beat.as_ref().filter(|t| t.reference().is_some()).map(TtbView::new),
        ttb_ref,
        ttb_text: ttb_ref.map(estimate_fr),
        hltb_url: hltb_url(&info.name),
        entry: entry.map(|e| EntryView {
            status: e.status,
            half: e.rating.map(Rating::half_stars),
            stars: e.rating.map(Rating::stars),
            updated: e.stamp.counter,
            platform: e.platform.clone(),
            comment: e.comment.clone(),
            minutes: e.minutes,
            hours_text: e.minutes.map(duration_fr),
            added: e.added,
            finished: e.finished,
        }),
    }
}

fn offline_info(g: &Game) -> GameInfo {
    GameInfo { id: g.id, name: g.title.clone(), slug: g.slug.clone(), year: g.year.parse().ok(), ..GameInfo::default() }
}

/// Reprend ce que la fiche stockée a déjà : la traduction si le résumé n'a pas changé, et les durées
/// quand la nouvelle fiche est détaillée mais sans durées (`rev == REV_DETAILS` : requête des durées
/// échouée) pour ne pas les perdre. Une fiche plus pauvre (`rev < REV_DETAILS`) garde son niveau, pour
/// que l'interface sache qu'elle reste à compléter ; une fiche `REV_FULL` fait foi.
fn keep_cached(old: Option<&GameInfo>, new: &mut GameInfo) {
    let Some(old) = old else { return };
    if old.summary == new.summary {
        new.summary_fr = old.summary_fr.clone();
    }
    if new.rev == REV_DETAILS && old.time_to_beat.is_some() {
        new.time_to_beat = old.time_to_beat.clone();
        new.rev = old.rev;
    }
}

fn clean_text(v: &Value, max_chars: usize, what: &str) -> Result<Option<String>> {
    let Some(s) = v.as_str() else { return Ok(None) };
    let s = s.trim();
    if s.chars().count() > max_chars {
        return Err(Error::Invalid(format!("{what} trop long ({max_chars} caractères au maximum)")));
    }
    Ok((!s.is_empty()).then(|| s.to_string()))
}

#[derive(Default)]
struct Filters {
    genre: Option<String>,
    platform: Option<String>,
    developer: Option<String>,
    min_half: u8,
    /// Tranche de durée estimée en secondes, `[min, max)` ; `None` = pas de filtre.
    ttb: Option<(u32, Option<u32>)>,
}

impl Filters {
    fn from_args(args: &Value) -> Filters {
        let get = |k: &str| args.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let min = args.get("min_rating").and_then(Value::as_f64).unwrap_or(0.0);
        const H: u32 = 3600;
        let ttb = match get("ttb").as_deref() {
            Some("lt5") => Some((0, Some(5 * H))),
            Some("5to15") => Some((5 * H, Some(15 * H))),
            Some("15to40") => Some((15 * H, Some(40 * H))),
            Some("ge40") => Some((40 * H, None)),
            _ => None,
        };
        Filters { genre: get("genre"), platform: get("platform"), developer: get("developer"), min_half: (min * 2.0).round().clamp(0.0, 10.0) as u8, ttb }
    }

    fn keeps(&self, info: &GameInfo, e: &Entry) -> bool {
        self.genre.as_ref().is_none_or(|g| info.genres.iter().any(|x| &tag_fr(x) == g))
            && self.platform.as_ref().is_none_or(|p| played_on(info, e).contains(p))
            && self.developer.as_ref().is_none_or(|d| info.developers.contains(d))
            && (self.min_half == 0 || e.rating.is_some_and(|r| r.half_stars() >= self.min_half))
            && self.ttb.is_none_or(|(min, max)| {
                info.time_to_beat.as_ref().and_then(|t| t.reference()).is_some_and(|d| d >= min && max.is_none_or(|m| d < m))
            })
    }
}

/// Plateformes retenues pour le filtre : celle choisie par l'utilisateur, sinon celles du jeu.
fn played_on(info: &GameInfo, e: &Entry) -> Vec<String> {
    match &e.platform {
        Some(p) => vec![p.clone()],
        None => info.platforms.clone(),
    }
}

/// Valeurs distinctes avec leur effectif, les plus fréquentes d'abord.
fn facet(values: impl Iterator<Item = String>) -> Vec<Value> {
    let mut counts: Vec<(String, u64)> = Vec::new();
    for v in values {
        match counts.iter_mut().find(|(n, _)| *n == v) {
            Some((_, c)) => *c += 1,
            None => counts.push((v, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    counts.into_iter().take(FACET_MAX).map(|(name, count)| json!({ "name": name, "count": count })).collect()
}

/// Regroupement pour les statistiques : effectif, note moyenne et temps de jeu par valeur.
fn groups(rows: &[(GameInfo, &Entry)], keys: impl Fn(&GameInfo, &Entry) -> Vec<String>) -> Vec<Value> {
    #[derive(Default)]
    struct Acc {
        count: u64,
        stars: f64,
        rated: u64,
        minutes: u64,
    }
    let mut map: Vec<(String, Acc)> = Vec::new();
    for (g, e) in rows {
        let mut seen: Vec<String> = Vec::new();
        for k in keys(g, e) {
            if seen.contains(&k) {
                continue;
            }
            seen.push(k.clone());
            let i = match map.iter().position(|(n, _)| *n == k) {
                Some(i) => i,
                None => {
                    map.push((k, Acc::default()));
                    map.len() - 1
                }
            };
            let a = &mut map[i].1;
            a.count += 1;
            a.minutes += u64::from(e.minutes.unwrap_or(0));
            if let Some(r) = e.rating {
                a.stars += r.stars();
                a.rated += 1;
            }
        }
    }
    map.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    map.into_iter()
        .take(STATS_GROUP_MAX)
        .map(|(name, a)| {
            json!({
                "name": name,
                "count": a.count,
                "average": (a.rated > 0).then(|| (a.stars / a.rated as f64 * 10.0).round() / 10.0),
                "minutes": a.minutes,
                "hours_text": (a.minutes > 0).then(|| duration_fr(a.minutes.min(u64::from(u32::MAX)) as u32)),
            })
        })
        .collect()
}

struct State {
    store: Store,
    config: Config,
    /// Où sont rangés les secrets en ce moment (coffre du système ou fichier).
    storage: Storage,
    igdb: Option<Arc<IgdbClient>>,
    translator: Option<Arc<Translator>>,
    /// Fiches vues dans les dernières recherches : l'interface n'envoie que des identifiants.
    recent: HashMap<u64, GameInfo>,
    /// Message à montrer une fois à l'ouverture (bibliothèque restaurée après un fichier illisible).
    notice: Option<String>,
    /// Import CSV en cours : lignes lues et résultat du rapprochement (rien n'est ajouté avant `csv_apply`).
    staged: Option<Staged>,
}

/// Résultat du rapprochement d'une ligne de CSV avec un jeu.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum MatchKind {
    /// Un seul jeu porte exactement ce titre (et cette année si elle est donnée).
    Sure,
    /// Plusieurs possibilités, ou titre approchant : à vérifier.
    Check,
    NotFound,
    /// Le jeu est déjà dans la bibliothèque.
    Present,
}

struct RowMatch {
    kind: MatchKind,
    /// Cinq jeux au plus, le meilleur d'abord.
    candidates: Vec<GameInfo>,
}

struct Staged {
    rows: Vec<CsvRow>,
    matches: Vec<Option<RowMatch>>,
}

pub struct Service {
    dir: PathBuf,
    /// Où `export_file` dépose ses fichiers.
    export_dir: PathBuf,
    /// Pause entre deux recherches IGDB lors d'un import CSV (IGDB limite à 4 requêtes par seconde).
    match_pause: std::time::Duration,
    vault: Vault,
    transport: Arc<dyn Transport>,
    offline: Catalog,
    state: Mutex<State>,
}

fn arg_u64(args: &Value, key: &str) -> Result<u64> {
    args.get(key).and_then(Value::as_u64).ok_or_else(|| Error::Invalid(format!("« {key} » manquant ou non numérique")))
}

fn arg_status(args: &Value, key: &str) -> Result<Option<Status>> {
    match args.get(key).and_then(Value::as_str) {
        None => Ok(None),
        Some(s) => s.parse().map(Some),
    }
}

/// `Some(None)` = note effacée (`null`), `None` = champ absent.
/// Minutes brutes (entier), telles que renvoyées dans `entry.minutes` : sert à rétablir une entrée.
fn arg_raw_minutes(v: &Value) -> Result<Option<u32>> {
    match v {
        Value::Null => Ok(None),
        Value::Number(n) => n.as_u64().filter(|m| *m <= 99_999 * 60).map(|m| (m > 0).then_some(m as u32)).ok_or_else(|| Error::Invalid("minutes : entier positif attendu".into())),
        _ => Err(Error::Invalid("minutes : entier attendu".into())),
    }
}

/// Date saisie : secondes Unix entre 1980 et dans deux jours (marge de fuseau). `None` : champ absent ;
/// `Some(None)` : effacée (`null`).
fn arg_date(args: &Value, key: &str) -> Result<Option<Option<i64>>> {
    const MIN: i64 = 315_532_800;
    match args.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => {
            let t = v.as_i64().filter(|t| (MIN..=store::now_secs() + 2 * 86_400).contains(t));
            t.map(|t| Some(Some(t))).ok_or_else(|| Error::Invalid(format!("« {key} » : date invalide (entre 1980 et aujourd'hui)")))
        }
    }
}

/// Durée en minutes : nombre d'heures (12.5) ou texte (« 12h30 », « 45 min »). `null` ou vide : effacer.
fn arg_minutes(v: &Value) -> Result<Option<u32>> {
    match v {
        Value::Null => Ok(None),
        Value::Number(n) => {
            let h = n.as_f64().unwrap_or(f64::NAN);
            parse_duration(&h.to_string()).map_err(Error::Invalid)
        }
        Value::String(t) => parse_duration(t).map_err(Error::Invalid),
        _ => Err(Error::Invalid("durée : nombre d'heures ou texte attendu".into())),
    }
}

fn arg_rating(args: &Value) -> Result<Option<Option<Rating>>> {
    match args.get("rating") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => {
            let stars = v.as_f64().ok_or_else(|| Error::InvalidRating(v.to_string()))?;
            Ok(Some(Some(Rating::from_stars(stars)?)))
        }
    }
}

impl Service {
    pub fn open(dir: &Path, transport: Arc<dyn Transport>) -> Result<Service> {
        let mut svc = Self::open_with(dir, transport, Vault::system())?;
        svc.export_dir = transfer::system_export_dir(dir);
        if !cfg!(test) {
            svc.match_pause = std::time::Duration::from_millis(280);
        }
        Ok(svc)
    }

    /// Comme `open`, avec un coffre choisi (tests ; `Vault::none()` pour tout garder dans le fichier).
    pub fn open_with(dir: &Path, transport: Arc<dyn Transport>, vault: Vault) -> Result<Service> {
        let (store, recovery) = Store::load_report(dir)?;
        if recovery.is_none() {
            store::auto_backup(dir, store::now_secs());
        }
        let notice = recovery.map(|r| recovery_notice(&r));
        let (config, storage) = Config::load_with(dir, &vault);
        let offline = Catalog::from_json(include_str!("../data/catalogue_hors_ligne.json"))?;
        let svc = Service {
            dir: dir.to_path_buf(),
            export_dir: dir.join("exports"),
            match_pause: std::time::Duration::ZERO,
            vault,
            transport,
            offline,
            state: Mutex::new(State { store, config, storage, igdb: None, translator: None, recent: HashMap::new(), notice, staged: None }),
        };
        {
            let mut st = svc.lock()?;
            svc.rebuild(&mut st);
        }
        Ok(svc)
    }

    /// Remplit la bibliothèque avec quelques jeux du catalogue intégré (pour `app --demo` et les captures).
    pub fn seed_demo(&self) -> Result<usize> {
        // (titre, statut, note en demi-étoiles, plateforme, commentaire)
        const DEMO: [(&str, Status, u8, &str, &str); 18] = [
            ("Hades", Status::Completed, 10, "PC", "Les dialogues avec la famille de Zagreus rendent chaque tentative différente."),
            ("Hades II", Status::Playing, 0, "PC", ""),
            ("The Legend of Zelda: Tears of the Kingdom", Status::Playing, 0, "Switch", "Je bloque dans les profondeurs, à revoir au calme."),
            ("Horizon Zero Dawn", Status::Completed, 9, "PS5", ""),
            ("Kentucky Route Zero", Status::Played, 8, "PC", "Magnifique, à relire un jour en entier."),
            ("Katana Zero", Status::Completed, 9, "PC", ""),
            ("The Legend of Zelda: Breath of the Wild", Status::Completed, 10, "Switch", ""),
            ("The Legend of Zelda: Majora's Mask", Status::Backlog, 0, "", ""),
            ("The Legend of Zelda: Echoes of Wisdom", Status::Backlog, 0, "Switch", ""),
            ("Resident Evil Zero", Status::Abandoned, 5, "GameCube", "Trop de va-et-vient pour moi."),
            ("F-Zero GX", Status::Played, 7, "GameCube", ""),
            ("Mega Man Zero", Status::Wishlist, 0, "", ""),
            ("Star Fox Zero", Status::Abandoned, 4, "Wii U", ""),
            ("Zero Escape: Virtue's Last Reward", Status::Wishlist, 0, "", ""),
            ("Donkey Kong Country: Tropical Freeze", Status::Playing, 0, "Switch", ""),
            ("Tales of Zestiria", Status::Backlog, 0, "", ""),
            ("The Legend of Zelda: The Wind Waker HD", Status::Completed, 9, "Wii U", ""),
            ("Suzerain", Status::Backlog, 0, "PC", ""),
        ];
        let mut st = self.lock()?;
        let mut n = 0;
        let now = store::now_secs();
        for (i, (title, status, half, platform, comment)) in DEMO.into_iter().enumerate() {
            let Some(g) = self.offline.games().iter().find(|g| g.title == title) else { continue };
            let rating = if half > 0 { Some(Rating::from_half_stars(half)?) } else { None };
            let some = |t: &str| (!t.is_empty()).then(|| t.to_string());
            st.store.games.insert(g.id, offline_info(g));
            let minutes = match status {
                Status::Backlog | Status::Wishlist => None,
                Status::Playing => Some(180 + (i as u32 * 97) % 900),
                Status::Abandoned => Some(120 + (i as u32 * 61) % 400),
                _ => Some(600 + (i as u32 * 523) % 5400),
            };
            st.store.library.upsert_full(g.id, status, rating, some(platform), some(comment), minutes);
            // dates étalées sur deux ans, pour que les statistiques d'ancienneté aient de quoi montrer
            let day = 86_400;
            let added = now - (12 + i as i64 * 41) * day;
            let finished = status.is_ended().then(|| (added + (9 + i as i64 * 13) * day).min(now - day));
            st.store.library.set_dates(g.id, Some(Some(added)), Some(finished))?;
            n += 1;
        }
        self.persist(&st)?;
        Ok(n)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| Error::Io("état interne verrouillé (erreur précédente)".into()))
    }

    fn rebuild(&self, st: &mut State) {
        let (cfg, _) = st.config.effective();
        st.igdb = cfg
            .is_complete()
            .then(|| Arc::new(IgdbClient::new(self.transport.clone(), &cfg.igdb_client_id, &cfg.igdb_client_secret)));
        st.translator = (!cfg.deepl_key.trim().is_empty()).then(|| Arc::new(Translator::new(self.transport.clone(), &cfg.deepl_key)));
    }

    fn persist(&self, st: &State) -> Result<()> {
        st.store.save(&self.dir)
    }

    fn remember(st: &mut State, games: &[GameInfo]) {
        if st.recent.len() + games.len() > RECENT_MAX {
            st.recent.clear();
        }
        for g in games {
            st.recent.insert(g.id, g.clone());
        }
    }

    fn find_info(&self, st: &State, id: u64) -> Option<GameInfo> {
        st.recent
            .get(&id)
            .or_else(|| st.store.games.get(&id))
            .cloned()
            .or_else(|| self.offline.get(id).map(offline_info))
    }

    /// Point d'entrée unique pour les deux enveloppes (navigateur local et Tauri).
    pub fn call(&self, method: &str, args: &Value) -> Value {
        match self.dispatch(method, args) {
            Ok(v) => json!({ "ok": true, "result": v }),
            Err(e) => json!({ "ok": false, "error": e.to_string() }),
        }
    }

    pub fn dispatch(&self, method: &str, args: &Value) -> Result<Value> {
        match method {
            "status" => self.status(),
            "search" => self.search(args),
            "library" => self.library(args),
            "add" => self.add(args),
            "set_status" => self.set_status(args),
            "set_rating" => self.set_rating(args),
            "set_platform" => self.set_platform(args),
            "set_comment" => self.set_comment(args),
            "set_hours" => self.set_hours(args),
            "set_dates" => self.set_dates(args),
            "refresh" => self.refresh(args),
            "hltb_url" => self.hltb_url_rpc(args),
            "translate" => self.translate(args),
            "remove" => self.remove(args),
            "stats" => self.stats(),
            "get_settings" => self.get_settings(),
            "save_settings" => self.save_settings(args),
            "test_igdb" => self.test_igdb(),
            "sync_metadata" => self.sync_metadata(),
            "export" => Ok(serde_json::to_value(&self.lock()?.store)?),
            "export_file" => self.export_file(args),
            "import_preview" => self.import_preview(args),
            "import_apply" => self.import_apply(args),
            "backups" => self.backups(),
            "backup_now" => self.backup_now(),
            "restore_backup" => self.restore_backup(args),
            "reveal" => self.reveal(args),
            "csv_start" => self.csv_start(args),
            "csv_match" => self.csv_match(args),
            "csv_review" => self.csv_review(),
            "csv_apply" => self.csv_apply(args),
            "csv_cancel" => {
                self.lock()?.staged = None;
                Ok(json!({ "ok": true }))
            }
            other => Err(Error::Invalid(format!("méthode inconnue : {other}"))),
        }
    }

    fn status(&self) -> Result<Value> {
        let mut st = self.lock()?;
        Ok(json!({
            "notice": st.notice.take(),
            "mode": if st.igdb.is_some() { "igdb" } else { "offline" },
            "in_library": st.store.library.live().count(),
            "data_dir": self.dir.display().to_string(),
        }))
    }

    fn search(&self, args: &Value) -> Result<Value> {
        let query = args.get("query").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let query: String = query.chars().take(80).collect();
        let igdb = self.lock()?.igdb.clone();
        let mode = if igdb.is_some() { "igdb" } else { "offline" };
        if normalize(&query).chars().count() < 2 {
            return Ok(json!({ "source": mode, "results": [], "warning": null }));
        }
        let (source, games, warning) = match igdb {
            Some(client) => match client.search(&query, SEARCH_LIMIT) {
                Ok(g) => ("igdb", g, None),
                Err(e) => (
                    "offline",
                    self.search_offline(&query),
                    Some(format!("IGDB indisponible ({e}). Résultats du catalogue hors ligne.")),
                ),
            },
            None => ("offline", self.search_offline(&query), None),
        };
        let mut games = games;
        let mut st = self.lock()?;
        let mut changed = false;
        for g in &mut games {
            let old = st.store.games.get(&g.id);
            keep_cached(old, g);
            // un jeu déjà dans la bibliothèque profite de la fiche plus complète reçue
            if old.is_some_and(|o| g.rev > o.rev) {
                st.store.games.insert(g.id, g.clone());
                changed = true;
            }
        }
        if changed {
            self.persist(&st)?;
        }
        Self::remember(&mut st, &games);
        let cards: Vec<Card> = games.iter().map(|g| card(g, st.store.library.get(g.id))).collect();
        Ok(json!({ "source": source, "results": cards, "warning": warning }))
    }

    fn search_offline(&self, query: &str) -> Vec<GameInfo> {
        let mut seen = std::collections::HashSet::new();
        autocomplete(&self.offline, query, true)
            .suggestions
            .into_iter()
            .filter(|s| seen.insert(s.data.id))
            .filter_map(|s| self.offline.get(s.data.id).map(offline_info))
            .take(SEARCH_LIMIT)
            .collect()
    }

    fn library(&self, args: &Value) -> Result<Value> {
        let want = arg_status(args, "status")?;
        let sort = args.get("sort").and_then(Value::as_str).unwrap_or("recent");
        let q = normalize(args.get("query").and_then(Value::as_str).unwrap_or(""));
        let filters = Filters::from_args(args);
        let st = self.lock()?;
        let mut all: Vec<(GameInfo, &Entry)> = st
            .store
            .library
            .live()
            .map(|e| {
                let info = st.store.games.get(&e.game_id).cloned().unwrap_or_else(|| GameInfo {
                    id: e.game_id,
                    name: format!("Jeu n°{}", e.game_id),
                    ..GameInfo::default()
                });
                (info, e)
            })
            .collect();
        // listes de choix des filtres : toute la bibliothèque, sans les filtres en cours
        let facets = json!({
            "genres": facet(all.iter().flat_map(|(g, _)| g.genres.iter().map(|x| tag_fr(x)))),
            "platforms": facet(all.iter().flat_map(|(g, e)| played_on(g, e))),
            "developers": facet(all.iter().flat_map(|(g, _)| g.developers.iter().cloned())),
        });
        let total = all.len();
        all.retain(|(g, e)| (q.is_empty() || normalize(&g.name).contains(&q)) && filters.keeps(g, e));
        let mut counts: serde_json::Map<String, Value> = STATUSES.iter().map(|s| (status_key(*s).to_string(), json!(0))).collect();
        counts.insert("all".into(), json!(all.len()));
        for (_, e) in &all {
            let k = status_key(e.status);
            let n = counts[k].as_u64().unwrap_or(0) + 1;
            counts.insert(k.into(), json!(n));
        }
        let mut rows = all;
        if let Some(w) = want {
            rows.retain(|(_, e)| e.status == w);
        }
        match sort {
            "rating" => rows.sort_by(|a, b| {
                let r = |e: &Entry| e.rating.map_or(0, Rating::half_stars);
                r(b.1).cmp(&r(a.1)).then(b.1.stamp.counter.cmp(&a.1.stamp.counter))
            }),
            "name" => rows.sort_by_key(|(g, _)| g.name.to_lowercase()),
            "year" => rows.sort_by(|a, b| b.0.year.cmp(&a.0.year).then(a.0.name.cmp(&b.0.name))),
            // plateforme jouée, de A à Z ; les jeux sans plateforme à la fin
            "platform" => rows.sort_by_cached_key(|(g, e)| {
                let p = e.platform.as_deref().unwrap_or("").to_lowercase();
                (p.is_empty(), p, g.name.to_lowercase())
            }),
            // durée de référence croissante ; les jeux sans durée à la fin, par nom
            "ttb" => rows.sort_by_cached_key(|(g, _)| {
                let d = g.time_to_beat.as_ref().and_then(|t| t.reference());
                (d.is_none(), d, g.name.to_lowercase())
            }),
            "added" => rows.sort_by(|a, b| b.1.added.cmp(&a.1.added).then(b.1.stamp.counter.cmp(&a.1.stamp.counter))),
            "hours" => rows.sort_by(|a, b| b.1.minutes.unwrap_or(0).cmp(&a.1.minutes.unwrap_or(0)).then(b.1.stamp.counter.cmp(&a.1.stamp.counter))),
            _ => rows.sort_by_key(|r| std::cmp::Reverse(r.1.stamp.counter)),
        }
        let entries: Vec<Card> = rows.iter().map(|(g, e)| card(g, Some(e))).collect();
        Ok(json!({ "entries": entries, "counts": counts, "facets": facets, "total": total }))
    }

    fn add(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let status = arg_status(args, "status")?.unwrap_or(Status::Backlog);
        let rating = arg_rating(args)?;
        let platform = args.get("platform").map(|v| clean_text(v, PLATFORM_MAX_CHARS, "nom de plateforme")).transpose()?;
        let comment = args.get("comment").map(|v| clean_text(v, COMMENT_MAX_CHARS, "commentaire")).transpose()?;
        let minutes = args.get("minutes").map(arg_raw_minutes).transpose()?;
        let (added, finished) = (arg_date(args, "added")?, arg_date(args, "finished")?);
        let mut st = self.lock()?;
        let info = self.find_info(&st, id).ok_or(Error::UnknownGame(id))?;
        let existing = st.store.library.get(id).cloned();
        let rating = rating.unwrap_or_else(|| existing.as_ref().and_then(|e| e.rating));
        let platform = platform.unwrap_or_else(|| existing.as_ref().and_then(|e| e.platform.clone()));
        let comment = comment.unwrap_or_else(|| existing.as_ref().and_then(|e| e.comment.clone()));
        let minutes = minutes.unwrap_or_else(|| existing.as_ref().and_then(|e| e.minutes));
        st.store.games.insert(id, info.clone());
        st.store.library.upsert_full(id, status, rating, platform, comment, minutes);
        // « Annuler » un retrait rétablit aussi les dates d'origine
        let finished = if status.is_ended() { finished } else { None };
        if added.is_some() || finished.is_some() {
            st.store.library.set_dates(id, added, finished)?;
        }
        self.persist(&st)?;
        Ok(serde_json::to_value(card(&info, st.store.library.get(id)))?)
    }

    fn set_status(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let status = arg_status(args, "status")?.ok_or_else(|| Error::Invalid("« status » manquant".into()))?;
        let mut st = self.lock()?;
        st.store.library.set_status(id, status)?;
        self.persist(&st)?;
        self.entry_card(&st, id)
    }

    /// `added` / `finished` : secondes Unix, `null` pour effacer, absent pour ne pas toucher.
    /// Une date de fin n'a de sens que pour un jeu terminé, joué ou abandonné.
    fn set_dates(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let (added, finished) = (arg_date(args, "added")?, arg_date(args, "finished")?);
        let mut st = self.lock()?;
        if matches!(finished, Some(Some(_))) && st.store.library.get(id).is_some_and(|e| !e.status.is_ended()) {
            return Err(Error::Invalid("la date de fin n'existe que pour un jeu terminé, joué ou abandonné".into()));
        }
        st.store.library.set_dates(id, added, finished)?;
        self.persist(&st)?;
        self.entry_card(&st, id)
    }

    fn set_rating(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let rating = arg_rating(args)?.ok_or_else(|| Error::Invalid("« rating » manquant (null pour effacer)".into()))?;
        let mut st = self.lock()?;
        st.store.library.set_rating(id, rating)?;
        self.persist(&st)?;
        self.entry_card(&st, id)
    }

    /// `platform` : nom choisi, ou `null` / texte vide pour effacer.
    fn set_platform(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let platform = args.get("platform").map_or(Ok(None), |v| clean_text(v, PLATFORM_MAX_CHARS, "nom de plateforme"))?;
        let mut st = self.lock()?;
        st.store.library.set_platform(id, platform)?;
        self.persist(&st)?;
        self.entry_card(&st, id)
    }

    /// `comment` : texte libre (10 000 caractères au maximum), ou `null` / texte vide pour effacer.
    fn set_comment(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let comment = args.get("comment").map_or(Ok(None), |v| clean_text(v, COMMENT_MAX_CHARS, "commentaire"))?;
        let mut st = self.lock()?;
        st.store.library.set_comment(id, comment)?;
        self.persist(&st)?;
        self.entry_card(&st, id)
    }

    /// Recharge la fiche d'un jeu depuis IGDB (studio, éditeur, modes…), sans toucher à la bibliothèque.
    fn refresh(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let client = self.lock()?.igdb.clone().ok_or(Error::NotConfigured)?;
        let fresh = client.by_ids(&[id])?.into_iter().find(|g| g.id == id).ok_or(Error::UnknownGame(id))?;
        let mut st = self.lock()?;
        let mut info = fresh;
        keep_cached(st.store.games.get(&id).or_else(|| st.recent.get(&id)), &mut info);
        if let Some(slot) = st.store.games.get_mut(&id) {
            *slot = info.clone();
            self.persist(&st)?;
        }
        st.recent.insert(id, info.clone());
        Ok(serde_json::to_value(card(&info, st.store.library.get(id)))?)
    }

    /// Lien HowLongToBeat d'un jeu connu (bibliothèque ou résultats récents).
    fn hltb_url_rpc(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let st = self.lock()?;
        let name = st.store.games.get(&id).or_else(|| st.recent.get(&id)).map(|g| g.name.clone()).ok_or(Error::UnknownGame(id))?;
        Ok(json!({ "url": hltb_url(&name) }))
    }

    /// Traduit le résumé en français avec DeepL ; le résultat est gardé (pas de seconde requête).
    fn translate(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let (translator, info) = {
            let st = self.lock()?;
            let tr = st.translator.clone().ok_or_else(|| Error::Invalid("ajoute ta clé DeepL dans les réglages pour traduire".into()))?;
            (tr, self.find_info(&st, id).ok_or(Error::UnknownGame(id))?)
        };
        let summary = info.summary.clone().ok_or_else(|| Error::Invalid("ce jeu n'a pas de résumé à traduire".into()))?;
        let mut info = info;
        if info.summary_fr.is_none() {
            info.summary_fr = Some(translator.to_french(&summary)?); // appel réseau hors verrou
        }
        let mut st = self.lock()?;
        if let Some(slot) = st.store.games.get_mut(&id) {
            *slot = info.clone();
            self.persist(&st)?;
        }
        st.recent.insert(id, info.clone());
        Ok(serde_json::to_value(card(&info, st.store.library.get(id)))?)
    }

    fn entry_card(&self, st: &State, id: u64) -> Result<Value> {
        let info = st.store.games.get(&id).cloned().ok_or(Error::NotInLibrary(id))?;
        Ok(serde_json::to_value(card(&info, st.store.library.get(id)))?)
    }

    /// `hours` : « 12h30 », « 12,5 », un nombre d'heures, ou `null` / vide pour effacer.
    fn set_hours(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let minutes = args.get("hours").map_or(Ok(None), arg_minutes)?;
        let mut st = self.lock()?;
        st.store.library.set_minutes(id, minutes)?;
        self.persist(&st)?;
        self.entry_card(&st, id)
    }

    fn remove(&self, args: &Value) -> Result<Value> {
        let id = arg_u64(args, "id")?;
        let mut st = self.lock()?;
        st.store.library.remove(id)?;
        self.persist(&st)?;
        Ok(json!({ "removed": id }))
    }

    fn stats(&self) -> Result<Value> {
        let st = self.lock()?;
        let h: Histogram = st.store.library.histogram();
        let heights = h.bar_heights();
        let bars: Vec<Value> = (0..10)
            .map(|i| json!({ "stars": (i + 1) as f64 / 2.0, "count": h.counts[i], "percent": (h.percent(i) * 10.0).round() / 10.0, "height": heights[i] }))
            .collect();
        let mut counts: serde_json::Map<String, Value> = STATUSES.iter().map(|s| (status_key(*s).to_string(), json!(0))).collect();
        let mut rows: Vec<(GameInfo, &Entry)> = Vec::new();
        for e in st.store.library.live() {
            let k = status_key(e.status);
            let n = counts[k].as_u64().unwrap_or(0) + 1;
            counts.insert(k.into(), json!(n));
            let info = st.store.games.get(&e.game_id).cloned().unwrap_or_else(|| GameInfo { id: e.game_id, name: format!("Jeu n°{}", e.game_id), ..GameInfo::default() });
            rows.push((info, e));
        }
        let count_of = |k: &str| counts[k].as_u64().unwrap_or(0);
        let started = count_of("playing") + count_of("completed") + count_of("played") + count_of("abandoned");
        let completed = count_of("completed");

        // heures
        let timed: Vec<&(GameInfo, &Entry)> = rows.iter().filter(|(_, e)| e.minutes.is_some()).collect();
        let total_minutes: u64 = timed.iter().map(|(_, e)| u64::from(e.minutes.unwrap_or(0))).sum();
        let mut by_time: Vec<&(GameInfo, &Entry)> = timed.clone();
        by_time.sort_by(|a, b| b.1.minutes.cmp(&a.1.minutes).then_with(|| a.0.name.cmp(&b.0.name)));
        let longest: Vec<Card> = by_time.iter().take(5).map(|(g, e)| card(g, Some(e))).collect();
        let finished_times: Vec<u32> = rows.iter().filter(|(_, e)| e.status == Status::Completed).filter_map(|(_, e)| e.minutes).collect();
        let avg_finish = (!finished_times.is_empty()).then(|| finished_times.iter().map(|m| u64::from(*m)).sum::<u64>() / finished_times.len() as u64);

        // ancienneté du backlog et rythme de fin (seuls les jeux datés comptent)
        let now = store::now_secs();
        let days_since = |t: i64| (now - t).max(0) / 86_400;
        let mut waiting: Vec<&(GameInfo, &Entry)> = rows.iter().filter(|(_, e)| e.status == Status::Backlog && e.added.is_some()).collect();
        waiting.sort_by_key(|(g, e)| (e.added, g.name.clone()));
        let waiting_days: Vec<i64> = waiting.iter().filter_map(|(_, e)| e.added).map(days_since).collect();
        let oldest: Vec<Card> = waiting.iter().take(5).map(|(g, e)| card(g, Some(e))).collect();
        let this_year = crate::igdb::year_from_unix(now);
        let mut by_year: std::collections::BTreeMap<i32, u64> = std::collections::BTreeMap::new();
        for (_, e) in rows.iter().filter(|(_, e)| e.status == Status::Completed) {
            if let Some(f) = e.finished {
                *by_year.entry(crate::igdb::year_from_unix(f)).or_insert(0) += 1;
            }
        }
        let to_finish: Vec<i64> = rows
            .iter()
            .filter(|(_, e)| e.status == Status::Completed)
            .filter_map(|(_, e)| Some(e.finished? - e.added?))
            .filter(|d| *d >= 0)
            .map(|d| d / 86_400)
            .collect();
        let avg = |v: &[i64]| (!v.is_empty()).then(|| v.iter().sum::<i64>() / v.len() as i64);

        // notes
        let mut rated: Vec<&(GameInfo, &Entry)> = rows.iter().filter(|(_, e)| e.rating.is_some()).collect();
        rated.sort_by(|a, b| {
            let r = |e: &Entry| e.rating.map_or(0, Rating::half_stars);
            r(b.1).cmp(&r(a.1)).then(b.1.stamp.counter.cmp(&a.1.stamp.counter))
        });
        let top_rated: Vec<Card> = rated.iter().take(5).map(|(g, e)| card(g, Some(e))).collect();
        let mut halves: Vec<u8> = rated.iter().filter_map(|(_, e)| e.rating.map(Rating::half_stars)).collect();
        halves.sort_unstable();
        let median = (!halves.is_empty()).then(|| {
            let n = halves.len();
            let m = if n % 2 == 1 { f64::from(halves[n / 2]) } else { (f64::from(halves[n / 2 - 1]) + f64::from(halves[n / 2])) / 2.0 };
            m / 2.0
        });
        let loved = halves.iter().filter(|h| **h >= 9).count();

        // regroupements
        let by_genre = groups(&rows, |g, _| g.genres.iter().map(|x| tag_fr(x)).collect());
        let by_platform = groups(&rows, played_on);
        let by_developer = groups(&rows, |g, _| g.developers.clone());
        let by_publisher = groups(&rows, |g, _| g.publishers.clone());
        let mut decades: std::collections::BTreeMap<i32, u64> = std::collections::BTreeMap::new();
        for (g, _) in &rows {
            if let Some(y) = g.year {
                *decades.entry(y.div_euclid(10) * 10).or_insert(0) += 1;
            }
        }
        let decades: Vec<Value> = decades.into_iter().map(|(d, n)| json!({ "name": format!("{d}s"), "count": n })).collect();
        let (dated, detailed) = (rows.iter().filter(|(g, _)| g.year.is_some()).count(), rows.iter().filter(|(g, _)| g.rev >= 2).count());

        Ok(json!({
            "total": rows.len(),
            "rated": h.total(),
            "average": h.average(),
            "median": median,
            "loved": loved,
            "counts": counts,
            "bars": bars,
            "started": started,
            "finish_rate": (started > 0).then(|| (completed as f64 / started as f64 * 1000.0).round() / 10.0),
            "backlog_pending": count_of("backlog") + count_of("wishlist"),
            "commented": rows.iter().filter(|(_, e)| e.comment.is_some()).count(),
            "hours": {
                "total_minutes": total_minutes,
                "total_text": duration_fr(total_minutes.min(u64::from(u32::MAX)) as u32),
                "games_timed": timed.len(),
                "average_finish_text": avg_finish.map(|m| duration_fr(m as u32)),
                "longest": longest,
            },
            "top_rated": top_rated,
            "timeline": {
                "backlog_total": count_of("backlog"),
                "backlog_dated": waiting.len(),
                "backlog_average_days": avg(&waiting_days),
                "oldest": oldest,
                "year": this_year,
                "finished_this_year": by_year.get(&this_year).copied().unwrap_or(0),
                "finished_by_year": by_year.iter().map(|(y, n)| json!({ "name": y.to_string(), "count": n })).collect::<Vec<_>>(),
                "average_days_to_finish": avg(&to_finish),
                "to_finish_sample": to_finish.len(),
            },
            "genres": by_genre,
            "platforms": by_platform,
            "developers": by_developer,
            "publishers": by_publisher,
            "decades": decades,
            "coverage": { "dated": dated, "detailed": detailed },
        }))
    }

    fn get_settings(&self) -> Result<Value> {
        let st = self.lock()?;
        let (eff, from_env) = st.config.effective();
        Ok(json!({
            "igdb_client_id": eff.igdb_client_id,
            "has_secret": !eff.igdb_client_secret.is_empty(),
            "configured": eff.is_complete(),
            "from_env": from_env,
            "has_deepl": !eff.deepl_key.trim().is_empty(),
            "secret_storage": if st.storage == Storage::Os { "os" } else { "file" },
            "secret_storage_name": self.vault.store().map(|v| v.label()),
            "data_dir": self.dir.display().to_string(),
        }))
    }

    fn save_settings(&self, args: &Value) -> Result<Value> {
        {
            let mut st = self.lock()?;
            let text = |k: &str| args.get(k).and_then(Value::as_str).map(str::trim);
            if args.get("clear").and_then(Value::as_bool) == Some(true) {
                st.config.igdb_client_id.clear();
                st.config.igdb_client_secret.clear();
            } else {
                if let Some(id) = text("igdb_client_id") {
                    st.config.igdb_client_id = id.to_string();
                }
                if let Some(secret) = text("igdb_client_secret").filter(|s| !s.is_empty()) {
                    st.config.igdb_client_secret = secret.to_string();
                }
            }
            if args.get("clear_deepl").and_then(Value::as_bool) == Some(true) {
                st.config.deepl_key.clear();
            } else if let Some(key) = text("deepl_key").filter(|s| !s.is_empty()) {
                st.config.deepl_key = key.to_string();
            }
            st.storage = st.config.save_with(&self.dir, &self.vault)?;
            self.rebuild(&mut st);
        }
        self.get_settings()
    }

    fn test_igdb(&self) -> Result<Value> {
        let client = self.lock()?.igdb.clone().ok_or(Error::NotConfigured)?;
        client.check()?;
        Ok(json!({ "ok": true }))
    }

    /// Complète les fiches de la bibliothèque (jaquettes, studios, éditeurs…) quand IGDB est configuré.
    fn sync_metadata(&self) -> Result<Value> {
        let (client, ids) = {
            let st = self.lock()?;
            let ids: Vec<u64> = st
                .store
                .library
                .live()
                .map(|e| e.game_id)
                .filter(|id| st.store.games.get(id).is_none_or(|g| g.rev < REV_FULL))
                .collect();
            (st.igdb.clone(), ids)
        };
        let Some(client) = client else {
            return Ok(json!({ "updated": 0, "pending": ids.len(), "mode": "offline" }));
        };
        let (mut updated, mut done) = (0, 0);
        for chunk in ids.chunks(100) {
            if done > 0 && !cfg!(test) {
                // ne pas enchaîner les requêtes : IGDB limite le débit
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            done += 1;
            let games = client.by_ids(chunk)?;
            let mut st = self.lock()?;
            for mut g in games {
                keep_cached(st.store.games.get(&g.id), &mut g);
                if g.rev >= REV_FULL {
                    updated += 1;
                }
                st.store.games.insert(g.id, g);
            }
            self.persist(&st)?;
        }
        let pending = {
            let st = self.lock()?;
            ids.iter().filter(|id| st.store.games.get(id).is_none_or(|g| g.rev < REV_FULL)).count()
        };
        Ok(json!({ "updated": updated, "pending": pending, "mode": "igdb" }))
    }


    // ------------------------------------------------ import d'une liste CSV

    /// Lit le fichier et le garde en attente. Aucun appel réseau, rien n'est ajouté.
    fn csv_start(&self, args: &Value) -> Result<Value> {
        let text = args.get("content").and_then(Value::as_str).unwrap_or("");
        if text.len() > transfer::MAX_IMPORT_BYTES {
            return Err(Error::Invalid("fichier trop volumineux".into()));
        }
        let parsed = csv_import::parse(text, store::now_secs())?;
        let mut st = self.lock()?;
        let out = json!({
            "rows": parsed.rows.len(),
            "blank": parsed.blank,
            "delimiter": parsed.delimiter.to_string(),
            "columns": parsed.columns,
            "with_ids": parsed.rows.iter().filter(|r| r.igdb_id.is_some()).count(),
            "with_issues": parsed.rows.iter().filter(|r| !r.issues.is_empty()).count(),
            "mode": if st.igdb.is_some() { "igdb" } else { "offline" },
        });
        st.staged = Some(Staged { matches: (0..parsed.rows.len()).map(|_| None).collect(), rows: parsed.rows });
        Ok(out)
    }

    /// Rapproche `limit` lignes (10 au plus) à partir de `offset`. À rappeler jusqu'à `done == total` ;
    /// une erreur réseau peut être suivie du même appel (le travail déjà fait est gardé).
    fn csv_match(&self, args: &Value) -> Result<Value> {
        let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let limit = (args.get("limit").and_then(Value::as_u64).unwrap_or(5) as usize).clamp(1, 10);
        let (rows, igdb, total) = {
            let st = self.lock()?;
            let staged = st.staged.as_ref().ok_or_else(|| Error::Invalid("aucun import en cours : choisis d'abord un fichier".into()))?;
            let end = (offset + limit).min(staged.rows.len());
            let rows: Vec<(usize, CsvRow)> = (offset.min(end)..end).filter(|i| staged.matches[*i].is_none()).map(|i| (i, staged.rows[i].clone())).collect();
            (rows, st.igdb.clone(), staged.rows.len())
        };
        // 1. identifiants IGDB donnés dans le fichier : une seule requête pour tout le lot
        let wanted: Vec<u64> = rows.iter().filter_map(|(_, r)| r.igdb_id).collect();
        let by_id: HashMap<u64, GameInfo> = match (&igdb, wanted.is_empty()) {
            (Some(client), false) => client.by_ids(&wanted)?.into_iter().map(|g| (g.id, g)).collect(),
            (None, false) => {
                let st = self.lock()?;
                wanted
                    .iter()
                    .filter_map(|id| self.offline.get(*id).map(offline_info).or_else(|| st.store.games.get(id).cloned()))
                    .map(|g| (g.id, g))
                    .collect()
            }
            _ => HashMap::new(),
        };
        let mut done = Vec::new();
        for (i, row) in &rows {
            if let Some(info) = row.igdb_id.and_then(|id| by_id.get(&id)) {
                let kind = if self.lock()?.store.library.get(info.id).is_some() { MatchKind::Present } else { MatchKind::Sure };
                done.push((*i, RowMatch { kind, candidates: vec![info.clone()] }));
                continue;
            }
            if let Some(present) = self.csv_find_in_library(row)? {
                done.push((*i, RowMatch { kind: MatchKind::Present, candidates: vec![present] }));
                continue;
            }
            let found = match &igdb {
                Some(client) => {
                    if !self.match_pause.is_zero() {
                        std::thread::sleep(self.match_pause);
                    }
                    client.search(&row.title, 8)?
                }
                None => self.search_offline(&row.title),
            };
            done.push((*i, self.csv_classify(row, found)?));
        }
        let mut st = self.lock()?;
        let staged = st.staged.as_mut().ok_or_else(|| Error::Invalid("import annulé".into()))?;
        for (i, m) in done {
            if let Some(slot) = staged.matches.get_mut(i) {
                *slot = Some(m);
            }
        }
        let matched = staged.matches.iter().filter(|m| m.is_some()).count();
        Ok(json!({ "done": matched, "total": total }))
    }

    /// Un jeu de la bibliothèque dont le nom est exactement ce titre (et la même année si les deux la donnent).
    fn csv_find_in_library(&self, row: &CsvRow) -> Result<Option<GameInfo>> {
        let st = self.lock()?;
        let key = fold(&row.title);
        let found = st
            .store
            .library
            .live()
            .filter_map(|e| st.store.games.get(&e.game_id))
            .find(|g| fold(&g.name) == key && row.year.zip(g.year).is_none_or(|(a, b)| a == b))
            .cloned();
        Ok(found)
    }

    /// Choisit la certitude et trie les candidats : titre identique d'abord, puis année identique.
    fn csv_classify(&self, row: &CsvRow, mut found: Vec<GameInfo>) -> Result<RowMatch> {
        if found.is_empty() {
            return Ok(RowMatch { kind: MatchKind::NotFound, candidates: vec![] });
        }
        let key = fold(&row.title);
        let same_year = |g: &GameInfo| row.year.is_some() && g.year == row.year;
        found.sort_by_key(|g| (fold(&g.name) != key, !same_year(g))); // tri stable : l'ordre IGDB reste dans chaque groupe
        found.truncate(5);
        let exact: Vec<&GameInfo> = found.iter().filter(|g| fold(&g.name) == key).collect();
        let sure = match (exact.len(), row.year) {
            (1, None) => true,
            (_, Some(y)) => exact.iter().filter(|g| g.year == Some(y)).count() == 1,
            _ => false,
        };
        let present = found.first().is_some_and(|g| self.lock().is_ok_and(|st| st.store.library.get(g.id).is_some()));
        let kind = match (sure, present) {
            (true, true) => MatchKind::Present,
            (true, false) => MatchKind::Sure,
            _ => MatchKind::Check,
        };
        Ok(RowMatch { kind, candidates: found })
    }

    /// Tout ce qu'il faut pour l'écran de vérification : une entrée par ligne du fichier.
    fn csv_review(&self) -> Result<Value> {
        let st = self.lock()?;
        let staged = st.staged.as_ref().ok_or_else(|| Error::Invalid("aucun import en cours".into()))?;
        let mut counts = json!({ "sure": 0, "check": 0, "notfound": 0, "present": 0, "pending": 0 });
        let items: Vec<Value> = staged
            .rows
            .iter()
            .zip(&staged.matches)
            .enumerate()
            .map(|(i, (r, m))| {
                let kind = m.as_ref().map_or("pending", |m| match m.kind {
                    MatchKind::Sure => "sure",
                    MatchKind::Check => "check",
                    MatchKind::NotFound => "notfound",
                    MatchKind::Present => "present",
                });
                counts[kind] = json!(counts[kind].as_u64().unwrap_or(0) + 1);
                let candidates: Vec<Value> = m
                    .as_ref()
                    .map(|m| {
                        m.candidates
                            .iter()
                            .map(|g| json!({ "id": g.id, "name": g.name, "year": g.year, "cover_url": g.cover_url("cover_small"), "platforms": g.platforms.iter().take(4).collect::<Vec<_>>() }))
                            .collect()
                    })
                    .unwrap_or_default();
                json!({
                    "index": i, "line": r.line, "title": r.title, "year": r.year, "status": r.status,
                    "stars": r.rating_half.map(|h| f64::from(h) / 2.0), "platform": r.platform,
                    "hours_text": r.minutes.map(duration_fr), "issues": r.issues, "kind": kind, "candidates": candidates,
                })
            })
            .collect();
        Ok(json!({ "items": items, "counts": counts, "total": staged.rows.len() }))
    }

    /// Ajoute les jeux choisis : `choices` = [{ index, id }]. Un `id` doit être l'un des candidats proposés pour
    /// cette ligne. Les jeux déjà présents ne sont jamais modifiés. Une sauvegarde est faite d'abord.
    fn csv_apply(&self, args: &Value) -> Result<Value> {
        let choices = args.get("choices").and_then(Value::as_array).ok_or_else(|| Error::Invalid("« choices » manquant".into()))?;
        let mut st = self.lock()?;
        let staged = st.staged.take().ok_or_else(|| Error::Invalid("aucun import en cours".into()))?;
        let mut picked: Vec<(usize, GameInfo)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let (mut present, mut duplicates) = (0usize, 0usize);
        for c in choices {
            let (Some(i), Some(id)) = (c.get("index").and_then(Value::as_u64), c.get("id").and_then(Value::as_u64)) else { continue };
            let Some(info) = staged.matches.get(i as usize).and_then(Option::as_ref).and_then(|m| m.candidates.iter().find(|g| g.id == id)) else {
                st.staged = Some(staged);
                return Err(Error::Invalid("choix invalide : ce jeu n'était pas proposé pour cette ligne".into()));
            };
            if st.store.library.get(id).is_some() {
                present += 1;
            } else if !seen.insert(id) {
                duplicates += 1;
            } else {
                picked.push((i as usize, info.clone()));
            }
        }
        if picked.is_empty() {
            return Ok(json!({ "added": 0, "present": present, "duplicates": duplicates }));
        }
        if has_data(&st.store) {
            if let Err(e) = st.store.backup(&self.dir, "avant-import") {
                st.staged = Some(staged); // rien n'a été touché : on peut réessayer
                return Err(e);
            }
        }
        for (i, info) in &picked {
            let row = &staged.rows[*i];
            match st.store.games.get(&info.id) {
                Some(old) if old.rev >= info.rev => {}
                _ => {
                    st.store.games.insert(info.id, info.clone());
                }
            }
            let rating = row.rating_half.and_then(|h| Rating::from_half_stars(h).ok());
            st.store.library.upsert_full(info.id, row.status, rating, row.platform.clone(), row.comment.clone(), row.minutes);
            // la date d'ajout n'est connue que si le fichier la donne : sinon on ne prétend pas que c'est aujourd'hui
            st.store.library.set_dates(info.id, Some(row.added), Some(row.finished))?;
        }
        st.recent.clear();
        self.persist(&st)?;
        Ok(json!({ "added": picked.len(), "present": present, "duplicates": duplicates }))
    }

    // ------------------------------------------------ export, import, sauvegardes

    fn backups(&self) -> Result<Value> {
        Ok(json!({
            "items": store::list_backups(&self.dir),
            "keep": store::BACKUP_KEEP,
            "folder": store::backups_dir(&self.dir).display().to_string(),
            "export_folder": self.export_dir.display().to_string(),
        }))
    }

    fn backup_now(&self) -> Result<Value> {
        let st = self.lock()?;
        if !has_data(&st.store) {
            return Err(Error::Invalid("la bibliothèque est vide : rien à sauvegarder".into()));
        }
        st.store.backup(&self.dir, "manuelle")?;
        drop(st);
        self.backups()
    }

    fn restore_backup(&self, args: &Value) -> Result<Value> {
        let file = args.get("file").and_then(Value::as_str).unwrap_or("");
        let restored = Store::load_backup(&self.dir, file).map_err(|e| match e {
            Error::Invalid(_) => e,
            _ => Error::Invalid("cette sauvegarde est illisible ou a été supprimée".into()),
        })?;
        let mut st = self.lock()?;
        if has_data(&st.store) {
            st.store.backup(&self.dir, "avant-restauration")?;
        }
        st.store = restored;
        st.recent.clear();
        self.persist(&st)?;
        let games = st.store.library.live().count();
        drop(st);
        Ok(json!({ "games": games, "backups": self.backups()? }))
    }

    /// Écrit un export dans le dossier de destination (jamais en écrasant un fichier existant).
    fn export_file(&self, args: &Value) -> Result<Value> {
        let format = args.get("format").and_then(Value::as_str).unwrap_or("json");
        let (content, ext, games) = {
            let st = self.lock()?;
            let games = st.store.library.live().count();
            match format {
                "json" => (serde_json::to_string_pretty(&st.store)?, "json", games),
                "csv" => (transfer::to_csv(&st.store), "csv", games),
                other => return Err(Error::Invalid(format!("format d'export inconnu : {other}"))),
            }
        };
        if games == 0 {
            return Err(Error::Invalid("la bibliothèque est vide : rien à exporter".into()));
        }
        std::fs::create_dir_all(&self.export_dir)?;
        let path = transfer::unique_path(&self.export_dir, &transfer::export_stem(store::now_secs()), ext);
        std::fs::write(&path, content)?;
        Ok(json!({
            "format": ext,
            "games": games,
            "file": path.file_name().map(|n| n.to_string_lossy().into_owned()),
            "path": path.display().to_string(),
            "folder": self.export_dir.display().to_string(),
        }))
    }

    fn import_preview(&self, args: &Value) -> Result<Value> {
        let text = args.get("content").and_then(Value::as_str).unwrap_or("");
        let (incoming, skipped) = transfer::parse_import(text)?;
        let st = self.lock()?;
        Ok(serde_json::to_value(transfer::plan_import(&st.store, &incoming, skipped))?)
    }

    fn import_apply(&self, args: &Value) -> Result<Value> {
        let text = args.get("content").and_then(Value::as_str).unwrap_or("");
        let mode = ImportMode::parse(args.get("mode").and_then(Value::as_str).unwrap_or(""))?;
        let (incoming, skipped) = transfer::parse_import(text)?;
        let mut st = self.lock()?;
        let plan = transfer::plan_import(&st.store, &incoming, skipped);
        if has_data(&st.store) {
            // Sans sauvegarde d'avant-import, on n'importe pas.
            st.store.backup(&self.dir, "avant-import")?;
        }
        transfer::apply_import(&mut st.store, &incoming, mode);
        st.recent.clear();
        self.persist(&st)?;
        let effect = match mode {
            ImportMode::Add => plan.add,
            ImportMode::Merge => plan.merge,
            ImportMode::Replace => plan.replace,
        };
        Ok(json!({ "effect": effect, "games": st.store.library.live().count() }))
    }

    /// Ouvre le dossier des exports ou des sauvegardes dans l'explorateur de fichiers.
    fn reveal(&self, args: &Value) -> Result<Value> {
        let dir = match args.get("target").and_then(Value::as_str).unwrap_or("") {
            "exports" => &self.export_dir,
            "data" => &self.dir,
            "backups" => &store::backups_dir(&self.dir),
            other => return Err(Error::Invalid(format!("dossier inconnu : {other}"))),
        };
        std::fs::create_dir_all(dir)?;
        // Un export précis peut être sélectionné ; seul un nom de fichier nu, présent dans le dossier, est accepté.
        let file = args.get("file").and_then(Value::as_str).filter(|f| std::path::Path::new(f).file_name().is_some_and(|n| n == *f));
        match file.map(|f| dir.join(f)).filter(|p| p.is_file()) {
            Some(path) => transfer::reveal(&path)?,
            None => transfer::reveal(dir)?,
        }
        Ok(json!({ "ok": true }))
    }
}

fn has_data(store: &Store) -> bool {
    !store.library.entries.is_empty()
}

fn recovery_notice(r: &Recovery) -> String {
    const KEPT: &str = "Le fichier abîmé est gardé dans le dossier de données (nom contenant « corrompu »).";
    match r {
        Recovery::FromBak => format!("Ton fichier de bibliothèque était illisible : la version précédente a été restaurée automatiquement. {KEPT}"),
        Recovery::FromBackup(f) => {
            let when = store::parse_backup_name(f).map(|(ts, _)| date_fr(ts)).unwrap_or_default();
            format!("Ton fichier de bibliothèque était illisible : la sauvegarde du {when} a été restaurée automatiquement. Les changements faits depuis ne sont pas récupérables. {KEPT}")
        }
        Recovery::Nothing => format!("Ton fichier de bibliothèque était illisible et aucune sauvegarde n’était exploitable : une bibliothèque vide a été créée. {KEPT}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::igdb::fake::*;
    use crate::store::temp_dir;
    use crate::library::Status;

    fn offline(tag: &str) -> (Service, PathBuf) {
        let dir = temp_dir(tag);
        (Service::open(&dir, FakeTransport::with(vec![])).unwrap(), dir)
    }

    fn ok(v: Value) -> Value {
        assert_eq!(v["ok"], true, "{v}");
        v["result"].clone()
    }

    #[test]
    fn jeu_de_demonstration() {
        let (s, dir) = offline("demo");
        let n = s.seed_demo().unwrap();
        assert!(n >= 16, "{n}");
        let lib = ok(s.call("library", &json!({})));
        assert_eq!(lib["counts"]["all"], n);
        assert!(lib["counts"]["playing"].as_u64().unwrap() >= 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recherche_hors_ligne_et_requete_trop_courte() {
        let (s, dir) = offline("off");
        let r = ok(s.call("search", &json!({"query": "zleda"})));
        assert_eq!(r["source"], "offline");
        assert_eq!(r["results"].as_array().unwrap().len(), 21);
        assert!(r["results"][0]["cover_url"].is_null());
        assert!(ok(s.call("search", &json!({"query": "z"})))["results"].as_array().unwrap().is_empty());
        assert!(ok(s.call("search", &json!({})))["results"].as_array().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parcours_complet_et_persistance() {
        let (s, dir) = offline("flow");
        // 7346 = « Breath of the Wild » dans le catalogue hors ligne ? sinon on prend un id connu
        let hades = ok(s.call("search", &json!({"query": "hades"})))["results"][0].clone();
        let id = hades["id"].as_u64().unwrap();
        let added = ok(s.call("add", &json!({"id": id, "status": "playing"})));
        assert_eq!(added["entry"]["status"], "playing");
        assert!(added["entry"]["stars"].is_null());
        assert_eq!(ok(s.call("set_rating", &json!({"id": id, "rating": 4.5})))["entry"]["half"], 9);
        assert_eq!(ok(s.call("set_status", &json!({"id": id, "status": "completed"})))["entry"]["stars"], 4.5);
        let st = ok(s.call("stats", &json!({})));
        assert_eq!(st["total"], 1);
        assert_eq!(st["average"], 4.5);
        assert_eq!(st["counts"]["completed"], 1);
        assert_eq!(st["bars"][8]["count"], 1);
        // une recherche indique que le jeu est déjà dans la bibliothèque
        let again = ok(s.call("search", &json!({"query": "hades"})));
        assert_eq!(again["results"][0]["entry"]["status"], "completed");
        // les données survivent à un redémarrage
        let s2 = Service::open(&dir, FakeTransport::with(vec![])).unwrap();
        let lib = ok(s2.call("library", &json!({})));
        assert_eq!(lib["entries"].as_array().unwrap().len(), 1);
        assert_eq!(lib["entries"][0]["entry"]["stars"], 4.5);
        // note effacée puis suppression
        assert!(ok(s2.call("set_rating", &json!({"id": id, "rating": null})))["entry"]["stars"].is_null());
        ok(s2.call("remove", &json!({"id": id})));
        assert!(ok(s2.call("library", &json!({})))["entries"].as_array().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn erreurs_d_arguments() {
        let (s, dir) = offline("err");
        let id = ok(s.call("search", &json!({"query": "hades"})))["results"][0]["id"].as_u64().unwrap();
        for (m, a) in [
            ("add", json!({"id": 999_999_999})),
            ("add", json!({"id": id, "status": "nimporte"})),
            ("add", json!({"id": id, "rating": 4.7})),
            ("add", json!({})),
            ("set_rating", json!({"id": id, "rating": 3})),
            ("set_status", json!({"id": id})),
            ("remove", json!({"id": id})),
            ("inconnue", json!({})),
        ] {
            assert_eq!(s.call(m, &a)["ok"], false, "{m} {a}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn filtre_tri_et_compteurs_de_la_bibliotheque() {
        let (s, dir) = offline("lib");
        let ids: Vec<u64> = ok(s.call("search", &json!({"query": "zelda"})))["results"]
            .as_array()
            .unwrap()
            .iter()
            .take(4)
            .map(|g| g["id"].as_u64().unwrap())
            .collect();
        ok(s.call("add", &json!({"id": ids[0], "status": "playing", "rating": 2.0})));
        ok(s.call("add", &json!({"id": ids[1], "status": "backlog", "rating": 5.0})));
        ok(s.call("add", &json!({"id": ids[2], "status": "backlog"})));
        ok(s.call("add", &json!({"id": ids[3], "status": "wishlist"})));
        let all = ok(s.call("library", &json!({})));
        assert_eq!(all["counts"]["all"], 4);
        assert_eq!(all["counts"]["backlog"], 2);
        assert_eq!(all["entries"][0]["id"], ids[3], "le plus récent d'abord");
        let by_rating = ok(s.call("library", &json!({"sort": "rating"})));
        assert_eq!(by_rating["entries"][0]["id"], ids[1]);
        let backlog = ok(s.call("library", &json!({"status": "backlog"})));
        assert_eq!(backlog["entries"].as_array().unwrap().len(), 2);
        assert_eq!(backlog["counts"]["all"], 4, "les compteurs ne dépendent pas du filtre de statut");
        assert!(ok(s.call("library", &json!({"query": "xyzxyz"})))["entries"].as_array().unwrap().is_empty());
        assert_eq!(s.call("library", &json!({"status": "bof"}))["ok"], false);
        let by_name = ok(s.call("library", &json!({"sort": "name"})));
        let names: Vec<String> = by_name["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_lowercase()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recherche_igdb_ajout_et_lecture_hors_ligne_ensuite() {
        let dir = temp_dir("igdb");
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(GAMES_SAMPLE.into())]);
        let s = Service::open(&dir, t.clone()).unwrap();
        assert_eq!(ok(s.call("status", &json!({})))["mode"], "offline");
        let cfg = ok(s.call("save_settings", &json!({"igdb_client_id": " id ", "igdb_client_secret": "sec"})));
        assert_eq!(cfg["configured"], true);
        assert!(cfg.get("igdb_client_secret").is_none(), "le secret ne doit jamais être renvoyé");
        assert!(!cfg.to_string().contains("sec\""));
        let r = ok(s.call("search", &json!({"query": "zelda"})));
        assert_eq!(r["source"], "igdb");
        assert_eq!(r["results"][0]["cover_url"], "https://images.igdb.com/igdb/image/upload/t_cover_big/co3p2d.jpg");
        assert_eq!(r["results"][0]["cover_url_2x"], "https://images.igdb.com/igdb/image/upload/t_cover_big_2x/co3p2d.jpg");
        assert_eq!(r["results"][0]["year"], 2017);
        ok(s.call("add", &json!({"id": 7346, "status": "backlog"})));
        // redémarrage sans clés ni réseau : la fiche (jaquette comprise) est dans le fichier local
        let _ = std::fs::remove_file(crate::config::Config::path(&dir));
        let s2 = Service::open(&dir, FakeTransport::with(vec![])).unwrap();
        let lib = ok(s2.call("library", &json!({})));
        assert_eq!(lib["entries"][0]["name"], "The Legend of Zelda: Breath of the Wild");
        assert!(lib["entries"][0]["cover_url"].as_str().unwrap().contains("co3p2d"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repli_hors_ligne_avec_avertissement_si_igdb_echoue() {
        let dir = temp_dir("fallback");
        let t = FakeTransport::with(vec![Err(Error::Network("délai dépassé".into()))]);
        let s = Service::open(&dir, t).unwrap();
        s.call("save_settings", &json!({"igdb_client_id": "a", "igdb_client_secret": "b"}));
        let r = ok(s.call("search", &json!({"query": "hades"})));
        assert_eq!(r["source"], "offline");
        assert!(r["warning"].as_str().unwrap().contains("délai dépassé"));
        assert!(!r["results"].as_array().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reglages_effacement_et_test_de_connexion() {
        let dir = temp_dir("settings");
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok("[]".into())]);
        let s = Service::open(&dir, t).unwrap();
        assert_eq!(s.call("test_igdb", &json!({}))["ok"], false, "sans clés : erreur claire");
        s.call("save_settings", &json!({"igdb_client_id": "a", "igdb_client_secret": "b"}));
        assert_eq!(ok(s.call("test_igdb", &json!({})))["ok"], true);
        // un identifiant seul ne remplace pas le secret
        let c = ok(s.call("save_settings", &json!({"igdb_client_id": "autre"})));
        assert_eq!((c["igdb_client_id"].as_str(), c["has_secret"].as_bool()), (Some("autre"), Some(true)));
        let c = ok(s.call("save_settings", &json!({"clear": true})));
        assert_eq!((c["configured"].as_bool(), c["has_secret"].as_bool()), (Some(false), Some(false)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn completion_des_fiches_manquantes() {
        let dir = temp_dir("sync");
        let hades_with_cover = r#"[{"id":113112,"name":"Hades","slug":"hades--1","cover":{"image_id":"coabc"},"first_release_date":1600300800}]"#;
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(hades_with_cover.into()), Ok("[]".into())]);
        let s = Service::open(&dir, t.clone()).unwrap();
        // ajouté hors ligne : pas de jaquette
        ok(s.call("add", &json!({"id": 113112, "status": "backlog"})));
        assert_eq!(ok(s.call("sync_metadata", &json!({})))["mode"], "offline");
        s.call("save_settings", &json!({"igdb_client_id": "a", "igdb_client_secret": "b"}));
        let r = ok(s.call("sync_metadata", &json!({})));
        assert_eq!((r["updated"].as_u64(), r["pending"].as_u64()), (Some(1), Some(0)));
        let lib = ok(s.call("library", &json!({})));
        assert!(lib["entries"][0]["cover_url"].as_str().unwrap().contains("coabc"));
        assert!(t.calls()[1].2.contains("where id = (113112);"), "requête des fiches (la dernière demande les durées)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    const DETAILED: &str = r#"[
      {"id":1,"name":"Hades","slug":"hades--1","first_release_date":1600300800,"summary":"Defy the god of the dead.","total_rating":90,
       "cover":{"image_id":"co1"},"platforms":[{"name":"PC (Microsoft Windows)","abbreviation":"PC"},{"name":"Nintendo Switch","abbreviation":"Switch"}],
       "genres":[{"name":"Role-playing (RPG)"},{"name":"Indie"}],"themes":[{"name":"Fantasy"}],"game_modes":[{"name":"Single player"}],
       "player_perspectives":[{"name":"Isometric"}],"franchises":[{"name":"Hades"}],
       "involved_companies":[{"developer":true,"publisher":true,"company":{"name":"Supergiant Games"}}]},
      {"id":2,"name":"Celeste","slug":"celeste","first_release_date":1516924800,
       "platforms":[{"abbreviation":"PC"},{"abbreviation":"Switch"}],"genres":[{"name":"Platform"},{"name":"Indie"}],
       "involved_companies":[{"developer":true,"publisher":true,"company":{"name":"Maddy Makes Games"}}]},
      {"id":3,"name":"Doom Eternal","slug":"doom-eternal","first_release_date":1584662400,
       "platforms":[{"abbreviation":"PC"},{"abbreviation":"PS4"}],"genres":[{"name":"Shooter"}],
       "involved_companies":[{"developer":true,"publisher":false,"company":{"name":"id Software"}},{"developer":false,"publisher":true,"company":{"name":"Bethesda Softworks"}}]}
    ]"#;

    fn with_igdb(tag: &str, responses: Vec<Result<String>>) -> (Service, PathBuf, Arc<FakeTransport>) {
        let dir = temp_dir(tag);
        let t = FakeTransport::with(responses);
        let s = Service::open(&dir, t.clone()).unwrap();
        s.call("save_settings", &json!({"igdb_client_id": "a", "igdb_client_secret": "b"}));
        (s, dir, t)
    }

    #[test]
    fn cles_dans_le_coffre_du_systeme() {
        use crate::secrets::MemoryStore;
        let dir = temp_dir("vault");
        let store = MemoryStore::new();
        let open = || Service::open_with(&dir, FakeTransport::with(vec![]), Vault::with(store.clone())).unwrap();
        let s = open();
        let c = ok(s.call("save_settings", &json!({"igdb_client_id": "mon-id", "igdb_client_secret": "SECRET-IGDB", "deepl_key": "CLE-DEEPL:fx"})));
        assert_eq!((c["secret_storage"].as_str(), c["configured"].as_bool(), c["has_deepl"].as_bool()), (Some("os"), Some(true), Some(true)));
        assert_eq!(c["secret_storage_name"], "le coffre de test");
        let file = std::fs::read_to_string(crate::config::Config::path(&dir)).unwrap();
        assert!(file.contains("mon-id") && !file.contains("SECRET-IGDB") && !file.contains("CLE-DEEPL"), "le fichier ne contient aucun secret : {file}");
        // les réponses de l'interface ne renvoient jamais les secrets
        for method in ["get_settings", "status"] {
            let out = s.call(method, &json!({})).to_string();
            assert!(!out.contains("SECRET-IGDB") && !out.contains("CLE-DEEPL"), "{method} : {out}");
        }
        // redémarrage : tout est relu depuis le coffre
        let c = ok(open().call("get_settings", &json!({})));
        assert_eq!((c["configured"].as_bool(), c["has_deepl"].as_bool(), c["secret_storage"].as_str()), (Some(true), Some(true), Some("os")));
        // effacement : le coffre est vidé aussi
        ok(s.call("save_settings", &json!({"clear": true, "clear_deepl": true})));
        assert_eq!((store.peek("igdb_client_secret"), store.peek("deepl_key")), (None, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sans_coffre_les_cles_restent_dans_le_fichier() {
        let dir = temp_dir("novault");
        let s = Service::open_with(&dir, FakeTransport::with(vec![]), Vault::none()).unwrap();
        let c = ok(s.call("save_settings", &json!({"igdb_client_id": "a", "igdb_client_secret": "b"})));
        assert_eq!((c["secret_storage"].as_str(), c["secret_storage_name"].is_null()), (Some("file"), true));
        assert!(std::fs::read_to_string(crate::config::Config::path(&dir)).unwrap().contains("\"b\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn heures_jouees() {
        let (s, dir) = offline("hours");
        let id = ok(s.call("search", &json!({"query": "hades"})))["results"][0]["id"].as_u64().unwrap();
        let added = ok(s.call("add", &json!({"id": id, "status": "playing"})));
        assert!(added["entry"]["minutes"].is_null() && added["entry"]["hours_text"].is_null());
        for (input, minutes, text) in [(json!("12h30"), 750, "12 h 30"), (json!("12,5"), 750, "12 h 30"), (json!(3), 180, "3 h"), (json!(0.75), 45, "45 min"), (json!("45 min"), 45, "45 min")] {
            let r = ok(s.call("set_hours", &json!({"id": id, "hours": input})));
            assert_eq!((r["entry"]["minutes"].as_u64(), r["entry"]["hours_text"].as_str()), (Some(minutes), Some(text)), "{input}");
        }
        // statut, note et plateforme ne touchent pas au temps de jeu
        ok(s.call("set_status", &json!({"id": id, "status": "completed"})));
        ok(s.call("set_platform", &json!({"id": id, "platform": "Switch 2"})));
        assert_eq!(ok(s.call("set_rating", &json!({"id": id, "rating": 4.0})))["entry"]["minutes"], 45);
        // « Annuler » après un retrait : le temps est rendu avec les autres champs
        ok(s.call("remove", &json!({"id": id})));
        let back = ok(s.call("add", &json!({"id": id, "status": "completed", "minutes": 45})));
        assert_eq!(back["entry"]["minutes"], 45);
        // persistance
        let s2 = Service::open(&dir, FakeTransport::with(vec![])).unwrap();
        assert_eq!(ok(s2.call("library", &json!({})))["entries"][0]["entry"]["hours_text"], "45 min");
        // erreurs : rien n'est modifié
        for bad in [json!("beaucoup"), json!(-2), json!("12h99"), json!(1_000_000), json!(true)] {
            assert_eq!(s2.call("set_hours", &json!({"id": id, "hours": bad}))["ok"], false, "{bad}");
        }
        assert_eq!(s2.call("set_hours", &json!({"id": 424_242, "hours": 3}))["ok"], false, "jeu absent de la bibliothèque");
        assert_eq!(ok(s2.call("library", &json!({})))["entries"][0]["entry"]["minutes"], 45);
        // effacement : null, vide ou zéro
        for clear in [json!(null), json!(""), json!(0)] {
            ok(s2.call("set_hours", &json!({"id": id, "hours": 2})));
            assert!(ok(s2.call("set_hours", &json!({"id": id, "hours": clear})))["entry"]["minutes"].is_null());
        }
    }

    #[test]
    fn tri_par_heures_et_statistiques_detaillees() {
        let (s, _dir) = offline("stats2");
        s.seed_demo().unwrap();
        let lib = ok(s.call("library", &json!({"sort": "hours"})));
        let mins: Vec<u64> = lib["entries"].as_array().unwrap().iter().map(|c| c["entry"]["minutes"].as_u64().unwrap_or(0)).collect();
        assert!(mins.windows(2).all(|w| w[0] >= w[1]), "décroissant : {mins:?}");
        let st = ok(s.call("stats", &json!({})));
        let total = st["total"].as_u64().unwrap();
        assert!(total >= 10);
        // la somme des heures par plateforme ne dépasse pas le total (un jeu = une plateforme jouée)
        let total_min = st["hours"]["total_minutes"].as_u64().unwrap();
        assert!(total_min > 0 && st["hours"]["games_timed"].as_u64().unwrap() > 0);
        let by_platform: u64 = st["platforms"].as_array().unwrap().iter().map(|g| g["minutes"].as_u64().unwrap()).sum();
        assert!(by_platform <= total_min && by_platform > 0);
        assert!(st["hours"]["longest"].as_array().unwrap().len() <= 5);
        let top = st["top_rated"].as_array().unwrap();
        assert!(top.len() <= 5 && !top.is_empty());
        let stars: Vec<f64> = top.iter().map(|c| c["entry"]["stars"].as_f64().unwrap()).collect();
        assert!(stars.windows(2).all(|w| w[0] >= w[1]), "meilleures notes d'abord : {stars:?}");
        let started = st["counts"]["playing"].as_u64().unwrap() + st["counts"]["completed"].as_u64().unwrap() + st["counts"]["played"].as_u64().unwrap() + st["counts"]["abandoned"].as_u64().unwrap();
        assert_eq!(st["started"], started);
        let rate = st["finish_rate"].as_f64().unwrap();
        assert!((rate - st["counts"]["completed"].as_f64().unwrap() / started as f64 * 100.0).abs() < 0.06, "{rate}");
        assert_eq!(st["backlog_pending"].as_u64().unwrap(), st["counts"]["backlog"].as_u64().unwrap() + st["counts"]["wishlist"].as_u64().unwrap());
        assert!(st["median"].as_f64().is_some() && st["commented"].as_u64().unwrap() > 0);
        // plateformes : celle choisie par le joueur, jamais en double pour un même jeu
        let plats = st["platforms"].as_array().unwrap();
        assert!(plats.iter().any(|g| g["name"] == "PC"));
        assert!(plats.iter().map(|g| g["count"].as_u64().unwrap()).sum::<u64>() <= total);
        // décennies triées
        let decades: Vec<String> = st["decades"].as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap().to_string()).collect();
        let mut sorted = decades.clone();
        sorted.sort();
        assert_eq!(decades, sorted);
    }

    #[test]
    fn statistiques_d_une_bibliotheque_vide() {
        let (s, _dir) = offline("stats0");
        let st = ok(s.call("stats", &json!({})));
        assert_eq!(st["total"], 0);
        assert!(st["finish_rate"].is_null() && st["median"].is_null() && st["hours"]["average_finish_text"].is_null());
        assert_eq!(st["hours"]["total_text"], "0 min");
        assert!(st["top_rated"].as_array().unwrap().is_empty() && st["genres"].as_array().unwrap().is_empty());
    }

    #[test]
    fn plateforme_commentaire_et_persistance() {
        let (s, dir) = offline("pc");
        let id = ok(s.call("search", &json!({"query": "hades"})))["results"][0]["id"].as_u64().unwrap();
        let added = ok(s.call("add", &json!({"id": id, "status": "backlog"})));
        assert!(added["entry"]["platform"].is_null() && added["entry"]["comment"].is_null());
        let r = ok(s.call("set_platform", &json!({"id": id, "platform": "  Switch "})));
        assert_eq!(r["entry"]["platform"], "Switch");
        let r = ok(s.call("set_comment", &json!({"id": id, "comment": "Super jeu.\nÀ finir avant décembre."})));
        assert_eq!(r["entry"]["comment"], "Super jeu.\nÀ finir avant décembre.");
        // changer de statut ou de note ne perd ni plateforme ni commentaire
        ok(s.call("set_status", &json!({"id": id, "status": "playing"})));
        let r = ok(s.call("set_rating", &json!({"id": id, "rating": 4.0})));
        assert_eq!((r["entry"]["platform"].as_str(), r["entry"]["comment"].as_str()), (Some("Switch"), Some("Super jeu.\nÀ finir avant décembre.")));
        // ré-ajouter (« Annuler » après un retrait) restitue tout
        ok(s.call("remove", &json!({"id": id})));
        let back = ok(s.call("add", &json!({"id": id, "status": "playing", "rating": 4.0, "platform": "Switch", "comment": "Super jeu.\nÀ finir avant décembre."})));
        assert_eq!((back["entry"]["platform"].as_str(), back["entry"]["stars"].as_f64()), (Some("Switch"), Some(4.0)));
        // les données survivent à un redémarrage
        let s2 = Service::open(&dir, FakeTransport::with(vec![])).unwrap();
        let lib = ok(s2.call("library", &json!({})));
        assert_eq!(lib["entries"][0]["entry"]["comment"], "Super jeu.\nÀ finir avant décembre.");
        // effacement : null ou texte vide
        assert!(ok(s2.call("set_platform", &json!({"id": id, "platform": null})))["entry"]["platform"].is_null());
        assert!(ok(s2.call("set_comment", &json!({"id": id, "comment": "   "})))["entry"]["comment"].is_null());
        // limites et erreurs
        assert_eq!(s2.call("set_comment", &json!({"id": id, "comment": "x".repeat(10_001)}))["ok"], false);
        assert_eq!(s2.call("set_platform", &json!({"id": id, "platform": "x".repeat(61)}))["ok"], false);
        assert_eq!(s2.call("set_comment", &json!({"id": 424_242, "comment": "a"}))["ok"], false, "jeu absent de la bibliothèque");
        assert_eq!(ok(s2.call("set_comment", &json!({"id": id, "comment": "x".repeat(10_000)})))["entry"]["comment"].as_str().unwrap().len(), 10_000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fiche_detaillee_en_francais_filtres_et_facettes() {
        let (s, dir, _t) = with_igdb("facets", vec![Ok(TOKEN_OK.into()), Ok(DETAILED.into())]);
        let r = ok(s.call("search", &json!({"query": "jeux"})));
        let hades = &r["results"][0];
        assert_eq!(hades["released"], "17 septembre 2020");
        assert_eq!(hades["genres"], json!(["Jeu de rôle", "Indépendant"]));
        assert_eq!((hades["themes"][0].as_str(), hades["modes"][0].as_str()), (Some("Fantasy"), Some("Solo")));
        assert_eq!(hades["developers"], json!(["Supergiant Games"]));
        assert_eq!(hades["publishers"], json!(["Supergiant Games"]));
        assert_eq!((hades["franchise"].as_str(), hades["rev"].as_u64()), (Some("Hades"), Some(2)));
        for (id, st, stars) in [(1, "completed", 5.0), (2, "playing", 4.0), (3, "backlog", 0.0)] {
            ok(s.call("add", &json!({"id": id, "status": st})));
            if stars > 0.0 {
                ok(s.call("set_rating", &json!({"id": id, "rating": stars})));
            }
        }
        ok(s.call("set_platform", &json!({"id": 1, "platform": "Switch"})));
        let lib = ok(s.call("library", &json!({})));
        let names = |v: &Value| v.as_array().unwrap().iter().map(|x| format!("{}:{}", x["name"].as_str().unwrap(), x["count"])).collect::<Vec<_>>();
        assert_eq!(names(&lib["facets"]["genres"]), vec!["Indépendant:2", "Jeu de rôle:1", "Plateforme:1", "Tir:1"]);
        // Hades a choisi « Switch » : seule cette plateforme compte pour lui ; les autres jeux gardent celles d'IGDB
        assert_eq!(names(&lib["facets"]["platforms"]), vec!["PC:2", "Switch:2", "PS4:1"]);
        assert_eq!(lib["facets"]["developers"].as_array().unwrap().len(), 3);
        let ids = |v: &Value| v["entries"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap()).collect::<Vec<_>>();
        let by = |args: Value| ok(s.call("library", &args));
        assert_eq!(by(json!({"genre": "Tir"}))["total"], 3, "le total ne dépend pas des filtres");
        assert_eq!(ids(&by(json!({"genre": "Indépendant", "sort": "name"}))), vec![2, 1]);
        assert_eq!(ids(&by(json!({"platform": "PC", "sort": "name"}))), vec![2, 3]);
        assert_eq!(ids(&by(json!({"platform": "Switch", "sort": "name"}))), vec![2, 1]);
        assert_eq!(ids(&by(json!({"developer": "id Software"}))), vec![3]);
        assert_eq!(ids(&by(json!({"min_rating": 4.5}))), vec![1]);
        assert_eq!(ids(&by(json!({"min_rating": 4.0, "sort": "name"}))), vec![2, 1]);
        assert!(ids(&by(json!({"genre": "Indépendant", "developer": "id Software"}))).is_empty(), "filtres cumulés");
        // les compteurs de statut suivent les filtres, les listes de choix non
        let f = by(json!({"genre": "Indépendant"}));
        assert_eq!((f["counts"]["all"].as_u64(), f["counts"]["backlog"].as_u64(), f["counts"]["playing"].as_u64()), (Some(2), Some(0), Some(1)));
        assert_eq!(f["facets"]["genres"].as_array().unwrap().len(), 4);
        let f = by(json!({"genre": "Indépendant", "status": "playing"}));
        assert_eq!(ids(&f), vec![2]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn traduction_du_resume_gardee_en_cache() {
        let deepl = r#"{"translations":[{"detected_source_language":"EN","text":"Défie le dieu des morts."}]}"#;
        let (s, dir, t) = with_igdb("tr", vec![Ok(TOKEN_OK.into()), Ok(DETAILED.into()), Ok(deepl.into())]);
        let id = ok(s.call("search", &json!({"query": "hades"})))["results"][0]["id"].as_u64().unwrap();
        assert_eq!(s.call("translate", &json!({"id": id}))["ok"], false, "sans clé DeepL : message clair");
        assert_eq!(ok(s.call("get_settings", &json!({})))["has_deepl"], false);
        let cfg = ok(s.call("save_settings", &json!({"deepl_key": "secret-key:fx"})));
        assert_eq!(cfg["has_deepl"], true);
        assert!(!cfg.to_string().contains("secret-key"), "la clé DeepL ne doit jamais être renvoyée");
        ok(s.call("add", &json!({"id": id, "status": "backlog"})));
        let r = ok(s.call("translate", &json!({"id": id})));
        assert_eq!((r["summary"].as_str(), r["summary_fr"].as_str()), (Some("Defy the god of the dead."), Some("Défie le dieu des morts.")));
        // seconde demande : aucune requête réseau de plus
        let n = t.calls().len();
        assert_eq!(ok(s.call("translate", &json!({"id": id})))["summary_fr"], "Défie le dieu des morts.");
        assert_eq!(t.calls().len(), n);
        // la traduction est enregistrée avec la fiche
        let s2 = Service::open(&dir, FakeTransport::with(vec![])).unwrap();
        assert_eq!(ok(s2.call("library", &json!({})))["entries"][0]["summary_fr"], "Défie le dieu des morts.");
        // jeu sans résumé
        let cel = ok(s.call("search", &json!({"query": "celeste"}))); // plus de réponse simulée : repli hors ligne
        assert_eq!(cel["source"], "offline");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn jeu_sans_resume_et_effacement_de_la_cle_deepl() {
        let (s, dir, _t) = with_igdb("tr2", vec![Ok(TOKEN_OK.into()), Ok(DETAILED.into())]);
        ok(s.call("search", &json!({"query": "jeux"})));
        s.call("save_settings", &json!({"deepl_key": "k"}));
        let e = s.call("translate", &json!({"id": 2}));
        assert!(e["error"].as_str().unwrap().contains("pas de résumé"), "{e}");
        // effacer la clé DeepL laisse IGDB intact, et inversement
        let c = ok(s.call("save_settings", &json!({"clear_deepl": true})));
        assert_eq!((c["has_deepl"].as_bool(), c["configured"].as_bool()), (Some(false), Some(true)));
        s.call("save_settings", &json!({"deepl_key": "k"}));
        let c = ok(s.call("save_settings", &json!({"clear": true})));
        assert_eq!((c["has_deepl"].as_bool(), c["configured"].as_bool()), (Some(true), Some(false)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rafraichir_une_fiche_ancienne_garde_la_traduction() {
        let dir = temp_dir("refresh");
        // fiche enregistrée avant l'ajout des détails (rev 0) avec une traduction
        let mut store = crate::store::Store::default();
        let old = GameInfo { id: 1, name: "Hades".into(), slug: "hades--1".into(), summary: Some("Defy the god of the dead.".into()), summary_fr: Some("Défie le dieu des morts.".into()), ..GameInfo::default() };
        store.games.insert(1, old);
        store.library.upsert(1, Status::Playing, None);
        store.save(&dir).unwrap();
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(DETAILED.into()), Ok("[]".into()), Ok(DETAILED.into())]);
        let s = Service::open(&dir, t.clone()).unwrap();
        assert_eq!(s.call("refresh", &json!({"id": 1}))["error"].as_str().map(|e| e.contains("IGDB")), Some(true), "sans clés");
        s.call("save_settings", &json!({"igdb_client_id": "a", "igdb_client_secret": "b"}));
        let r = ok(s.call("refresh", &json!({"id": 1})));
        assert_eq!((r["rev"].as_u64(), r["developers"][0].as_str()), (Some(3), Some("Supergiant Games")));
        assert_eq!(r["summary_fr"], "Défie le dieu des morts.", "résumé inchangé : traduction conservée");
        assert_eq!(r["entry"]["status"], "playing");
        assert!(t.calls()[1].2.contains("where id = (1);"), "requête des fiches (la dernière demande les durées)");
        // fiche déjà complète : la synchronisation globale n'a plus rien à faire
        let sync = ok(s.call("sync_metadata", &json!({})));
        assert_eq!((sync["updated"].as_u64(), sync["pending"].as_u64()), (Some(0), Some(0)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keep_cached_ne_reprend_les_durees_que_pour_une_fiche_detaillee() {
        let ttb = crate::igdb::TimeToBeat { normally: Some(90000), count: 12, ..Default::default() };
        let old = GameInfo { id: 1, name: "Hades".into(), rev: REV_FULL, time_to_beat: Some(ttb.clone()), ..GameInfo::default() };
        let mut n2 = GameInfo { id: 1, name: "Hades".into(), rev: 2, ..GameInfo::default() };
        keep_cached(Some(&old), &mut n2);
        assert_eq!((n2.rev, n2.time_to_beat), (REV_FULL, Some(ttb)));
        let mut n1 = GameInfo { id: 1, name: "Hades".into(), rev: 1, ..GameInfo::default() };
        keep_cached(Some(&old), &mut n1);
        assert_eq!((n1.rev, n1.time_to_beat), (1, None));
        let mut n3 = GameInfo { id: 1, name: "Hades".into(), rev: REV_FULL, ..GameInfo::default() };
        keep_cached(Some(&old), &mut n3);
        assert_eq!((n3.rev, n3.time_to_beat), (REV_FULL, None));
    }

    #[test]
    fn url_hltb_encodee() {
        assert_eq!(hltb_url("Hades"), "https://howlongtobeat.com/?q=Hades");
        assert_eq!(
            hltb_url("Ratchet & Clank: \"Rift\" #2 é"),
            "https://howlongtobeat.com/?q=Ratchet%20%26%20Clank%3A%20%22Rift%22%20%232%20%C3%A9"
        );
    }

    #[test]
    fn carte_avec_durees() {
        let dir = temp_dir("carte-durees");
        let mut store = crate::store::Store::default();
        let ttb = crate::igdb::TimeToBeat { hastily: Some(72000), normally: Some(91200), completely: None, count: 3 };
        store.games.insert(1, GameInfo { id: 1, name: "Hades".into(), slug: "hades--1".into(), rev: REV_FULL, time_to_beat: Some(ttb), ..GameInfo::default() });
        store.games.insert(2, GameInfo { id: 2, name: "Celeste".into(), slug: "celeste--2".into(), rev: REV_FULL, ..GameInfo::default() });
        store.games.insert(3, GameInfo { id: 3, name: "Vide".into(), slug: "vide--3".into(), rev: REV_FULL, time_to_beat: Some(crate::igdb::TimeToBeat::default()), ..GameInfo::default() });
        store.library.upsert(1, Status::Playing, None);
        store.library.upsert(2, Status::Backlog, None);
        store.library.upsert(3, Status::Backlog, None);
        store.save(&dir).unwrap();
        let s = Service::open(&dir, FakeTransport::with(vec![])).unwrap();
        let lib = ok(s.call("library", &json!({})));
        let find = |id: u64| lib["entries"].as_array().unwrap().iter().find(|e| e["id"] == id).unwrap().clone();
        let (a, b) = (find(1), find(2));
        assert_eq!(a["ttb_text"], "~25 h");
        assert_eq!(a["ttb_ref"], 91200);
        assert_eq!(a["ttb"]["normally"], 91200);
        assert_eq!(a["ttb"]["normally_text"], "~25 h");
        assert_eq!(a["ttb"]["completely"], Value::Null);
        assert_eq!(a["ttb"]["completely_text"], Value::Null);
        assert_eq!(a["ttb"]["count"], 3);
        assert_eq!(a["ttb"]["few_votes"], true);
        assert_eq!(a["hltb_url"], "https://howlongtobeat.com/?q=Hades");
        assert_eq!((b["ttb"].clone(), b["ttb_ref"].clone(), b["ttb_text"].clone()), (Value::Null, Value::Null, Value::Null));
        assert_eq!(b["hltb_url"], "https://howlongtobeat.com/?q=Celeste");
        let c = find(3);
        assert_eq!((c["ttb"].clone(), c["ttb_ref"].clone()), (Value::Null, Value::Null), "entrée IGDB sans aucune durée utilisable");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Bibliothèque de jeux dont la durée de référence (normale) est donnée en secondes.
    fn service_avec_durees(tag: &str, games: &[(u64, &str, Option<u32>)]) -> (Service, std::path::PathBuf) {
        let dir = temp_dir(tag);
        let mut store = crate::store::Store::default();
        for (id, name, secs) in games {
            let time_to_beat = secs.map(|s| crate::igdb::TimeToBeat { normally: Some(s), count: 10, ..Default::default() });
            store.games.insert(*id, GameInfo { id: *id, name: (*name).into(), slug: format!("{name}--{id}"), rev: REV_FULL, time_to_beat, ..GameInfo::default() });
            store.library.upsert(*id, Status::Backlog, None);
        }
        store.save(&dir).unwrap();
        (Service::open(&dir, FakeTransport::with(vec![])).unwrap(), dir)
    }

    fn noms(lib: &Value) -> Vec<String> {
        lib["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn tri_par_duree_estimee() {
        let h = 3600;
        let (s, dir) = service_avec_durees("tri-ttb", &[(1, "A", Some(20 * h)), (2, "Zelda", None), (3, "C", Some(3 * h)), (4, "Alpha", None)]);
        let lib = ok(s.call("library", &json!({"sort": "ttb"})));
        assert_eq!(noms(&lib), ["C", "A", "Alpha", "Zelda"], "sans durée à la fin, par nom");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn filtre_par_tranche_de_duree() {
        let (h, m) = (3600, 60);
        let games = [
            (1, "g1", Some(4 * h + 59 * m)),
            (2, "g2", Some(5 * h)),
            (3, "g3", Some(14 * h + 59 * m)),
            (4, "g4", Some(15 * h)),
            (5, "g5", Some(39 * h + 59 * m)),
            (6, "g6", Some(40 * h)),
            (7, "g7", None),
        ];
        let (s, dir) = service_avec_durees("filtre-ttb", &games);
        let pick = |f: &str| {
            let mut n = noms(&ok(s.call("library", &json!({"ttb": f, "sort": "name"}))));
            n.sort();
            n
        };
        assert_eq!(pick("lt5"), ["g1"]);
        assert_eq!(pick("5to15"), ["g2", "g3"]);
        assert_eq!(pick("15to40"), ["g4", "g5"]);
        assert_eq!(pick("ge40"), ["g6"]);
        assert_eq!(pick("").len(), 7);
        assert_eq!(pick("nimporte").len(), 7);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rpc_hltb_url() {
        let dir = temp_dir("rpc-hltb");
        let mut store = crate::store::Store::default();
        store.games.insert(1, GameInfo { id: 1, name: "Hades II".into(), slug: "hades-ii--1".into(), ..GameInfo::default() });
        store.library.upsert(1, Status::Playing, None);
        store.save(&dir).unwrap();
        let s = Service::open(&dir, FakeTransport::with(vec![])).unwrap();
        assert_eq!(ok(s.call("hltb_url", &json!({"id": 1})))["url"], "https://howlongtobeat.com/?q=Hades%20II");
        assert!(s.call("hltb_url", &json!({"id": 999}))["error"].is_string());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refresh_garde_les_durees_si_la_requete_echoue() {
        let dir = temp_dir("refresh-durees");
        let mut store = crate::store::Store::default();
        let ttb = crate::igdb::TimeToBeat { normally: Some(90000), count: 12, ..Default::default() };
        let old = GameInfo { id: 1, name: "Hades".into(), slug: "hades--1".into(), rev: REV_FULL, time_to_beat: Some(ttb), ..GameInfo::default() };
        store.games.insert(1, old);
        store.library.upsert(1, Status::Playing, None);
        store.save(&dir).unwrap();
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(DETAILED.into()), Err(Error::Network("coupure".into()))]);
        let s = Service::open(&dir, t).unwrap();
        s.call("save_settings", &json!({"igdb_client_id": "a", "igdb_client_secret": "b"}));
        let r = ok(s.call("refresh", &json!({"id": 1})));
        assert_eq!(r["ttb"]["normally"], 90000, "durées conservées");
        assert_eq!(r["rev"], 3);
        assert_eq!(r["developers"][0], "Supergiant Games", "le reste de la fiche est bien rafraîchi");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_complete_les_fiches_rev2_seulement() {
        let dir = temp_dir("sync-rev2");
        let mut store = crate::store::Store::default();
        store.games.insert(1, GameInfo { id: 1, name: "Hades".into(), slug: "hades--1".into(), rev: 2, ..GameInfo::default() });
        store.games.insert(2, GameInfo { id: 2, name: "Celeste".into(), slug: "celeste--2".into(), rev: REV_FULL, ..GameInfo::default() });
        store.library.upsert(1, Status::Playing, None);
        store.library.upsert(2, Status::Backlog, None);
        store.save(&dir).unwrap();
        let only_a = r#"[{"id":1,"name":"Hades","slug":"hades--1"}]"#;
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(only_a.into()), Ok("[]".into())]);
        let s = Service::open(&dir, t.clone()).unwrap();
        s.call("save_settings", &json!({"igdb_client_id": "a", "igdb_client_secret": "b"}));
        let sync = ok(s.call("sync_metadata", &json!({})));
        assert_eq!((sync["updated"].as_u64(), sync["pending"].as_u64()), (Some(1), Some(0)));
        let calls = t.calls();
        assert_eq!(calls.len(), 3, "jeton, fiches, durées");
        assert!(calls[1].2.contains("where id = (1);"), "seul le jeu en rev 2 est demandé : {}", calls[1].2);
        let again = ok(s.call("sync_metadata", &json!({})));
        assert_eq!(again["updated"], 0);
        assert_eq!(t.calls().len(), 3, "aucun appel réseau au second passage");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_sans_durees_ne_compte_pas_les_fiches_incompletes() {
        let dir = temp_dir("sync-echec-durees");
        let mut store = crate::store::Store::default();
        for (id, n) in [(1u64, "Hades"), (2, "Celeste")] {
            store.games.insert(id, GameInfo { id, name: n.into(), slug: format!("{n}--{id}"), rev: 2, ..GameInfo::default() });
            store.library.upsert(id, Status::Backlog, None);
        }
        store.save(&dir).unwrap();
        let games = r#"[{"id":1,"name":"Hades","slug":"hades--1"},{"id":2,"name":"Celeste","slug":"celeste--2"}]"#;
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(games.into()), Err(Error::Network("coupure".into()))]);
        let s = Service::open(&dir, t).unwrap();
        s.call("save_settings", &json!({"igdb_client_id": "a", "igdb_client_secret": "b"}));
        let sync = ok(s.call("sync_metadata", &json!({})));
        assert_eq!((sync["updated"].as_u64(), sync["pending"].as_u64()), (Some(0), Some(2)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Comme `offline`, avec un dossier d'export à part (jamais le vrai dossier Téléchargements).
    fn with_exports(tag: &str) -> (Service, PathBuf) {
        let (mut s, dir) = offline(tag);
        s.export_dir = dir.join("sortie");
        (s, dir)
    }

    fn read_export(r: &Value) -> String {
        std::fs::read_to_string(r["path"].as_str().unwrap()).unwrap()
    }

    fn status_of(s: &Service, id: u64) -> String {
        let lib = ok(s.call("library", &json!({})));
        lib["entries"].as_array().unwrap().iter().find(|e| e["id"] == id).unwrap()["entry"]["status"].as_str().unwrap().to_string()
    }

    /// Un statut différent de l'actuel, pour que la modification soit bien visible.
    fn other_status(s: &Service, id: u64) -> &'static str {
        if status_of(s, id) == "wishlist" { "backlog" } else { "wishlist" }
    }

    fn live_ids(s: &Service) -> Vec<u64> {
        let mut v: Vec<u64> = ok(s.call("library", &json!({"sort": "name"})))["entries"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap()).collect();
        v.sort();
        v
    }

    #[test]
    fn export_csv_pour_excel() {
        let (s, dir) = with_exports("csv");
        assert_eq!(s.call("export_file", &json!({"format": "csv"}))["ok"], false, "bibliothèque vide");
        let n = s.seed_demo().unwrap();
        let id = live_ids(&s)[0];
        ok(s.call("set_comment", &json!({"id": id, "comment": "=SOMME(A1)"})));
        ok(s.call("set_hours", &json!({"id": id, "hours": "12h20"})));
        ok(s.call("set_rating", &json!({"id": id, "rating": 4.5})));
        let r = ok(s.call("export_file", &json!({"format": "csv"})));
        assert_eq!(r["games"].as_u64(), Some(n as u64));
        let text = read_export(&r);
        assert!(text.starts_with("\u{feff}Titre;Année;Statut;Note (sur 5);"), "BOM et en-tête");
        assert_eq!(text.lines().count(), n + 1);
        assert!(text.lines().all(|l| l.ends_with(&['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'][..]) || l.starts_with('\u{feff}')), "dernière colonne : l'identifiant");
        let line = text.lines().find(|l| l.ends_with(&format!(";{id}"))).unwrap();
        assert!(line.contains(";'=SOMME(A1);"), "formule neutralisée : {line}");
        assert!(line.contains(";4,5;") && line.contains(";12,33;"), "virgule décimale : {line}");
        // un second export le même jour ne remplace pas le premier
        let r2 = ok(s.call("export_file", &json!({"format": "csv"})));
        assert_ne!(r["path"], r2["path"]);
        assert!(r2["file"].as_str().unwrap().ends_with("-2.csv"));
        assert!(std::path::Path::new(r["path"].as_str().unwrap()).exists());
        assert_eq!(s.call("export_file", &json!({"format": "xml"}))["ok"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_json_se_reimporte_dans_une_bibliotheque_vide() {
        let (a, dir_a) = with_exports("exp-a");
        a.seed_demo().unwrap();
        let file = read_export(&ok(a.call("export_file", &json!({"format": "json"}))));
        let (b, dir_b) = with_exports("exp-b");
        let plan = ok(b.call("import_preview", &json!({"content": file})));
        assert_eq!((plan["file_games"].as_u64(), plan["local_games"].as_u64(), plan["add"]["added"].as_u64()), (Some(18), Some(0), Some(18)));
        let done = ok(b.call("import_apply", &json!({"content": file, "mode": "replace"})));
        assert_eq!(done["games"], 18);
        assert_eq!(live_ids(&a), live_ids(&b));
        let (la, lb) = (ok(a.call("library", &json!({"sort": "name"}))), ok(b.call("library", &json!({"sort": "name"}))));
        assert_eq!(la["entries"], lb["entries"], "statuts, notes, plateformes, commentaires, heures");
        // pas de sauvegarde d'avant-import quand il n'y avait rien à protéger
        assert!(store::list_backups(&dir_b).iter().all(|b| b.kind != "avant-import"));
        // réimporter le même fichier ne change rien
        let again = ok(b.call("import_preview", &json!({"content": file})));
        for mode in ["add", "merge", "replace"] {
            assert_eq!((again[mode]["added"].as_u64(), again[mode]["updated"].as_u64(), again[mode]["removed"].as_u64()), (Some(0), Some(0), Some(0)), "{mode}");
        }
        let _ = std::fs::remove_dir_all(&dir_a);
        let _ = std::fs::remove_dir_all(&dir_b);
    }

    #[test]
    fn les_trois_modes_d_import() {
        let (s, dir) = with_exports("modes");
        s.seed_demo().unwrap();
        let ids = live_ids(&s);
        let (updated, removed) = (ids[0], ids[1]);
        let old = read_export(&ok(s.call("export_file", &json!({"format": "json"}))));
        // modifications faites ici après l'export : elles sont plus récentes que le fichier
        let (before, changed) = (status_of(&s, updated), other_status(&s, updated));
        ok(s.call("set_status", &json!({"id": updated, "status": changed})));
        ok(s.call("remove", &json!({"id": removed})));
        let plan = ok(s.call("import_preview", &json!({"content": old})));
        assert_eq!((plan["add"]["added"].as_u64(), plan["add"]["updated"].as_u64()), (Some(1), Some(0)), "ajouter : seul le jeu retiré revient");
        assert_eq!((plan["merge"]["added"].as_u64(), plan["merge"]["updated"].as_u64(), plan["merge"]["removed"].as_u64()), (Some(0), Some(0), Some(0)), "fusionner : ici est plus récent");
        assert_eq!((plan["replace"]["added"].as_u64(), plan["replace"]["updated"].as_u64()), (Some(1), Some(1)));
        assert_eq!(plan["add"]["added_names"].as_array().unwrap().len(), 1);
        // mode « ajouter » : le statut modifié ici n'est pas touché
        ok(s.call("import_apply", &json!({"content": old, "mode": "add"})));
        assert_eq!(live_ids(&s).len(), 18);
        assert_eq!(status_of(&s, updated), changed);
        // mode « remplacer » : le fichier redevient la bibliothèque
        let r = ok(s.call("import_apply", &json!({"content": old, "mode": "replace"})));
        assert_eq!(r["games"], 18);
        assert_eq!(status_of(&s, updated), before);
        // chaque import a laissé une sauvegarde d'avant-import
        let kinds: Vec<String> = store::list_backups(&dir).into_iter().map(|b| b.kind).filter(|k| k == "avant-import").collect();
        assert_eq!(kinds.len(), 2);
        assert_eq!(s.call("import_apply", &json!({"content": old, "mode": "tout"}))["ok"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fusionner_adopte_ce_qui_est_plus_recent_dans_le_fichier() {
        let (s, dir) = with_exports("merge");
        s.seed_demo().unwrap();
        let ids = live_ids(&s);
        let (updated, removed) = (ids[0], ids[1]);
        let old = read_export(&ok(s.call("export_file", &json!({"format": "json"}))));
        let changed = other_status(&s, updated);
        ok(s.call("set_status", &json!({"id": updated, "status": changed})));
        ok(s.call("remove", &json!({"id": removed})));
        let newer = read_export(&ok(s.call("export_file", &json!({"format": "json"}))));
        // retour à l'ancienne version, puis le fichier récent devient la nouveauté
        ok(s.call("import_apply", &json!({"content": old, "mode": "replace"})));
        let plan = ok(s.call("import_preview", &json!({"content": newer})));
        assert_eq!((plan["merge"]["updated"].as_u64(), plan["merge"]["removed"].as_u64(), plan["merge"]["added"].as_u64()), (Some(1), Some(1), Some(0)));
        assert_eq!(plan["add"]["added"], 0, "ajouter ne retire ni ne modifie rien");
        ok(s.call("import_apply", &json!({"content": newer, "mode": "merge"})));
        assert_eq!(live_ids(&s).len(), 17);
        assert_eq!(status_of(&s, updated), changed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_refuse_les_fichiers_invalides() {
        let (s, dir) = with_exports("bad-import");
        s.seed_demo().unwrap();
        for bad in ["", "   ", "pas du json", "{}", "[1,2]", "{\"version\":2,\"games\":{},\"library\":{\"replica\":\"x\",\"clock\":0,\"entries\":{}}}"] {
            for method in ["import_preview", "import_apply"] {
                let r = s.call(method, &json!({"content": bad, "mode": "replace"}));
                assert_eq!(r["ok"], false, "{method} {bad}");
            }
        }
        assert_eq!(live_ids(&s).len(), 18, "rien n'a changé");
        assert!(store::list_backups(&dir).iter().all(|b| b.kind != "avant-import"), "pas de sauvegarde pour un import refusé");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sauvegardes_et_restauration() {
        let (s, dir) = with_exports("restore");
        assert_eq!(s.call("backup_now", &json!({}))["ok"], false, "rien à sauvegarder");
        s.seed_demo().unwrap();
        let ids = live_ids(&s);
        let list = ok(s.call("backup_now", &json!({})));
        let file = list["items"][0]["file"].as_str().unwrap().to_string();
        assert_eq!((list["items"][0]["kind"].as_str(), list["items"][0]["games"].as_u64()), (Some("manuelle"), Some(18)));
        ok(s.call("remove", &json!({"id": ids[0]})));
        ok(s.call("remove", &json!({"id": ids[1]})));
        assert_eq!(live_ids(&s).len(), 16);
        let r = ok(s.call("restore_backup", &json!({"file": file})));
        assert_eq!(r["games"], 18);
        assert_eq!(live_ids(&s), ids);
        let kinds: Vec<&str> = r["backups"]["items"].as_array().unwrap().iter().map(|b| b["kind"].as_str().unwrap()).collect();
        assert!(kinds.contains(&"avant-restauration"), "{kinds:?}");
        // la restauration est durable
        let again = Service::open(&dir, FakeTransport::with(vec![])).unwrap();
        assert_eq!(live_ids(&again), ids);
        // noms refusés
        for bad in ["", "../bibliotheque.json", "bibliotheque-20200101-000000-auto.json", "C:\\Windows\\win.ini"] {
            assert_eq!(s.call("restore_backup", &json!({"file": bad}))["ok"], false, "{bad}");
        }
        assert_eq!(live_ids(&s), ids);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fichier_abime_au_lancement_est_restaure_et_signale_une_fois() {
        let (s, dir) = with_exports("crash");
        s.seed_demo().unwrap();
        ok(s.call("set_status", &json!({"id": live_ids(&s)[0], "status": "wishlist"}))); // crée la copie .bak
        drop(s);
        std::fs::write(Store::path(&dir), "{ \"version\": 1, \"gam").unwrap();
        let s = Service::open(&dir, FakeTransport::with(vec![])).unwrap();
        assert_eq!(live_ids(&s).len(), 18);
        let first = ok(s.call("status", &json!({})));
        assert!(first["notice"].as_str().unwrap().contains("restaurée"), "{first}");
        assert!(ok(s.call("status", &json!({})))["notice"].is_null(), "montré une seule fois");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ouvrir_le_dossier_refuse_les_cibles_inconnues() {
        let (s, dir) = with_exports("reveal");
        for bad in [json!({}), json!({"target": "/etc"}), json!({"target": "..", "file": "x"})] {
            assert_eq!(s.call("reveal", &bad)["ok"], false, "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn entry_of(s: &Service, id: u64) -> Value {
        ok(s.call("library", &json!({})))["entries"].as_array().unwrap().iter().find(|e| e["id"] == id).unwrap()["entry"].clone()
    }

    #[test]
    fn dates_d_ajout_et_de_fin() {
        let (s, dir) = with_exports("dates");
        let id = ok(s.call("search", &json!({"query": "hades"})))["results"][0]["id"].as_u64().unwrap();
        let now = store::now_secs();
        let added = ok(s.call("add", &json!({"id": id, "status": "playing"})));
        assert!((now - added["entry"]["added"].as_i64().unwrap()).abs() < 60 && added["entry"]["finished"].is_null());
        // pas de date de fin pour un jeu en cours
        assert_eq!(s.call("set_dates", &json!({"id": id, "finished": now - 86_400}))["ok"], false);
        let done = ok(s.call("set_status", &json!({"id": id, "status": "completed"})));
        assert!(done["entry"]["finished"].as_i64().is_some(), "fin posée automatiquement");
        let (a, f) = (now - 400 * 86_400, now - 30 * 86_400);
        let r = ok(s.call("set_dates", &json!({"id": id, "added": a, "finished": f})));
        assert_eq!((r["entry"]["added"].as_i64(), r["entry"]["finished"].as_i64()), (Some(a), Some(f)));
        // un champ absent n'est pas touché, null efface
        let r = ok(s.call("set_dates", &json!({"id": id, "finished": null})));
        assert_eq!((r["entry"]["added"].as_i64(), r["entry"]["finished"].is_null()), (Some(a), true));
        // dates refusées, rien n'est modifié
        for bad in [json!("hier"), json!(0), json!(now + 30 * 86_400), json!(-5), json!(1.5e12), json!(true)] {
            assert_eq!(s.call("set_dates", &json!({"id": id, "added": bad}))["ok"], false, "{bad}");
        }
        assert_eq!(entry_of(&s, id)["added"].as_i64(), Some(a));
        assert_eq!(s.call("set_dates", &json!({"id": 424_242, "added": a}))["ok"], false, "jeu absent");
        // retrait puis « Annuler » : les dates d'origine reviennent
        ok(s.call("set_dates", &json!({"id": id, "finished": f})));
        let prev = entry_of(&s, id);
        ok(s.call("remove", &json!({"id": id})));
        let back = ok(s.call("add", &json!({"id": id, "status": "completed", "added": prev["added"], "finished": prev["finished"]})));
        assert_eq!((back["entry"]["added"].as_i64(), back["entry"]["finished"].as_i64()), (Some(a), Some(f)));
        // une date de fin envoyée avec un statut « en cours » est ignorée
        ok(s.call("remove", &json!({"id": id})));
        let back = ok(s.call("add", &json!({"id": id, "status": "playing", "finished": f})));
        assert!(back["entry"]["finished"].is_null());
        // persistance
        ok(s.call("set_status", &json!({"id": id, "status": "completed"})));
        ok(s.call("set_dates", &json!({"id": id, "added": a})));
        let again = Service::open(&dir, FakeTransport::with(vec![])).unwrap();
        assert_eq!(entry_of(&again, id)["added"].as_i64(), Some(a));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tri_par_date_d_ajout() {
        let (s, dir) = with_exports("sort-added");
        s.seed_demo().unwrap();
        let ids = live_ids(&s);
        // le plus récemment ajouté d'abord ; sans date en dernier
        ok(s.call("set_dates", &json!({"id": ids[0], "added": null})));
        let lib = ok(s.call("library", &json!({"sort": "added"})));
        let dates: Vec<Option<i64>> = lib["entries"].as_array().unwrap().iter().map(|e| e["entry"]["added"].as_i64()).collect();
        assert_eq!(dates.last().unwrap(), &None);
        let dated: Vec<i64> = dates.iter().flatten().copied().collect();
        assert!(dated.windows(2).all(|w| w[0] >= w[1]), "{dated:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn statistiques_d_anciennete_et_de_fin() {
        let (s, dir) = with_exports("timeline");
        let empty = ok(s.call("stats", &json!({})));
        assert_eq!(empty["timeline"]["backlog_dated"], 0);
        s.seed_demo().unwrap();
        let st = ok(s.call("stats", &json!({})));
        let t = &st["timeline"];
        assert_eq!(t["backlog_total"], st["counts"]["backlog"]);
        assert_eq!(t["backlog_dated"], t["backlog_total"]);
        let oldest = t["oldest"].as_array().unwrap();
        assert!(!oldest.is_empty() && oldest.len() <= 5);
        let ages: Vec<i64> = oldest.iter().map(|c| c["entry"]["added"].as_i64().unwrap()).collect();
        assert!(ages.windows(2).all(|w| w[0] <= w[1]), "du plus ancien au plus récent : {ages:?}");
        assert!(t["backlog_average_days"].as_i64().unwrap() > 0);
        let by_year: u64 = t["finished_by_year"].as_array().unwrap().iter().map(|y| y["count"].as_u64().unwrap()).sum();
        assert_eq!(by_year, st["counts"]["completed"].as_u64().unwrap(), "tous les jeux terminés de la démo sont datés");
        assert!(t["average_days_to_finish"].as_i64().unwrap() >= 0);
        // un jeu sans date n'entre dans aucune moyenne
        let id = oldest[0]["id"].as_u64().unwrap();
        ok(s.call("set_dates", &json!({"id": id, "added": null})));
        assert_eq!(ok(s.call("stats", &json!({})))["timeline"]["backlog_dated"].as_u64().unwrap(), t["backlog_dated"].as_u64().unwrap() - 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_csv_contient_les_dates() {
        let (s, dir) = with_exports("csv-dates");
        s.seed_demo().unwrap();
        let id = live_ids(&s).into_iter().find(|id| status_of(&s, *id) == "completed").unwrap();
        ok(s.call("set_dates", &json!({"id": id, "added": 1_735_732_800, "finished": 1_738_411_200})));
        let text = read_export(&ok(s.call("export_file", &json!({"format": "csv"}))));
        assert!(text.contains(";Ajouté le;Terminé le;Identifiant IGDB"));
        assert!(text.lines().any(|l| l.ends_with(&format!(";2025-01-01;2025-02-01;{id}"))), "dates ISO");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tri_par_plateforme() {
        let (s, dir) = with_exports("sort-platform");
        s.seed_demo().unwrap();
        let ids = live_ids(&s);
        ok(s.call("set_platform", &json!({"id": ids[0], "platform": null})));
        let lib = ok(s.call("library", &json!({"sort": "platform"})));
        let plats: Vec<String> = lib["entries"].as_array().unwrap().iter().map(|e| e["entry"]["platform"].as_str().unwrap_or("").to_lowercase()).collect();
        let first_empty = plats.iter().position(String::is_empty).unwrap();
        assert!(plats[first_empty..].iter().all(String::is_empty), "sans plateforme à la fin : {plats:?}");
        assert!(plats[..first_empty].windows(2).all(|w| w[0] <= w[1]), "ordre alphabétique : {plats:?}");
        assert!(plats[..first_empty].len() >= 10);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Lance le rapprochement jusqu'au bout, par lots, comme le fait l'interface.
    fn match_all(s: &Service, total: usize) -> Value {
        let mut offset = 0;
        while offset < total {
            let r = ok(s.call("csv_match", &json!({"offset": offset, "limit": 4})));
            offset += 4;
            assert!(r["done"].as_u64().unwrap() <= total as u64);
        }
        ok(s.call("csv_review", &json!({})))
    }

    fn pick(review: &Value, kinds: &[&str]) -> Vec<Value> {
        review["items"].as_array().unwrap().iter().filter(|i| kinds.contains(&i["kind"].as_str().unwrap())).map(|i| json!({"index": i["index"], "id": i["candidates"][0]["id"]})).collect()
    }

    #[test]
    fn import_csv_avec_le_catalogue_hors_ligne() {
        let (s, dir) = with_exports("csv-offline");
        // « Katana Zero » est déjà dans la bibliothèque
        let kz = ok(s.call("search", &json!({"query": "katana zero"})))["results"][0]["id"].as_u64().unwrap();
        ok(s.call("add", &json!({"id": kz, "status": "backlog"})));
        let csv = "Titre;Année;Statut;Note;Plateforme;Heures;Commentaire\n\
            hades;2020;Terminé;4,5;PC;12h30;Génial\n\
            Katana Zero;;Joué;;;;\n\
            Zelda;;;;;;\n\
            Un jeu qui n'existe pas du tout;;;;;;\n\
            Hades;;Backlog;;;;doublon\n";
        let start = ok(s.call("csv_start", &json!({"content": csv})));
        assert_eq!((start["rows"].as_u64(), start["mode"].as_str()), (Some(5), Some("offline")));
        let review = match_all(&s, 5);
        let kinds: Vec<&str> = review["items"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["sure", "present", "check", "notfound", "check"], "« Hades » sans année reste ambigu : {review}");
        assert!(review["items"][2]["candidates"].as_array().unwrap().len() > 1, "Zelda : plusieurs possibilités");
        assert_eq!(review["counts"]["sure"], 1);
        // ajout : les sûrs et le premier candidat de « Zelda » ; le jeu déjà présent et le doublon sont écartés
        let mut choices = pick(&review, &["sure", "check", "present"]);
        assert_eq!(choices.len(), 4);
        let r = ok(s.call("csv_apply", &json!({"choices": choices.clone()})));
        assert_eq!((r["added"].as_u64(), r["present"].as_u64(), r["duplicates"].as_u64()), (Some(2), Some(1), Some(1)));
        let lib = ok(s.call("library", &json!({"sort": "name"})));
        assert_eq!(lib["entries"].as_array().unwrap().len(), 3);
        let hades = lib["entries"].as_array().unwrap().iter().find(|e| e["name"] == "Hades").unwrap();
        let e = &hades["entry"];
        assert_eq!((e["status"].as_str(), e["stars"].as_f64(), e["platform"].as_str(), e["hours_text"].as_str(), e["comment"].as_str()), (Some("completed"), Some(4.5), Some("PC"), Some("12 h 30"), Some("Génial")));
        assert!(e["added"].is_null() && e["finished"].is_null(), "pas de date inventée : {e}");
        assert_eq!(status_of(&s, kz), "backlog", "le jeu déjà présent n'a pas bougé");
        // sauvegarde faite avant l'ajout
        assert!(store::list_backups(&dir).iter().any(|b| b.kind == "avant-import" && b.games == Some(1)));
        // l'import est terminé : plus rien en attente
        assert_eq!(s.call("csv_apply", &json!({"choices": choices.split_off(0)}))["ok"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_csv_refuse_les_choix_non_proposes_et_les_appels_hors_sequence() {
        let (s, dir) = with_exports("csv-guard");
        assert_eq!(s.call("csv_match", &json!({})) ["ok"], false, "rien en cours");
        assert_eq!(s.call("csv_review", &json!({}))["ok"], false);
        assert_eq!(s.call("csv_start", &json!({"content": "Genre\nRPG"}))["ok"], false, "pas de colonne de titres");
        ok(s.call("csv_start", &json!({"content": "Titre\nHades\n"})));
        let review = match_all(&s, 1);
        let real = review["items"][0]["candidates"][0]["id"].as_u64().unwrap();
        // un identifiant que la ligne n'a pas proposé est refusé, sans rien ajouter
        let bad = s.call("csv_apply", &json!({"choices": [{"index": 0, "id": real + 99_999}]}));
        assert_eq!(bad["ok"], false);
        assert!(ok(s.call("library", &json!({})))["entries"].as_array().unwrap().is_empty());
        // l'import reste en attente après un refus
        let ok_apply = ok(s.call("csv_apply", &json!({"choices": [{"index": 0, "id": real}]})));
        assert_eq!(ok_apply["added"], 1);
        // annuler vide l'attente ; sans choix, rien n'est ajouté
        ok(s.call("csv_start", &json!({"content": "Titre\nKatana Zero\n"})));
        ok(s.call("csv_cancel", &json!({})));
        assert_eq!(s.call("csv_review", &json!({}))["ok"], false);
        ok(s.call("csv_start", &json!({"content": "Titre\nKatana Zero\n"})));
        match_all(&s, 1);
        assert_eq!(ok(s.call("csv_apply", &json!({"choices": []})))["added"], 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn l_export_csv_se_reimporte_par_identifiant() {
        let (a, dir_a) = with_exports("csv-rt-a");
        a.seed_demo().unwrap();
        let csv = read_export(&ok(a.call("export_file", &json!({"format": "csv"}))));
        let (b, dir_b) = with_exports("csv-rt-b");
        let start = ok(b.call("csv_start", &json!({"content": csv})));
        assert_eq!((start["rows"].as_u64(), start["with_ids"].as_u64(), start["with_issues"].as_u64()), (Some(18), Some(18), Some(0)), "{start}");
        let review = match_all(&b, 18);
        assert_eq!(review["counts"]["sure"], 18, "{}", review["counts"]);
        let r = ok(b.call("csv_apply", &json!({"choices": pick(&review, &["sure"])})));
        assert_eq!(r["added"], 18);
        assert_eq!(live_ids(&a), live_ids(&b));
        let (la, lb) = (ok(a.call("library", &json!({"sort": "name"}))), ok(b.call("library", &json!({"sort": "name"}))));
        for (x, y) in la["entries"].as_array().unwrap().iter().zip(lb["entries"].as_array().unwrap()) {
            for k in ["status", "stars", "platform", "minutes", "comment"] {
                // les heures passent par une décimale à deux chiffres : à quelques minutes près
                if k == "minutes" {
                    let d = (x["entry"][k].as_i64().unwrap_or(0) - y["entry"][k].as_i64().unwrap_or(0)).abs();
                    assert!(d <= 1, "{} : {d}", x["name"]);
                } else {
                    assert_eq!(x["entry"][k], y["entry"][k], "{} {k}", x["name"]);
                }
            }
            let day = |v: &Value| v.as_i64().map(|t| t / 86_400);
            assert_eq!(day(&x["entry"]["added"]), day(&y["entry"]["added"]), "{} ajout", x["name"]);
            assert_eq!(day(&x["entry"]["finished"]), day(&y["entry"]["finished"]), "{} fin", x["name"]);
        }
        let _ = std::fs::remove_dir_all(&dir_a);
        let _ = std::fs::remove_dir_all(&dir_b);
    }

    #[test]
    fn import_csv_avec_igdb() {
        // jeton, puis par identifiants (lot), puis une recherche par titre sans identifiant
        let (s, dir, t) = with_igdb("csv-igdb", vec![Ok(TOKEN_OK.into()), Ok(DETAILED.into()), Ok("[]".into()), Ok(DETAILED.into()), Ok(DETAILED.into()), Ok("[]".into())]);
        let csv = "Titre;Année;Statut;Identifiant IGDB\nHades;2020;Terminé;\nDoom;;;\nZzz;;;\nCeleste;;Joué;2\n";
        let start = ok(s.call("csv_start", &json!({"content": csv})));
        assert_eq!((start["mode"].as_str(), start["with_ids"].as_u64()), (Some("igdb"), Some(1)));
        let review = match_all(&s, 4);
        let kinds: Vec<&str> = review["items"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["sure", "check", "notfound", "sure"], "{review}");
        assert_eq!(review["items"][3]["candidates"][0]["name"], "Celeste", "trouvé par son identifiant");
        assert_eq!(review["items"][1]["candidates"].as_array().unwrap().len(), 3, "« Doom » ne correspond exactement à aucun titre : à vérifier");
        let calls = t.calls();
        assert_eq!(calls.len(), 6, "un jeton, un lot d'identifiants et ses durées, trois recherches : {calls:?}");
        assert!(calls[1].2.contains("where id = (2);") && calls[3].2.contains("search \"Hades\""), "{calls:?}");
        // le choix de « Doom » : le troisième candidat (Doom Eternal)
        let doom = review["items"][1]["candidates"].as_array().unwrap().iter().find(|c| c["name"] == "Doom Eternal").unwrap()["id"].clone();
        let mut choices = pick(&review, &["sure"]);
        choices.push(json!({"index": 1, "id": doom}));
        let r = ok(s.call("csv_apply", &json!({"choices": choices})));
        assert_eq!(r["added"], 3);
        assert_eq!(live_ids(&s), [1, 2, 3]);
        assert_eq!(status_of(&s, 1), "completed");
        assert_eq!(status_of(&s, 2), "played");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_csv_erreur_reseau_reprend_ou_on_en_etait() {
        let (s, dir, _t) = with_igdb("csv-neterr", vec![Ok(TOKEN_OK.into()), Err(Error::Network("coupé".into())), Ok(DETAILED.into())]);
        ok(s.call("csv_start", &json!({"content": "Titre\nHades\n"})));
        assert_eq!(s.call("csv_match", &json!({"offset": 0, "limit": 5}))["ok"], false, "l'erreur est remontée");
        let r = ok(s.call("csv_match", &json!({"offset": 0, "limit": 5})));
        assert_eq!((r["done"].as_u64(), r["total"].as_u64()), (Some(1), Some(1)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
