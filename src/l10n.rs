//! Affichage en français des libellés IGDB (genres, thèmes, modes, perspectives) et des dates.
//!
//! IGDB renvoie ces libellés en anglais. Les valeurs stockées restent celles d'IGDB ; la traduction
//! se fait à l'affichage avec une table fixe (aucun service externe). Un libellé inconnu est affiché tel quel.

const TAGS: &[(&str, &str)] = &[
    // genres
    ("Point-and-click", "Point-and-click"),
    ("Fighting", "Combat"),
    ("Shooter", "Tir"),
    ("Music", "Musique"),
    ("Platform", "Plateforme"),
    ("Puzzle", "Réflexion"),
    ("Racing", "Course"),
    ("Real Time Strategy (RTS)", "Stratégie en temps réel"),
    ("Role-playing (RPG)", "Jeu de rôle"),
    ("Simulator", "Simulation"),
    ("Sport", "Sport"),
    ("Strategy", "Stratégie"),
    ("Turn-based strategy (TBS)", "Stratégie au tour par tour"),
    ("Tactical", "Tactique"),
    ("Hack and slash/Beat 'em up", "Beat them all"),
    ("Quiz/Trivia", "Quiz"),
    ("Pinball", "Flipper"),
    ("Adventure", "Aventure"),
    ("Indie", "Indépendant"),
    ("Arcade", "Arcade"),
    ("Visual Novel", "Roman visuel"),
    ("Card & Board Game", "Cartes et jeux de plateau"),
    ("MOBA", "MOBA"),
    // thèmes
    ("Action", "Action"),
    ("Fantasy", "Fantasy"),
    ("Science fiction", "Science-fiction"),
    ("Horror", "Horreur"),
    ("Thriller", "Thriller"),
    ("Survival", "Survie"),
    ("Historical", "Historique"),
    ("Stealth", "Infiltration"),
    ("Comedy", "Comédie"),
    ("Business", "Gestion"),
    ("Drama", "Drame"),
    ("Non-fiction", "Non-fiction"),
    ("Sandbox", "Bac à sable"),
    ("Educational", "Éducatif"),
    ("Kids", "Enfants"),
    ("Open world", "Monde ouvert"),
    ("Warfare", "Guerre"),
    ("Party", "Jeu de société"),
    ("Mystery", "Mystère"),
    ("Romance", "Romance"),
    // modes de jeu
    ("Single player", "Solo"),
    ("Multiplayer", "Multijoueur"),
    ("Co-operative", "Coopération"),
    ("Split screen", "Écran partagé"),
    ("Massively Multiplayer Online (MMO)", "MMO"),
    ("Battle Royale", "Battle royale"),
    // perspectives
    ("First person", "Vue à la première personne"),
    ("Third person", "Vue à la troisième personne"),
    ("Bird view / Isometric", "Vue du dessus / isométrique"),
    ("Side view", "Vue de côté"),
    ("Text", "Textuel"),
    ("Auxiliary view", "Vue auxiliaire"),
    ("Virtual Reality", "Réalité virtuelle"),
    ("4X (explore, expand, exploit, and exterminate)", "4X"),
];

/// Libellé français d'un genre, thème, mode ou perspective IGDB ; inchangé s'il est inconnu.
pub fn tag_fr(name: &str) -> String {
    TAGS.iter().find(|(en, _)| en.eq_ignore_ascii_case(name)).map_or_else(|| name.to_string(), |(_, fr)| (*fr).to_string())
}

const MONTHS: [&str; 12] =
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];

/// « 3 mars 2017 » (« 1er » pour le premier du mois).
pub fn date_fr(secs: i64) -> String {
    let (y, m, d) = crate::igdb::ymd_from_unix(secs);
    let day = if d == 1 { "1er".to_string() } else { d.to_string() };
    format!("{day} {} {y}", MONTHS[(m - 1) as usize])
}

/// « 12 h 30 », « 45 min », « 120 h ».
pub fn duration_fr(minutes: u32) -> String {
    let (h, m) = (minutes / 60, minutes % 60);
    match (h, m) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// Lit une durée saisie à la main : `12`, `12,5`, `12.5`, `12h`, `12h30`, `12 h 30`, `1:30`, `45 min`.
/// Renvoie des minutes, ou `None` si le texte est vide. Erreur si illisible ou démesuré.
pub fn parse_duration(text: &str) -> Result<Option<u32>, String> {
    const MAX_MINUTES: f64 = 99_999.0 * 60.0;
    let t = text.trim().to_lowercase().replace(['\u{a0}', '\u{202f}'], " ");
    if t.is_empty() {
        return Ok(None);
    }
    let bad = || format!("durée illisible : « {} » (exemples : 12, 12,5, 12h30, 45 min)", text.trim());
    let num = |s: &str| -> Result<f64, String> { s.trim().replace(',', ".").parse::<f64>().map_err(|_| bad()) };
    let minutes = if let Some(rest) = t.strip_suffix("min").or_else(|| t.strip_suffix("mn")).or_else(|| t.strip_suffix('m')) {
        num(rest)?
    } else if let Some((h, m)) = t.split_once(['h', ':']) {
        let hours = if h.trim().is_empty() { 0.0 } else { num(h)? };
        let mins = if m.trim().is_empty() { 0.0 } else { num(m)? };
        if mins >= 60.0 {
            return Err(bad());
        }
        hours * 60.0 + mins
    } else {
        num(&t)? * 60.0
    };
    if !(0.0..=MAX_MINUTES).contains(&minutes) {
        return Err(bad());
    }
    let m = minutes.round() as u32;
    Ok(if m == 0 { None } else { Some(m) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genres_et_dates() {
        assert_eq!(tag_fr("Role-playing (RPG)"), "Jeu de rôle");
        assert_eq!(tag_fr("single PLAYER"), "Solo");
        assert_eq!(tag_fr("Genre inconnu"), "Genre inconnu");
        assert_eq!(date_fr(1_488_499_200), "3 mars 2017");
        assert_eq!(date_fr(1_600_300_800), "17 septembre 2020");
        assert_eq!(date_fr(1_704_067_200), "1er janvier 2024");
    }

    #[test]
    fn durees() {
        assert_eq!(duration_fr(45), "45 min");
        assert_eq!(duration_fr(120), "2 h");
        assert_eq!(duration_fr(750), "12 h 30");
        assert_eq!(duration_fr(65), "1 h 05");
        for (txt, want) in [("12", 720), ("12,5", 750), ("12.5", 750), ("12h", 720), ("12h30", 750), ("12 h 30", 750), ("1:30", 90), ("45 min", 45), ("45min", 45), ("0,25", 15)] {
            assert_eq!(parse_duration(txt), Ok(Some(want)), "{txt}");
        }
        assert_eq!(parse_duration("  "), Ok(None));
        assert_eq!(parse_duration("0"), Ok(None), "zéro heure = effacé");
        for bad in ["abc", "-3", "12h75", "100000", "1e9", "NaN", "h-"] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn la_table_n_a_pas_de_doublon() {
        let mut keys: Vec<String> = TAGS.iter().map(|(en, _)| en.to_lowercase()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), TAGS.len());
    }
}
