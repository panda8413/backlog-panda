//! Lecture d'un tableur CSV de jeux (export maison, Excel, LibreOffice, Google Sheets…).
//!
//! Ce module ne fait que lire et comprendre le fichier : colonnes reconnues par leur intitulé
//! (français ou anglais), statuts, notes, durées et dates. Le rapprochement des titres avec de vrais
//! jeux (IGDB ou catalogue intégré) est fait par `Service`, qui a le réseau.

use crate::error::{Error, Result};
use crate::l10n::parse_duration;
use crate::library::Status;
use crate::rating::Rating;
use serde::Serialize;

/// Au-delà, le rapprochement titre par titre deviendrait interminable : on demande de découper le fichier.
pub const MAX_ROWS: usize = 3000;
const PLATFORM_MAX: usize = 60;
const COMMENT_MAX: usize = 10_000;
const DATE_MIN: i64 = 315_532_800; // 1980-01-01

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Title,
    Year,
    Status,
    Rating,
    Platform,
    Hours,
    Comment,
    Added,
    Finished,
    IgdbId,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Title => "Titre",
            Role::Year => "Année",
            Role::Status => "Statut",
            Role::Rating => "Note",
            Role::Platform => "Plateforme",
            Role::Hours => "Temps de jeu",
            Role::Comment => "Commentaire",
            Role::Added => "Date d'ajout",
            Role::Finished => "Date de fin",
            Role::IgdbId => "Identifiant IGDB",
        }
    }
}

/// Une ligne du fichier, déjà interprétée.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CsvRow {
    /// Numéro de ligne dans le fichier (l'en-tête est la ligne 1).
    pub line: usize,
    pub title: String,
    pub year: Option<i32>,
    pub status: Status,
    pub rating_half: Option<u8>,
    pub platform: Option<String>,
    pub minutes: Option<u32>,
    pub comment: Option<String>,
    pub added: Option<i64>,
    pub finished: Option<i64>,
    pub igdb_id: Option<u64>,
    /// Ce qui n'a pas pu être lu dans cette ligne (la ligne reste importable sans la valeur concernée).
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ColumnInfo {
    pub header: String,
    /// Libellé du rôle reconnu, ou `None` si la colonne est ignorée.
    pub role: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub rows: Vec<CsvRow>,
    pub columns: Vec<ColumnInfo>,
    pub delimiter: char,
    /// Lignes vides ou sans titre, écartées.
    pub blank: usize,
}

/// Minuscules, sans accents, lettres et chiffres seulement : « Terminé le » donne `terminele`.
pub fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' | 'À' | 'Â' | 'Ä' | 'Á' | 'Ã' | 'Å' => 'a',
            'ç' | 'Ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' | 'Î' | 'Ï' | 'Í' | 'Ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' | 'Ô' | 'Ö' | 'Ó' | 'Ò' | 'Õ' => 'o',
            'û' | 'ü' | 'ù' | 'ú' | 'Û' | 'Ü' | 'Ù' | 'Ú' => 'u',
            'ÿ' | 'ý' => 'y',
            'ñ' | 'Ñ' => 'n',
            'œ' | 'Œ' => 'o',
            c => c,
        })
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn role_of(header: &str) -> Option<Role> {
    let k = fold(header);
    let starts = |names: &[&str]| names.iter().any(|n| k.starts_with(n));
    if k == "notes" {
        return Some(Role::Comment); // « Notes » seul désigne presque toujours des remarques, pas une note chiffrée
    }
    if k == "id" || starts(&["identifiantigdb", "igdbid", "igdb", "idigdb"]) {
        Some(Role::IgdbId)
    } else if starts(&["titre", "title", "nom", "name", "jeu", "game"]) {
        Some(Role::Title)
    } else if starts(&["annee", "year"]) {
        Some(Role::Year)
    } else if starts(&["statut", "status", "etat"]) {
        Some(Role::Status)
    } else if starts(&["terminele", "finile", "datedefin", "datefin", "finished", "completedon", "datecompleted", "enddate"]) {
        Some(Role::Finished)
    } else if starts(&["ajout", "datedajout", "dateajout", "datedeajout", "added", "dateadded"]) {
        Some(Role::Added)
    } else if starts(&["note", "rating", "score", "manote", "mynote"]) {
        Some(Role::Rating)
    } else if starts(&["plateforme", "platform", "console", "support"]) {
        Some(Role::Platform)
    } else if starts(&["heures", "hours", "tempsdejeu", "temps", "playtime", "duree"]) {
        Some(Role::Hours)
    } else if starts(&["commentaire", "comment", "avis", "review", "remarque", "impression"]) {
        Some(Role::Comment)
    } else {
        None
    }
}

/// Échelle de notation annoncée dans l'intitulé (« Note /10 », « Score sur 100 »), 5 par défaut.
fn scale_hint(header: &str) -> Option<f64> {
    let h = header.to_lowercase().replace(' ', "");
    if h.contains("/100") || h.contains("sur100") {
        Some(100.0)
    } else if h.contains("/10") || h.contains("sur10") {
        Some(10.0)
    } else if h.contains("/5") || h.contains("sur5") {
        Some(5.0)
    } else {
        None
    }
}

// ---------------------------------------------------------------- découpage CSV

/// Séparateur le plus fréquent (hors guillemets) dans la première ligne : `;`, `,` ou tabulation.
fn detect_delimiter(text: &str) -> char {
    let (mut quoted, mut counts) = (false, [0usize; 3]);
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '\n' | '\r' if !quoted => break,
            ';' if !quoted => counts[0] += 1,
            ',' if !quoted => counts[1] += 1,
            '\t' if !quoted => counts[2] += 1,
            _ => {}
        }
    }
    let best = (0..3).max_by_key(|i| (counts[*i], std::cmp::Reverse(*i))).unwrap_or(0);
    if counts[best] == 0 {
        ';'
    } else {
        [';', ',', '\t'][best]
    }
}

/// Découpe en enregistrements (guillemets, `""`, retours à la ligne dans une cellule, CRLF).
fn split_records(text: &str, delim: char) -> Vec<Vec<String>> {
    let (mut records, mut rec, mut cell) = (Vec::new(), Vec::new(), String::new());
    let (mut quoted, mut was_quoted) = (false, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if cell.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
            }
            c if c == delim && !quoted => {
                rec.push(std::mem::take(&mut cell));
                was_quoted = false;
            }
            '\n' | '\r' if !quoted => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                rec.push(std::mem::take(&mut cell));
                was_quoted = false;
                records.push(std::mem::take(&mut rec));
            }
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !rec.is_empty() || was_quoted {
        rec.push(cell);
        records.push(rec);
    }
    records
}

// ---------------------------------------------------------------- valeurs

/// Retire l'apostrophe que l'export ajoute devant `=`, `+`, `-`, `@` pour neutraliser les formules d'Excel.
fn unguard(cell: &str) -> &str {
    match cell.strip_prefix('\'') {
        Some(rest) if rest.starts_with(['=', '+', '-', '@']) => rest,
        _ => cell,
    }
}

fn parse_status(v: &str) -> Option<Status> {
    Some(match fold(v).as_str() {
        "termine" | "terminee" | "fini" | "finie" | "finished" | "completed" | "complete" | "beaten" | "done" => Status::Completed,
        "encours" | "playing" | "current" | "currentlyplaying" | "enjeu" => Status::Playing,
        "backlog" | "afaire" | "unplayed" | "toplay" | "aplay" | "pile" => Status::Backlog,
        "joue" | "jouee" | "played" => Status::Played,
        "abandonne" | "abandonnee" | "abandoned" | "dropped" | "arrete" => Status::Abandoned,
        "envie" | "wishlist" | "wishlisted" | "souhait" | "souhaits" | "wanted" | "listedenvies" => Status::Wishlist,
        _ => return None,
    })
}

fn parse_year(v: &str) -> Option<i32> {
    let digits: Vec<char> = v.chars().collect();
    digits.windows(4).find_map(|w| {
        let s: String = w.iter().collect();
        s.chars().all(|c| c.is_ascii_digit()).then(|| s.parse::<i32>().ok()).flatten().filter(|y| (1950..=2100).contains(y))
    })
}

/// Demi-étoiles (1–10) depuis une note : `4,5`, `9/10`, `★★★★½`. `0` ou vide : pas de note.
fn parse_rating(v: &str, scale: Option<f64>) -> std::result::Result<Option<u8>, String> {
    let t = v.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let stars_text = t.chars().filter(|c| *c == '★').count() as f64 + if t.contains('½') { 0.5 } else { 0.0 };
    let (value, scale) = if stars_text > 0.0 && t.chars().all(|c| matches!(c, '★' | '☆' | '½' | ' ')) {
        (stars_text, 5.0)
    } else if let Some((a, b)) = t.split_once('/') {
        let num = |s: &str| s.trim().replace(',', ".").parse::<f64>().ok();
        match (num(a), num(b).filter(|b| *b > 0.0)) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(format!("note illisible : « {t} »")),
        }
    } else {
        let n = t.replace(',', ".").parse::<f64>().map_err(|_| format!("note illisible : « {t} »"))?;
        let s = scale.unwrap_or(if n <= 5.0 { 5.0 } else if n <= 10.0 { 10.0 } else { 100.0 });
        (n, s)
    };
    if !value.is_finite() || value < 0.0 || value > scale {
        return Err(format!("note hors échelle : « {t} »"));
    }
    let stars = ((value * 5.0 / scale) * 2.0).round() / 2.0;
    Ok(if stars < 0.5 { None } else { Rating::from_stars(stars).ok().map(Rating::half_stars) })
}

/// `2026-10-17`, `17/10/2026`, `17-10-2026`, `17.10.2026` (jour avant mois) → secondes Unix, à midi UTC.
fn parse_date(v: &str, now: i64) -> std::result::Result<Option<i64>, String> {
    let t = v.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let t = t.split([' ', 'T']).next().unwrap_or(t);
    let parts: Vec<&str> = t.split(['-', '/', '.']).collect();
    let n = |s: &str| s.parse::<i64>().ok();
    let ymd = match parts.as_slice() {
        [a, b, c] if a.len() == 4 => n(a).zip(n(b)).zip(n(c)).map(|((y, m), d)| (y, m, d)),
        [a, b, c] if c.len() == 4 => n(c).zip(n(b)).zip(n(a)).map(|((y, m), d)| (y, m, d)),
        _ => None,
    };
    let bad = || format!("date illisible : « {t} » (attendu : 2026-10-17 ou 17/10/2026)");
    let (y, m, d) = ymd.filter(|(y, m, d)| (1..=12).contains(m) && (1..=days_in_month(*y, *m)).contains(d)).ok_or_else(bad)?;
    let secs = crate::store::days_from_civil(y, m, d) * 86_400 + 43_200;
    if !(DATE_MIN..=now + 2 * 86_400).contains(&secs) {
        return Err(format!("date hors limites : « {t} » (entre 1980 et aujourd'hui)"));
    }
    Ok(Some(secs))
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn clip(s: &str, max: usize) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.chars().take(max).collect())
}

// ---------------------------------------------------------------- analyse

pub fn parse(text: &str, now: i64) -> Result<Parsed> {
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Err(Error::Invalid("le fichier est vide".into()));
    }
    let delimiter = detect_delimiter(text);
    let mut records = split_records(text, delimiter).into_iter();
    let header = records.next().unwrap_or_default();

    let mut roles: Vec<Option<Role>> = Vec::new();
    let mut columns = Vec::new();
    for h in &header {
        let role = role_of(h).filter(|r| !roles.contains(&Some(*r))); // la première colonne d'un rôle gagne
        roles.push(role);
        columns.push(ColumnInfo { header: h.trim().to_string(), role: role.map(Role::label) });
    }
    let col = |r: Role| roles.iter().position(|x| *x == Some(r));
    if col(Role::Title).is_none() {
        return Err(Error::Invalid(
            "je ne trouve pas la colonne des titres : nomme-la « Titre » (ou « Title », « Nom », « Jeu ») sur la première ligne".into(),
        ));
    }
    let rating_scale = col(Role::Rating).and_then(|i| scale_hint(&header[i]));

    let (mut rows, mut blank) = (Vec::new(), 0usize);
    for (i, rec) in records.enumerate() {
        let get = |r: Role| col(r).and_then(|c| rec.get(c)).map_or("", |s| unguard(s.trim()));
        let title = get(Role::Title).split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            blank += 1;
            continue;
        }
        if rows.len() >= MAX_ROWS {
            return Err(Error::Invalid(format!("plus de {MAX_ROWS} lignes : découpe le fichier en plusieurs parties")));
        }
        let mut issues = Vec::new();
        let mut note = |e: String| issues.push(e);

        let status_text = get(Role::Status);
        let status = if status_text.is_empty() {
            Status::Backlog
        } else {
            parse_status(status_text).unwrap_or_else(|| {
                note(format!("statut inconnu : « {status_text} » (mis en Backlog)"));
                Status::Backlog
            })
        };
        let rating_half = parse_rating(get(Role::Rating), rating_scale).unwrap_or_else(|e| {
            note(e);
            None
        });
        let minutes = parse_duration(get(Role::Hours)).unwrap_or_else(|e| {
            note(e);
            None
        });
        let added = parse_date(get(Role::Added), now).unwrap_or_else(|e| {
            note(e);
            None
        });
        let finished = parse_date(get(Role::Finished), now).unwrap_or_else(|e| {
            note(e);
            None
        });
        let igdb_text = get(Role::IgdbId);
        let igdb_id = if igdb_text.is_empty() {
            None
        } else {
            igdb_text.parse::<u64>().ok().filter(|id| *id > 0).or_else(|| {
                note(format!("identifiant IGDB illisible : « {igdb_text} »"));
                None
            })
        };
        rows.push(CsvRow {
            line: i + 2,
            title,
            year: parse_year(get(Role::Year)),
            status,
            rating_half,
            platform: clip(get(Role::Platform), PLATFORM_MAX),
            minutes,
            comment: clip(get(Role::Comment), COMMENT_MAX),
            added,
            // une date de fin n'a de sens que pour un jeu terminé, joué ou abandonné
            finished: finished.filter(|_| status.is_ended()),
            igdb_id,
            issues,
        });
    }
    if rows.is_empty() {
        return Err(Error::Invalid("aucune ligne avec un titre dans ce fichier".into()));
    }
    Ok(Parsed { rows, columns, delimiter, blank })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_792_246_980; // 2026-10-17

    #[test]
    fn decoupage_csv() {
        let t = "a;b;c\r\n\"x;1\";\"il dit \"\"oui\"\"\";\"ligne1\nligne2\"\n\n;;\nfin;;";
        let r = split_records(t, ';');
        assert_eq!(r[0], ["a", "b", "c"]);
        assert_eq!(r[1], ["x;1", "il dit \"oui\"", "ligne1\nligne2"]);
        assert_eq!(r[2], [""], "ligne vide");
        assert_eq!(r[3], ["", "", ""]);
        assert_eq!(r[4], ["fin", "", ""]);
        assert_eq!(detect_delimiter("Titre,Statut,Note\nHades,Terminé,4"), ',');
        assert_eq!(detect_delimiter("Titre\tStatut\nHades\tx"), '\t');
        assert_eq!(detect_delimiter("\"a,b\";c;d"), ';', "une virgule entre guillemets ne compte pas");
        assert_eq!(detect_delimiter("Titre"), ';');
    }

    #[test]
    fn colonnes_francaises_et_anglaises() {
        for (h, r) in [
            ("Titre", Role::Title), ("Title", Role::Title), ("Nom du jeu", Role::Title), ("Année", Role::Year), ("Statut", Role::Status),
            ("Note (sur 5)", Role::Rating), ("Rating", Role::Rating), ("Ma note", Role::Rating), ("Plateforme jouée", Role::Platform),
            ("Heures jouées", Role::Hours), ("Temps de jeu", Role::Hours), ("Commentaire", Role::Comment), ("Notes", Role::Comment),
            ("Ajouté le", Role::Added), ("Date d'ajout", Role::Added), ("Terminé le", Role::Finished), ("Date de fin", Role::Finished),
            ("Identifiant IGDB", Role::IgdbId), ("ID", Role::IgdbId),
        ] {
            assert_eq!(role_of(h), Some(r), "{h}");
        }
        for h in ["Genres", "Développeurs", "Date de sortie", "Éditeurs", ""] {
            assert_eq!(role_of(h), None, "{h}");
        }
    }

    #[test]
    fn notes_sur_differentes_echelles() {
        let r = |v: &str, s| parse_rating(v, s).unwrap();
        assert_eq!((r("4,5", None), r("4.5", None), r("5", None), r("0", None), r("", None)), (Some(9), Some(9), Some(10), None, None));
        assert_eq!((r("8", None), r("9/10", None), r("85", None), r("85", Some(100.0)), r("3", Some(10.0))), (Some(8), Some(9), Some(9), Some(9), Some(3)));
        assert_eq!((r("★★★★½", None), r("★★★", None), r("4/5", None), r("2,2", None)), (Some(9), Some(6), Some(8), Some(4)));
        assert!(parse_rating("super", None).is_err() && parse_rating("12", Some(10.0)).is_err() && parse_rating("-1", None).is_err());
    }

    #[test]
    fn dates_lues() {
        let d = |v: &str| parse_date(v, NOW);
        assert_eq!(d("2026-10-17").unwrap().map(|t| t % 86_400), Some(43_200), "midi UTC");
        assert_eq!(d("17/10/2026").unwrap(), d("2026-10-17").unwrap());
        assert_eq!(d("17.10.2026").unwrap(), d("17-10-2026 14:30").unwrap());
        assert_eq!(d("").unwrap(), None);
        assert!(d("29/02/2024").unwrap().is_some(), "année bissextile");
        for bad in ["31/02/2020", "29/02/2023", "31/04/2024", "hier", "31/02/2026x", "2026-13-01", "1970-01-01", "2030-01-01", "10/2026"] {
            assert!(d(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn fichier_complet() {
        let csv = "\u{feff}Titre;Année;Statut;Note (sur 5);Plateforme jouée;Heures jouées;Commentaire;Genres;Ajouté le;Terminé le;Identifiant IGDB\r\n\
            Hades;2020;Terminé;4,5;PC;12h30;\"Génial; vraiment\";Roguelike;2025-01-10;2025-02-01;113112\r\n\
            Zelda  TOTK;2023;en cours;;Switch;45 min;'=1+1;;;;\r\n\
            ;;;;;;;;;;\r\n\
            Celeste;;Fini;9/10;;;;;;;abc\r\n\
            Doom;;bizarre;beaucoup;;vite;;;demain;;\r\n";
        let p = parse(csv, NOW).unwrap();
        assert_eq!((p.delimiter, p.rows.len(), p.blank), (';', 4, 1));
        assert_eq!(p.columns.iter().find(|c| c.header == "Genres").unwrap().role, None);
        let (h, z, c, d) = (&p.rows[0], &p.rows[1], &p.rows[2], &p.rows[3]);
        assert_eq!((h.title.as_str(), h.year, h.status, h.rating_half, h.minutes, h.igdb_id), ("Hades", Some(2020), Status::Completed, Some(9), Some(750), Some(113_112)));
        assert_eq!(h.comment.as_deref(), Some("Génial; vraiment"));
        assert!(h.added.is_some() && h.finished.is_some() && h.issues.is_empty());
        assert_eq!((z.title.as_str(), z.status, z.minutes, z.comment.as_deref()), ("Zelda TOTK", Status::Playing, Some(45), Some("=1+1")));
        assert_eq!((z.line, c.line, d.line), (3, 5, 6), "numéros de ligne du fichier");
        assert_eq!((c.status, c.rating_half, c.igdb_id), (Status::Completed, Some(9), None));
        assert!(c.issues.iter().any(|i| i.contains("IGDB")));
        assert_eq!((d.status, d.rating_half, d.minutes, d.added), (Status::Backlog, None, None, None));
        assert_eq!(d.issues.len(), 4, "{:?}", d.issues);
    }

    #[test]
    fn la_date_de_fin_suppose_un_statut_de_fin() {
        let p = parse("Titre;Statut;Terminé le\nA;Backlog;2025-01-01\nB;Terminé;2025-01-01\n", NOW).unwrap();
        assert_eq!((p.rows[0].finished, p.rows[1].finished.is_some()), (None, true));
    }

    #[test]
    fn fichiers_inacceptables() {
        for bad in ["", "  \n ", "Genre;Année\nRPG;2020", "Titre;Statut\n;\n;"] {
            assert!(parse(bad, NOW).is_err(), "{bad:?}");
        }
        let many = format!("Titre\n{}", (0..=MAX_ROWS).map(|i| format!("Jeu {i}\n")).collect::<String>());
        assert!(parse(&many, NOW).unwrap_err().to_string().contains("découpe"));
        // première colonne d'un rôle gagnante
        let p = parse("Titre;Nom\nA;B", NOW).unwrap();
        assert_eq!((p.rows[0].title.as_str(), p.columns[1].role), ("A", None));
    }

    #[test]
    fn l_export_maison_se_relit() {
        // le CSV écrit par `transfer::to_csv` doit être compris tel quel
        let csv = "\u{feff}Titre;Année;Statut;Note (sur 5);Plateforme jouée;Heures jouées;Commentaire;Genres;Développeurs;Éditeurs;Date de sortie;Ajouté le;Terminé le;Identifiant IGDB\r\n\
            Hades;2020;Terminé;4,5;PC;12,33;'=SOMME(A1);Roguelike;Supergiant;Supergiant;2020-09-17;2025-01-01;2025-02-01;113112\r\n";
        let r = &parse(csv, NOW).unwrap().rows[0];
        assert_eq!((r.title.as_str(), r.status, r.rating_half, r.minutes, r.igdb_id), ("Hades", Status::Completed, Some(9), Some(740), Some(113_112)));
        assert_eq!(r.comment.as_deref(), Some("=SOMME(A1)"));
        assert!(r.issues.is_empty());
    }
}
