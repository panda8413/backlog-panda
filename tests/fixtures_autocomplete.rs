//! Teste le moteur contre les réponses RÉELLES capturées sur backloggd.com (docs/evidence/E05_*).

use backlog_panda::fixtures::{autocomplete_files, import_catalog, read_response};
use backlog_panda::search::{autocomplete, AUTOCOMPLETE_MAX};
use backlog_panda::text::normalize;
use backlog_panda::catalog::Catalog;
use backlog_panda::rails_json;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn evidence() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/evidence")
}

fn raw(name: &str) -> String {
    std::fs::read_to_string(evidence().join(name)).unwrap()
}

/// Requête saisie pour chaque capture (d'après le nom de fichier, documenté dans docs/evidence/INDEX.md).
fn query_of(file: &str) -> &'static str {
    let stem = file.trim_start_matches("E05_autocomplete__");
    let key = stem.rsplit_once("__").unwrap().0;
    match key {
        "zelda" => "zelda",
        "ze" => "ze",
        "zleda" => "zleda",
        "hades" => "hades",
        "h" => "h",
        "" => "",
        "qqxzzvv" => "qqxzzvv",
        "Zelda_3A_Link_27s" => "Zelda: Link's",
        "zelda_links" => "zelda links",
        other => panic!("requête inconnue pour {other}"),
    }
}

fn file_names() -> Vec<String> {
    autocomplete_files(&evidence())
        .unwrap()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn les_captures_existent() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    assert!(file_names().len() >= 12, "{:?}", file_names());
}

#[test]
fn reserialisation_identique_octet_pour_octet() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    // OBSERVÉ : forme des clés, ordre, absence d'espaces, `&` → &, apostrophe intacte.
    for name in file_names() {
        let text = raw(&name);
        let parsed = read_response(&evidence().join(&name)).unwrap();
        assert_eq!(rails_json::to_string(&parsed).unwrap(), text, "{name}");
    }
}

#[test]
fn chaque_ligne_observee_verifie_le_predicat_de_sous_chaine() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let mut rows = 0;
    for name in file_names() {
        let q = normalize(query_of(&name));
        for s in read_response(&evidence().join(&name)).unwrap().suggestions {
            assert!(normalize(&s.data.title).contains(&q), "{name}: {:?} ne contient pas {q:?}", s.data.title);
            assert_eq!(s.value, s.data.title, "{name}");
            rows += 1;
        }
    }
    assert!(rows > 300, "{rows}");
}

#[test]
fn au_plus_cinquante_lignes_et_plafond_atteint() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let mut max_seen = 0;
    for name in file_names() {
        let n = read_response(&evidence().join(&name)).unwrap().suggestions.len();
        assert!(n <= AUTOCOMPLETE_MAX, "{name}: {n}");
        max_seen = max_seen.max(n);
    }
    assert_eq!(max_seen, AUTOCOMPLETE_MAX);
}

#[test]
fn requetes_courtes_ou_sans_resultat_donnent_une_liste_vide() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let catalog = import_catalog(&evidence()).unwrap();
    for (name, q) in [("E05_autocomplete__h__fe_true.json", "h"), ("E05_autocomplete____fe_true.json", "")] {
        assert!(read_response(&evidence().join(name)).unwrap().suggestions.is_empty());
        assert!(autocomplete(&catalog, q, true).suggestions.is_empty());
    }
    assert!(read_response(&evidence().join("E05_autocomplete__qqxzzvv__fe_true.json")).unwrap().suggestions.is_empty());
    assert!(autocomplete(&catalog, "qqxzzvv", true).suggestions.is_empty());
}

/// Pour une réponse < 50 lignes (donc complète), le moteur appliqué au catalogue de TOUTES les captures
/// doit renvoyer exactement les mêmes jeux : ni plus (faux positifs), ni moins (faux négatifs).
#[test]
fn le_moteur_reproduit_les_reponses_completes() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let catalog = import_catalog(&evidence()).unwrap();
    let mut checked = Vec::new();
    for name in file_names() {
        if name.contains("__fe_true") && name.contains("Zelda_3A") {
            continue; // lignes d'édition réécrites : couvert par le test des éditions
        }
        let observed = read_response(&evidence().join(&name)).unwrap().suggestions;
        if observed.is_empty() || observed.len() >= AUTOCOMPLETE_MAX {
            continue;
        }
        let filter = !name.contains("__fe_false");
        let ours = autocomplete(&catalog, query_of(&name), filter).suggestions;
        let ids = |v: &[backlog_panda::search::Suggestion]| v.iter().map(|s| s.data.id).collect::<BTreeSet<_>>();
        assert_eq!(ids(&ours), ids(&observed), "{name}");
        checked.push(name);
    }
    // Seules 2 captures sont complètes (< 50 lignes) : « zleda » (21) et « zelda links » (13). Les autres sont plafonnées à 50.
    assert_eq!(checked.len(), 2, "{checked:?}");
}

#[test]
fn la_requete_est_normalisee_zelda_deux_points_link_apostrophe() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    // OBSERVÉ : « Zelda: Link's » et « zelda links » donnent les mêmes jeux (2594 octets chacune pour fe_true).
    let catalog = import_catalog(&evidence()).unwrap();
    let a = autocomplete(&catalog, "Zelda: Link's", true);
    let b = autocomplete(&catalog, "zelda links", true);
    assert_eq!(a, b);
    assert_eq!(a.suggestions.len(), 13);
}

#[test]
fn filter_editions_reproduit_les_deux_variantes_observees() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let catalog = import_catalog(&evidence()).unwrap();
    let on = read_response(&evidence().join("E05_autocomplete__Zelda_3A_Link_27s__fe_true.json")).unwrap();
    let off = read_response(&evidence().join("E05_autocomplete__zelda_links__fe_false.json")).unwrap();
    assert_eq!(autocomplete(&catalog, "Zelda: Link's", true), on);
    assert_eq!(autocomplete(&catalog, "zelda links", false), off);
    // Les trois éditions (ids 136334, 134231, 136899) prennent l'id et le slug de 115284 mais gardent leur titre.
    let rewritten: Vec<_> = on.suggestions.iter().filter(|s| s.data.id == 115284).collect();
    assert_eq!(rewritten.len(), 4);
    assert!(rewritten.iter().all(|s| s.data.slug == "the-legend-of-zelda-links-awakening--1"));
}

#[test]
fn sans_parametre_le_comportement_est_celui_de_filter_editions_true() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    // OBSERVÉ : zelda__nofilter.json est identique octet pour octet à zelda__fe_true.json.
    assert_eq!(raw("E05_autocomplete__zelda__nofilter.json"), raw("E05_autocomplete__zelda__fe_true.json"));
    assert_eq!(raw("E05_autocomplete__hades__nofilter.json"), raw("E05_autocomplete__hades__fe_true.json"));
}

#[test]
fn les_slugs_stockes_ne_se_recalculent_pas_toujours() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    // DÉDUIT : `slugify` retrouve la grande majorité des slugs, mais pas tous → les slugs sont des valeurs stockées.
    let catalog: Catalog = import_catalog(&evidence()).unwrap();
    let (mut same, mut different) = (0, Vec::new());
    for g in catalog.games() {
        let base = g.slug.split("--").next().unwrap();
        if backlog_panda::text::slugify(&g.title) == base {
            same += 1;
        } else {
            different.push(g.slug.clone());
        }
    }
    assert_eq!(catalog.len(), 162);
    assert_eq!(same, 157, "écarts : {different:?}");
    assert_eq!(different.len(), 5);
}
