//! Client IGDB (API Twitch/IGDB v4) — recherche de jeux, jaquettes, métadonnées.
//!
//! ÉTAT DE VÉRIFICATION (honnête) : ce module n'a PAS pu être essayé contre le vrai service depuis
//! l'environnement de développement (accès réseau limité). Il est testé avec un transport simulé et
//! avec un vrai serveur HTTP local. Les noms de champs (`first_release_date`, `cover.image_id`,
//! `game_type`…) ont été recoupés avec des définitions de types publiques ; la syntaxe de requête
//! (`search "…"; fields …; limit N;`) et l'authentification (jeton « client credentials » Twitch +
//! en-têtes `Client-ID` / `Authorization: Bearer`) viennent de la documentation IGDB telle que je la connais.
//! Le bouton « Tester la connexion » des réglages sert à le vérifier sur votre machine.
//!
//! Secrets : le secret client n'apparaît ni dans l'URL (envoyé dans le corps du formulaire), ni dans les
//! messages d'erreur (expurgés).

use crate::error::{Error, Result};
use crate::text::url_encode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
pub const GAMES_URL: &str = "https://api.igdb.com/v4/games";
pub const IMAGE_BASE: &str = "https://images.igdb.com/igdb/image/upload";

/// Champs complets : fiche détaillée (studios, éditeurs, modes, thèmes, perspectives, franchise).
const FIELDS_FULL: &str = "name,slug,first_release_date,summary,total_rating,game_type,cover.image_id,platforms.name,platforms.abbreviation,genres.name,\
involved_companies.developer,involved_companies.publisher,involved_companies.company.name,game_modes.name,themes.name,player_perspectives.name,franchises.name";
/// Version précédente, qui a fonctionné en conditions réelles : repli si le service refuse un champ des détails (HTTP 400).
const FIELDS: &str = "name,slug,first_release_date,summary,total_rating,game_type,cover.image_id,platforms.name,platforms.abbreviation,genres.name";
/// Dernier repli : sans `game_type`.
const FIELDS_BASE: &str = "name,slug,first_release_date,summary,total_rating,cover.image_id,platforms.name,platforms.abbreviation,genres.name";

/// Types de jeux IGDB considérés comme « jeux » (les DLC, bundles, mods… passent après).
/// 0 jeu principal, 4 extension autonome, 8 remake, 9 remaster, 10 jeu étendu, 11 portage.
const MAIN_TYPES: [u64; 6] = [0, 4, 8, 9, 10, 11];

pub trait Transport: Send + Sync {
    fn post(&self, url: &str, headers: &[(&str, &str)], body: &str) -> Result<String>;
}

/// Transport HTTPS réel (TLS du système : SChannel sous Windows).
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new() -> Result<Self> {
        let tls = native_tls::TlsConnector::new().map_err(|e| Error::Network(format!("TLS : {e}")))?;
        let agent = ureq::AgentBuilder::new()
            .tls_connector(Arc::new(tls))
            .timeout_connect(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .user_agent("backlog-panda/0.1")
            .build();
        Ok(UreqTransport { agent })
    }
}

impl Transport for UreqTransport {
    fn post(&self, url: &str, headers: &[(&str, &str)], body: &str) -> Result<String> {
        let mut req = self.agent.post(url);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        match req.send_string(body) {
            Ok(resp) => resp.into_string().map_err(|e| Error::Network(format!("lecture de la réponse : {e}"))),
            Err(ureq::Error::Status(code, resp)) => {
                let mut text = resp.into_string().unwrap_or_default();
                text.truncate(300);
                Err(Error::Http(code, text))
            }
            // On n'utilise pas `to_string()` : il contient l'URL.
            Err(ureq::Error::Transport(t)) => {
                Err(Error::Network(format!("{}{}", t.kind(), t.message().map(|m| format!(" ({m})")).unwrap_or_default())))
            }
        }
    }
}

/// Durées « time to beat » d'IGDB, en secondes (`None` = inconnue).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimeToBeat {
    pub hastily: Option<u32>,
    pub normally: Option<u32>,
    pub completely: Option<u32>,
    /// Nombre de joueurs ayant soumis une durée.
    pub count: u32,
}

impl TimeToBeat {
    /// Durée de référence : normale, sinon rapide, sinon complète.
    pub fn reference(&self) -> Option<u32> {
        self.normally.or(self.hastily).or(self.completely)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameInfo {
    pub id: u64,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub year: Option<i32>,
    /// Identifiant d'image IGDB de la jaquette.
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub summary: Option<String>,
    /// Note moyenne IGDB, 0–100.
    #[serde(default)]
    pub rating: Option<f64>,
    /// Faux pour DLC, bundles, mods, etc.
    #[serde(default = "yes")]
    pub main: bool,
    /// Date de première sortie (horodatage Unix, UTC).
    #[serde(default)]
    pub released: Option<i64>,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub publishers: Vec<String>,
    /// Modes de jeu, thèmes et perspectives : noms IGDB (anglais), traduits à l'affichage (`l10n`).
    #[serde(default)]
    pub modes: Vec<String>,
    #[serde(default)]
    pub themes: Vec<String>,
    #[serde(default)]
    pub perspectives: Vec<String>,
    #[serde(default)]
    pub franchise: Option<String>,
    /// Traduction française du résumé (DeepL), mise en cache localement.
    #[serde(default)]
    pub summary_fr: Option<String>,
    /// Niveau de détail de la fiche : 0 = hors ligne, 1 = champs de base, 2 = fiche détaillée,
    /// 3 = fiche détaillée avec durées demandées.
    #[serde(default)]
    pub rev: u8,
    /// Durées de jeu IGDB ; `None` si inconnues ou pas encore demandées.
    #[serde(default)]
    pub time_to_beat: Option<TimeToBeat>,
}

fn yes() -> bool {
    true
}

impl Default for GameInfo {
    fn default() -> Self {
        GameInfo {
            id: 0,
            name: String::new(),
            slug: String::new(),
            year: None,
            cover: None,
            platforms: vec![],
            genres: vec![],
            summary: None,
            rating: None,
            main: true,
            released: None,
            developers: vec![],
            publishers: vec![],
            modes: vec![],
            themes: vec![],
            perspectives: vec![],
            franchise: None,
            summary_fr: None,
            rev: 0,
            time_to_beat: None,
        }
    }
}

/// Niveau de détail d'une fiche reçue avec tous les champs, sans les durées.
pub const REV_DETAILS: u8 = 2;
/// Fiche détaillée dont les durées ont été demandées (présentes ou non).
pub const REV_FULL: u8 = 3;
pub const TTB_URL: &str = "https://api.igdb.com/v4/game_time_to_beats";

impl GameInfo {
    /// `size` : `cover_small`, `cover_big`, `cover_big_2x`… (liste IGDB). Seuls `[a-z0-9_]` sont acceptés.
    pub fn cover_url(&self, size: &str) -> Option<String> {
        let id = self.cover.as_deref()?;
        let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        (ok(id) && ok(size)).then(|| format!("{IMAGE_BASE}/t_{size}/{id}.jpg"))
    }
}

/// (année, mois 1-12, jour 1-31) civils (UTC) d'un horodatage Unix, valable aussi avant 1970.
pub fn ymd_from_unix(secs: i64) -> (i32, u32, u32) {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    ((yoe + era * 400 + i64::from(month <= 2)) as i32, month as u32, day as u32)
}

/// Année civile (UTC) d'un horodatage Unix.
pub fn year_from_unix(secs: i64) -> i32 {
    ymd_from_unix(secs).0
}

/// Entreprises (studio ou éditeur) de `involved_companies`, selon le drapeau `developer` / `publisher`.
fn companies(v: Option<&Value>, flag: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in v.and_then(Value::as_array).into_iter().flatten() {
        if c.get(flag).and_then(Value::as_bool) != Some(true) {
            continue;
        }
        if let Some(n) = c.get("company").and_then(|c| c.get("name")).and_then(Value::as_str) {
            if !out.iter().any(|x| x == n) {
                out.push(n.to_string());
            }
        }
    }
    out
}

fn named(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    x.get("abbreviation")
                        .and_then(Value::as_str)
                        .or_else(|| x.get("name").and_then(Value::as_str))
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn genre_names(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Lit la réponse JSON de `POST /v4/games`. Les entrées sans `id` ou sans `name` sont ignorées.
pub fn parse_games(json: &str) -> Result<Vec<GameInfo>> {
    let root: Value = serde_json::from_str(json)?;
    let items = root.as_array().ok_or_else(|| Error::Parse("réponse IGDB : tableau attendu".into()))?;
    let mut out = Vec::new();
    for g in items {
        let (Some(id), Some(name)) = (g.get("id").and_then(Value::as_u64), g.get("name").and_then(Value::as_str)) else {
            continue;
        };
        let game_type = g.get("game_type").and_then(|t| t.as_u64().or_else(|| t.get("id").and_then(Value::as_u64)));
        out.push(GameInfo {
            id,
            name: name.to_string(),
            slug: g.get("slug").and_then(Value::as_str).unwrap_or_default().to_string(),
            year: g.get("first_release_date").and_then(Value::as_i64).map(year_from_unix),
            cover: g.get("cover").and_then(|c| c.get("image_id")).and_then(Value::as_str).map(str::to_string),
            platforms: g.get("platforms").map(named).unwrap_or_default(),
            genres: g.get("genres").map(genre_names).unwrap_or_default(),
            summary: g.get("summary").and_then(Value::as_str).map(str::to_string),
            rating: g.get("total_rating").and_then(Value::as_f64),
            main: game_type.is_none_or(|t| MAIN_TYPES.contains(&t)),
            released: g.get("first_release_date").and_then(Value::as_i64),
            developers: companies(g.get("involved_companies"), "developer"),
            publishers: companies(g.get("involved_companies"), "publisher"),
            modes: g.get("game_modes").map(genre_names).unwrap_or_default(),
            themes: g.get("themes").map(genre_names).unwrap_or_default(),
            perspectives: g.get("player_perspectives").map(genre_names).unwrap_or_default(),
            franchise: g.get("franchises").map(genre_names).and_then(|f| f.into_iter().next()),
            summary_fr: None,
            rev: 0,
            time_to_beat: None,
        });
    }
    Ok(out)
}

/// Lit la réponse JSON de `POST /v4/game_time_to_beats`, indexée par `game_id`.
/// Valeurs nulles ou absentes → `None` ; entrées sans `game_id` ignorées ; doublon : la première gagne.
pub fn parse_time_to_beats(json: &str) -> Result<HashMap<u64, TimeToBeat>> {
    let root: Value = serde_json::from_str(json)?;
    let items = root.as_array().ok_or_else(|| Error::Parse("réponse IGDB : tableau attendu".into()))?;
    let secs = |e: &Value, k: &str| e.get(k).and_then(Value::as_u64).filter(|&v| v > 0).map(|v| v.min(u32::MAX as u64) as u32);
    let mut out = HashMap::new();
    for e in items {
        let Some(game_id) = e.get("game_id").and_then(Value::as_u64) else { continue };
        out.entry(game_id).or_insert_with(|| TimeToBeat {
            hastily: secs(e, "hastily"),
            normally: secs(e, "normally"),
            completely: secs(e, "completely"),
            count: e.get("count").and_then(Value::as_u64).map_or(0, |c| c.min(u32::MAX as u64) as u32),
        });
    }
    Ok(out)
}

/// Échappe une chaîne pour une requête Apicalypse (guillemets, barres obliques, retours à la ligne).
pub fn apicalypse_string(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            c => vec![c],
        })
        .collect()
}

struct Token {
    value: String,
    expires: Instant,
}

pub struct IgdbClient {
    transport: Arc<dyn Transport>,
    client_id: String,
    client_secret: String,
    token: Mutex<Option<Token>>,
}

impl IgdbClient {
    pub fn new(transport: Arc<dyn Transport>, client_id: &str, client_secret: &str) -> Self {
        IgdbClient {
            transport,
            client_id: client_id.trim().to_string(),
            client_secret: client_secret.trim().to_string(),
            token: Mutex::new(None),
        }
    }

    /// Retire secret et jeton de tout message d'erreur.
    fn scrub(&self, e: Error) -> Error {
        let clean = |s: String| {
            let mut s = s;
            if !self.client_secret.is_empty() {
                s = s.replace(&self.client_secret, "***");
            }
            // `try_lock` : on peut être appelé pendant que `token()` détient déjà le verrou (pas de verrou réentrant).
            if let Some(t) = self.token.try_lock().ok().and_then(|t| t.as_ref().map(|t| t.value.clone())) {
                s = s.replace(&t, "***");
            }
            s
        };
        match e {
            Error::Http(c, b) => Error::Http(c, clean(b)),
            Error::Network(m) => Error::Network(clean(m)),
            Error::Parse(m) => Error::Parse(clean(m)),
            other => other,
        }
    }

    fn token(&self, force: bool) -> Result<String> {
        let mut guard = self.token.lock().map_err(|_| Error::Network("verrou empoisonné".into()))?;
        if !force {
            if let Some(t) = guard.as_ref().filter(|t| t.expires > Instant::now()) {
                return Ok(t.value.clone());
            }
        }
        let body = format!(
            "client_id={}&client_secret={}&grant_type=client_credentials",
            url_encode(&self.client_id),
            url_encode(&self.client_secret)
        );
        let resp = self
            .transport
            .post(TOKEN_URL, &[("Content-Type", "application/x-www-form-urlencoded"), ("Accept", "application/json")], &body)
            .map_err(|e| self.scrub(e))?;
        let v: Value = serde_json::from_str(&resp)?;
        let value = v
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Parse("jeton Twitch absent de la réponse".into()))?
            .to_string();
        let ttl = v.get("expires_in").and_then(Value::as_u64).unwrap_or(3_600).saturating_sub(120).max(30);
        *guard = Some(Token { value: value.clone(), expires: Instant::now() + Duration::from_secs(ttl) });
        Ok(value)
    }

    fn post_games(&self, apicalypse: &str) -> Result<String> {
        self.post_to(GAMES_URL, apicalypse)
    }

    fn post_to(&self, url: &str, apicalypse: &str) -> Result<String> {
        if self.client_id.is_empty() || self.client_secret.is_empty() {
            return Err(Error::NotConfigured);
        }
        let mut forced = false;
        loop {
            let token = self.token(forced)?;
            let auth = format!("Bearer {token}");
            let result = self.transport.post(
                url,
                &[("Client-ID", &self.client_id), ("Authorization", &auth), ("Accept", "application/json"), ("Content-Type", "text/plain")],
                apicalypse,
            );
            match result {
                Err(Error::Http(401, _)) if !forced => forced = true, // jeton révoqué ou expiré : un seul renouvellement
                Err(e) => return Err(self.scrub(e)),
                Ok(body) => return Ok(body),
            }
        }
    }

    /// Exécute la requête avec tous les champs ; si le service la refuse (400), retente avec la liste
    /// précédente (qui a fonctionné en conditions réelles), puis sans `game_type`.
    /// `rev` des fiches reçues : 2 = détaillées (sans durées), 1 = champs de base seulement (à compléter plus tard).
    fn run(&self, build: impl Fn(&str) -> String) -> Result<Vec<GameInfo>> {
        let tiers = [(FIELDS_FULL, REV_DETAILS), (FIELDS, 1), (FIELDS_BASE, 1)];
        for (i, (fields, rev)) in tiers.iter().enumerate() {
            match self.post_games(&build(fields)) {
                Err(Error::Http(400, _)) if i + 1 < tiers.len() => continue,
                other => {
                    let mut games = parse_games(&other?)?;
                    games.iter_mut().for_each(|g| g.rev = *rev);
                    return Ok(games);
                }
            }
        }
        unreachable!("la dernière variante retourne toujours")
    }

    /// Recherche par texte ; les jeux « principaux » d'abord, l'ordre de pertinence IGDB conservé à l'intérieur.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<GameInfo>> {
        let q = apicalypse_string(query.trim());
        let mut games = self.run(|fields| format!("search \"{q}\"; fields {fields}; limit {limit};"))?;
        games.sort_by_key(|g| !g.main); // tri stable
        Ok(games)
    }

    pub fn by_ids(&self, ids: &[u64]) -> Result<Vec<GameInfo>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let list = ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
        let mut games = self.run(|fields| format!("fields {fields}; where id = ({list}); limit {};", ids.len().min(500)))?;
        // Durées : seulement pour les fiches détaillées ; en cas d'échec, on garde les fiches telles quelles (rev 2)
        // pour que la prochaine synchronisation réessaie.
        if !games.is_empty() && games.iter().all(|g| g.rev == REV_DETAILS) {
            let q = format!("fields game_id,hastily,normally,completely,count; where game_id = ({list}); limit 500;");
            let mut res = self.post_to(TTB_URL, &q).and_then(|b| parse_time_to_beats(&b));
            if matches!(res, Err(Error::Http(429, _))) {
                // limite de débit : une seule nouvelle tentative, après une pause
                if !cfg!(test) {
                    std::thread::sleep(Duration::from_secs(1));
                }
                res = self.post_to(TTB_URL, &q).and_then(|b| parse_time_to_beats(&b));
            }
            match res {
                Ok(mut ttb) => {
                    for g in &mut games {
                        g.time_to_beat = ttb.remove(&g.id);
                        g.rev = REV_FULL;
                    }
                }
                Err(e) => eprintln!("Durées de jeu IGDB indisponibles, nouvel essai à la prochaine synchronisation : {e}"),
            }
        }
        Ok(games)
    }

    /// Vérifie clés et réseau (jeton + une requête minimale).
    pub fn check(&self) -> Result<()> {
        self.post_games("fields name; limit 1;").map(|_| ())
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::collections::VecDeque;

    pub type Call = (String, Vec<(String, String)>, String);

    #[derive(Default)]
    pub struct FakeTransport {
        pub calls: Mutex<Vec<Call>>,
        pub responses: Mutex<VecDeque<Result<String>>>,
    }

    impl FakeTransport {
        pub fn with(responses: Vec<Result<String>>) -> Arc<Self> {
            Arc::new(FakeTransport { calls: Mutex::new(vec![]), responses: Mutex::new(responses.into()) })
        }
        pub fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Transport for FakeTransport {
        fn post(&self, url: &str, headers: &[(&str, &str)], body: &str) -> Result<String> {
            self.calls.lock().unwrap().push((
                url.to_string(),
                headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                body.to_string(),
            ));
            self.responses.lock().unwrap().pop_front().unwrap_or_else(|| Err(Error::Network("plus de réponse simulée".into())))
        }
    }

    pub const TOKEN_OK: &str = r#"{"access_token":"tok123","expires_in":5000000,"token_type":"bearer"}"#;

    /// Réponse IGDB SYNTHÉTIQUE (écrite à la main d'après le format documenté, non capturée sur le service).
    pub const GAMES_SAMPLE: &str = r#"[
      {"id":7346,"name":"The Legend of Zelda: Breath of the Wild","slug":"the-legend-of-zelda-breath-of-the-wild",
       "first_release_date":1488499200,"summary":"Explore Hyrule.","total_rating":92.5,"game_type":0,
       "cover":{"id":1,"image_id":"co3p2d"},
       "platforms":[{"id":130,"name":"Nintendo Switch","abbreviation":"Switch"},{"id":41,"name":"Wii U"}],
       "genres":[{"id":12,"name":"Role-playing (RPG)"},{"id":31,"name":"Adventure"}]},
      {"id":41826,"name":"Breath of the Wild - The Champions' Ballad","slug":"champions-ballad","game_type":1},
      {"id":113112,"name":"Hades","slug":"hades--1","first_release_date":1600300800,"game_type":{"id":0,"type":"Main Game"}},
      {"name":"sans identifiant"},
      {"id":5,"slug":"sans-nom"}
    ]"#;
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    #[test]
    fn annees_unix() {
        assert_eq!(year_from_unix(0), 1970);
        assert_eq!(year_from_unix(-1), 1969);
        assert_eq!(year_from_unix(1_488_499_200), 2017);
        assert_eq!(year_from_unix(1_704_067_199), 2023);
        assert_eq!(year_from_unix(1_704_067_200), 2024);
        assert_eq!(year_from_unix(-315_619_200), 1960);
    }

    #[test]
    fn lecture_de_la_reponse() {
        let g = parse_games(GAMES_SAMPLE).unwrap();
        assert_eq!(g.len(), 3, "les deux entrées incomplètes sont ignorées");
        assert_eq!(g[0].year, Some(2017));
        assert_eq!(g[0].cover.as_deref(), Some("co3p2d"));
        assert_eq!(g[0].platforms, vec!["Switch", "Wii U"]);
        assert_eq!(g[0].genres.len(), 2);
        assert!(g[0].main);
        assert!(!g[1].main, "type 1 = DLC");
        assert!(g[2].main, "type donné sous forme d'objet");
        assert_eq!(g[1].year, None);
        assert_eq!(
            g[0].cover_url("cover_big").unwrap(),
            "https://images.igdb.com/igdb/image/upload/t_cover_big/co3p2d.jpg"
        );
        assert_eq!(g[1].cover_url("cover_big"), None);
        assert!(parse_games("{}").is_err());
        assert!(parse_games("[]").unwrap().is_empty());
    }

    #[test]
    fn url_de_jaquette_refuse_les_caracteres_dangereux() {
        let mut g = parse_games(GAMES_SAMPLE).unwrap().remove(0);
        g.cover = Some("a/../b\"><script>".into());
        assert_eq!(g.cover_url("cover_big"), None);
        g.cover = Some("co1".into());
        assert_eq!(g.cover_url("x/y"), None);
    }

    #[test]
    fn echappement_apicalypse() {
        assert_eq!(apicalypse_string(r#"say "hi" \o/"#), r#"say \"hi\" \\o/"#);
        assert_eq!(apicalypse_string("a\nb\r\tc"), "abc");
    }

    #[test]
    fn recherche_envoie_jeton_puis_requete_et_trie_les_jeux_principaux() {
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(GAMES_SAMPLE.into())]);
        let c = IgdbClient::new(t.clone(), " cid ", " sec ");
        let games = c.search("zelda \"x\"", 24).unwrap();
        // DLC repoussé à la fin, ordre relatif conservé
        assert_eq!(games.iter().map(|g| g.id).collect::<Vec<_>>(), vec![7346, 113112, 41826]);
        let calls = t.calls();
        assert_eq!(calls[0].0, TOKEN_URL);
        assert_eq!(calls[0].2, "client_id=cid&client_secret=sec&grant_type=client_credentials");
        assert!(!calls[0].0.contains("sec"), "le secret ne doit pas figurer dans l'URL");
        assert_eq!(calls[1].0, GAMES_URL);
        assert!(calls[1].1.contains(&("Client-ID".into(), "cid".into())));
        assert!(calls[1].1.contains(&("Authorization".into(), "Bearer tok123".into())));
        assert!(calls[1].2.starts_with("search \"zelda \\\"x\\\"\"; fields name,slug,"));
        assert!(calls[1].2.ends_with("limit 24;"));
    }

    #[test]
    fn le_jeton_est_reutilise() {
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok("[]".into()), Ok("[]".into())]);
        let c = IgdbClient::new(t.clone(), "a", "b");
        c.search("aa", 5).unwrap();
        c.search("bb", 5).unwrap();
        assert_eq!(t.calls().iter().filter(|c| c.0 == TOKEN_URL).count(), 1);
    }

    #[test]
    fn jeton_renouvele_une_fois_sur_401() {
        let t = FakeTransport::with(vec![
            Ok(TOKEN_OK.into()),
            Err(Error::Http(401, "expired".into())),
            Ok(r#"{"access_token":"tok456","expires_in":5000000}"#.into()),
            Ok("[]".into()),
        ]);
        let c = IgdbClient::new(t.clone(), "a", "b");
        assert!(c.search("aa", 5).unwrap().is_empty());
        let calls = t.calls();
        assert_eq!(calls.len(), 4);
        assert!(calls[3].1.contains(&("Authorization".into(), "Bearer tok456".into())));
    }

    #[test]
    fn deux_401_de_suite_sont_une_erreur() {
        let t = FakeTransport::with(vec![
            Ok(TOKEN_OK.into()),
            Err(Error::Http(401, "x".into())),
            Ok(TOKEN_OK.into()),
            Err(Error::Http(401, "x".into())),
        ]);
        assert_eq!(IgdbClient::new(t, "a", "b").search("aa", 5), Err(Error::Http(401, "x".into())));
    }

    #[test]
    fn repli_progressif_si_400() {
        // champs détaillés refusés -> liste précédente (avec game_type) -> sans game_type
        let t = FakeTransport::with(vec![
            Ok(TOKEN_OK.into()),
            Err(Error::Http(400, "bad field".into())),
            Err(Error::Http(400, "bad field".into())),
            Ok(GAMES_SAMPLE.into()),
        ]);
        let c = IgdbClient::new(t.clone(), "a", "b");
        let games = c.search("zelda", 5).unwrap();
        assert_eq!(games.len(), 3);
        assert!(games.iter().all(|g| g.rev == 1), "fiches de base : à compléter plus tard");
        let calls = t.calls();
        assert!(calls[1].2.contains("involved_companies") && calls[1].2.contains("game_type"));
        assert!(!calls[2].2.contains("involved_companies") && calls[2].2.contains("game_type"));
        assert!(!calls[3].2.contains("game_type"));
        // si les champs détaillés passent du premier coup : fiche complète
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(GAMES_SAMPLE.into())]);
        let c = IgdbClient::new(t, "a", "b");
        assert!(c.search("zelda", 5).unwrap().iter().all(|g| g.rev == REV_DETAILS));
    }

    #[test]
    fn details_studios_editeurs_modes_themes() {
        let json = r#"[{"id":1,"name":"Hades","first_release_date":1600300800,
          "involved_companies":[
            {"id":1,"developer":true,"publisher":false,"company":{"id":9,"name":"Supergiant Games"}},
            {"id":2,"developer":false,"publisher":true,"company":{"id":9,"name":"Supergiant Games"}},
            {"id":3,"developer":false,"publisher":true,"company":{"id":10,"name":"Private Division"}},
            {"id":4,"developer":true,"publisher":false}],
          "game_modes":[{"name":"Single player"}],"themes":[{"name":"Fantasy"},{"name":"Action"}],
          "player_perspectives":[{"name":"Isometric"}],"franchises":[{"name":"Hades"},{"name":"Autre"}]}]"#;
        let g = parse_games(json).unwrap().remove(0);
        assert_eq!(g.developers, vec!["Supergiant Games"]);
        assert_eq!(g.publishers, vec!["Supergiant Games", "Private Division"]);
        assert_eq!((g.modes.len(), g.themes.len(), g.perspectives.len()), (1, 2, 1));
        assert_eq!(g.franchise.as_deref(), Some("Hades"));
        assert_eq!(g.released, Some(1_600_300_800));
        // données stockées avant l'ajout des détails : lisibles, avec valeurs par défaut
        let old: GameInfo = serde_json::from_str(r#"{"id":1,"name":"X","slug":"x"}"#).unwrap();
        assert_eq!((old.rev, old.developers.len(), old.main), (0, 0, true));
    }

    #[test]
    fn dates_civiles() {
        assert_eq!(ymd_from_unix(0), (1970, 1, 1));
        assert_eq!(ymd_from_unix(1_488_499_200), (2017, 3, 3));
        assert_eq!(ymd_from_unix(1_704_067_199), (2023, 12, 31));
        assert_eq!(ymd_from_unix(951_782_400), (2000, 2, 29));
        assert_eq!(ymd_from_unix(-1), (1969, 12, 31));
    }

    #[test]
    fn sans_cles_on_ne_contacte_rien() {
        let t = FakeTransport::with(vec![]);
        assert_eq!(IgdbClient::new(t.clone(), "", "").search("zelda", 5), Err(Error::NotConfigured));
        assert!(t.calls().is_empty());
    }

    #[test]
    fn le_secret_est_expurge_des_erreurs() {
        let t = FakeTransport::with(vec![Err(Error::Network("echec avec s3cr3t dans le message".into()))]);
        let c = IgdbClient::new(t, "id", "s3cr3t");
        let e = c.search("zelda", 5).unwrap_err();
        assert!(!e.to_string().contains("s3cr3t"), "{e}");
        assert!(e.to_string().contains("***"));
    }

    #[test]
    fn by_ids_construit_la_clause_where() {
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok("[]".into())]);
        let c = IgdbClient::new(t.clone(), "a", "b");
        assert!(c.by_ids(&[]).unwrap().is_empty());
        c.by_ids(&[7, 9]).unwrap();
        let body = &t.calls()[1].2;
        assert!(body.contains("where id = (7,9);") && body.ends_with("limit 2;"), "{body}");
    }

    #[test]
    fn lecture_durees_zero_et_champs_absents() {
        let m = parse_time_to_beats(r#"[{"id":1,"game_id":10,"hastily":3600,"normally":0,"count":4},
                                         {"id":2,"game_id":11},{"id":3}]"#).unwrap();
        assert_eq!(m[&10], TimeToBeat { hastily: Some(3600), normally: None, completely: None, count: 4 });
        assert_eq!(m[&10].reference(), Some(3600), "normal absent : on prend rapide");
        assert_eq!(m[&11], TimeToBeat::default());
        assert_eq!(m.len(), 2, "entrée sans game_id ignorée");
    }

    #[test]
    fn by_ids_associe_les_durees_par_game_id() {
        let ttb = r#"[{"game_id":7346,"hastily":180000,"normally":330000,"completely":680000,"count":312},
                      {"game_id":7346,"normally":1,"count":1}]"#;
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(GAMES_SAMPLE.into()), Ok(ttb.into())]);
        let c = IgdbClient::new(t.clone(), "id", "s3cr3t");
        let games = c.by_ids(&[7346, 41826, 113112]).unwrap();
        let calls = t.calls();
        assert_eq!(calls[2].0, TTB_URL);
        assert!(calls[2].2.contains("where game_id = (7346,41826,113112)"));
        let zelda = games.iter().find(|g| g.id == 7346).unwrap();
        assert_eq!(zelda.time_to_beat.as_ref().unwrap().normally, Some(330000), "la première entrée gagne");
        assert!(games.iter().all(|g| g.rev == REV_FULL));
        assert!(games.iter().find(|g| g.id == 113112).unwrap().time_to_beat.is_none());
    }

    #[test]
    fn by_ids_sans_durees_si_la_requete_echoue() {
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(GAMES_SAMPLE.into()), Err(Error::Http(429, "trop".into()))]);
        let games = IgdbClient::new(t, "id", "s").by_ids(&[7346]).unwrap();
        assert!(!games.is_empty());
        assert!(games.iter().all(|g| g.rev == REV_DETAILS && g.time_to_beat.is_none()));
    }

    #[test]
    fn by_ids_reessaie_une_fois_sur_429() {
        let ttb = r#"[{"game_id":7346,"normally":330000,"count":9}]"#;
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(GAMES_SAMPLE.into()), Err(Error::Http(429, "trop".into())), Ok(ttb.into())]);
        let games = IgdbClient::new(t.clone(), "id", "s").by_ids(&[7346]).unwrap();
        assert_eq!(t.calls().len(), 4, "jeton, fiches, durées 429, durées ok");
        assert!(games.iter().all(|g| g.rev == REV_FULL));
        assert_eq!(games.iter().find(|g| g.id == 7346).unwrap().time_to_beat.as_ref().unwrap().normally, Some(330000));
    }

    #[test]
    fn by_ids_abandonne_apres_deux_429_sans_erreur() {
        let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(GAMES_SAMPLE.into()), Err(Error::Http(429, "a".into())), Err(Error::Http(429, "b".into()))]);
        let games = IgdbClient::new(t.clone(), "id", "s").by_ids(&[7346]).unwrap();
        assert_eq!(t.calls().len(), 4);
        assert!(!games.is_empty() && games.iter().all(|g| g.rev == REV_DETAILS && g.time_to_beat.is_none()));
    }

    #[test]
    fn ancienne_fiche_sans_durees() {
        let g: GameInfo = serde_json::from_str(r#"{"id":1,"name":"Hades","slug":"hades","rev":2}"#).unwrap();
        assert_eq!((g.rev, g.time_to_beat), (2, None));
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serveur HTTP minimal : répond `status`/`body` à `n` requêtes et renvoie ce qu'il a reçu.
    fn one_shot(status: &str, body: &'static str) -> (u16, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let status = status.to_string();
        let handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            // lit jusqu'à la fin des en-têtes puis du corps annoncé
            loop {
                let n = s.read(&mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).into_owned();
                if let Some(i) = text.find("\r\n\r\n") {
                    let len = text[..i]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if buf.len() >= i + 4 + len || n == 0 {
                        break;
                    }
                }
            }
            write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        });
        (port, handle)
    }

    #[test]
    fn transport_reel_envoie_en_tetes_et_corps() {
        let (port, server) = one_shot("200 OK", "[{\"id\":1}]");
        let t = UreqTransport::new().unwrap();
        let out = t.post(&format!("http://127.0.0.1:{port}/v4/games"), &[("Client-ID", "abc"), ("Authorization", "Bearer tok")], "search \"zelda\"; limit 5;").unwrap();
        assert_eq!(out, "[{\"id\":1}]");
        let received = server.join().unwrap();
        assert!(received.starts_with("POST /v4/games HTTP/1.1"), "{received}");
        let lower = received.to_ascii_lowercase();
        assert!(lower.contains("client-id: abc") && lower.contains("authorization: bearer tok"), "{received}");
        assert!(received.ends_with("search \"zelda\"; limit 5;"));
    }

    #[test]
    fn transport_reel_erreur_http_et_reseau_sans_fuite_d_url() {
        let (port, server) = one_shot("401 Unauthorized", "{\"message\":\"Invalid token\"}");
        let t = UreqTransport::new().unwrap();
        let err = t.post(&format!("http://127.0.0.1:{port}/v4/games?secret=zzz"), &[], "x").unwrap_err();
        server.join().unwrap();
        assert!(matches!(&err, Error::Http(401, m) if m.contains("Invalid token")), "{err:?}");
        // port fermé : erreur réseau, et le message n'embarque ni l'URL ni la requête
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let err = t.post(&format!("http://127.0.0.1:{port}/v4/games?secret=zzz"), &[], "x").unwrap_err();
        assert!(matches!(err, Error::Network(_)));
        assert!(!err.to_string().contains("secret=zzz"), "{err}");
    }
}
