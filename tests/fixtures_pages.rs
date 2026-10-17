//! Teste les parseurs et la logique de notes contre les extraits RÉELS (E06, E07, E08, E13).

use backlog_panda::rating::{Histogram, EMPTY_BAR_THRESHOLD_PERCENT};
use backlog_panda::turbo::{next_page, parse_results, parse_turbo_streams};
use std::path::PathBuf;

fn raw(name: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").join(name)).unwrap()
}

#[test]
fn page_de_recherche_reelle_e06() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let html = raw("E06_search_page_excerpt.html");
    let results = parse_results(&html);
    assert_eq!(results.len(), 2, "l'extrait E06 ne garde que 2 cartes sur 11");
    assert_eq!(results[0].game_id, 7346);
    assert_eq!(results[0].slug, "the-legend-of-zelda-breath-of-the-wild");
    assert_eq!(results[0].title, "The Legend of Zelda: Breath of the Wild");
    assert_eq!(results[0].year, "2017");
    assert_eq!(next_page(&html), Some(2));
}

#[test]
fn flux_turbo_reel_e07() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let body = raw("E07_turbo_stream_search_page2.txt");
    let streams = parse_turbo_streams(&body);
    assert_eq!(streams.len(), 2);
    assert_eq!((streams[0].action.as_str(), streams[0].target.as_str()), ("append", "search-results"));
    assert_eq!((streams[1].action.as_str(), streams[1].target.as_str()), ("replace", "pagination"));
    let cards = parse_results(&streams[0].template);
    assert_eq!(cards.len(), 11, "11 résultats par page");
    assert_eq!(cards[0].title, "The Legend of Zelda: The Wind Waker");
    assert_eq!(cards[0].year, "2002");
    assert_eq!(next_page(&streams[1].template), Some(3));
}

fn jsonld_number(doc: &str, key: &str) -> f64 {
    let i = doc.find(&format!("\"{key}\": \"")).unwrap() + key.len() + 5;
    doc[i..].split('"').next().unwrap().parse().unwrap()
}

#[test]
fn histogramme_reel_e08_et_moyenne_e13() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let h = Histogram::parse_tooltips(&raw("E08_fetch_game_stats_7346.txt")).unwrap();
    assert_eq!(h.counts, [258, 333, 384, 1186, 1722, 4433, 8218, 20502, 27955, 60397]);
    assert_eq!(h.total(), 125_388);

    // OBSERVÉ : la moyenne du JSON-LD est exactement Σ(demi-étoiles × n) / 2 / Σn.
    let page = raw("E13_game_page_excerpt.txt");
    let rating_value = jsonld_number(&page, "ratingValue");
    assert_eq!(h.average().unwrap(), rating_value);
    // … et NON la division par `ratingCount` (125 431), qui diffère de 43 de la somme des barres.
    assert_eq!(jsonld_number(&page, "ratingCount"), 125_431.0);
    assert_ne!(h.total() as f64, 125_431.0);
}

#[test]
fn pourcentages_et_hauteurs_de_barres_reels() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let text = raw("E08_fetch_game_stats_7346.txt");
    let h = Histogram::parse_tooltips(&text).unwrap();
    // Libellés « (x.y%) » : tous identiques avec le dénominateur Σn (le 16,4 % exclut ratingCount).
    for i in 0..10 {
        let label = format!("({:.1}%)", h.percent(i));
        let tip = h.tooltip(i);
        assert!(tip.ends_with(&label), "{tip}");
        assert!(text.contains(&format!("data-tippy-content=\"{tip}\"")), "{tip}");
    }
    // Hauteurs : mêmes valeurs que le style inline observé (n / max × 100).
    let heights = h.bar_heights();
    let styles: Vec<f64> = text
        .match_indices("style=\"height: ")
        .map(|(i, m)| text[i + m.len()..].split('%').next().unwrap().parse().unwrap())
        .take(10)
        .collect();
    assert_eq!(styles.len(), 10);
    for (a, b) in heights.iter().zip(&styles) {
        assert!((a - b).abs() < 1e-12, "{a} != {b}");
    }
    // Classe `empty` : observée sur les 3 premières barres seulement (< 0,64 %), pas sur 1,96 %.
    let classes: Vec<bool> = text.match_indices("<div class=\"bar-h ").take(10).map(|(i, m)| text[i + m.len()..].starts_with("empty")).collect();
    let ours: Vec<bool> = heights.iter().map(|&p| p < EMPTY_BAR_THRESHOLD_PERCENT).collect();
    assert_eq!(classes, ours);
}

#[test]
fn nos_barres_se_relisent_comme_les_barres_reelles() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let h = Histogram::parse_tooltips(&raw("E08_fetch_game_stats_7346.txt")).unwrap();
    let ours = h.render_bars("the-legend-of-zelda-breath-of-the-wild");
    assert_eq!(Histogram::parse_tooltips(&ours).unwrap(), h);
    for n in 1..=10 {
        assert!(ours.contains(&format!("/logs/the-legend-of-zelda-breath-of-the-wild/plays/rating:{n};/")));
    }
}
