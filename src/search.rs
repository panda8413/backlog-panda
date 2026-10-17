//! Recherche : autocomplétion JSON et pages de résultats paginées.
//!
//! Constantes OBSERVÉES (voir docs/REPORT.md) :
//! - au plus 50 suggestions (E05 : « ze », « zelda ») ;
//! - 11 résultats par page de recherche (E06 : 11 cartes, E07 : 11 cartes) ;
//! - une requête d'1 caractère ou vide renvoie `{"suggestions":[]}` (E05 : « h », « »).
//!
//! NON DÉTERMINÉ : l'ordre de classement (stable à l'intérieur d'une même réponse mais non expliqué).
//! Ici : ordre d'insertion dans le catalogue.

use crate::catalog::{Catalog, Game};
use crate::text::normalize;
use serde::{Deserialize, Serialize};

pub const AUTOCOMPLETE_MAX: usize = 50;
pub const AUTOCOMPLETE_MIN_CHARS: usize = 2;
pub const RESULTS_PER_PAGE: usize = 11;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestionData {
    pub slug: String,
    pub title: String,
    pub year: String,
    pub id: u64,
}

/// Forme exacte d'une suggestion observée : `{"value":…,"data":{"slug","title","year","id"}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub value: String,
    pub data: SuggestionData,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AutocompleteResponse {
    pub suggestions: Vec<Suggestion>,
}

impl Suggestion {
    /// Avec `filter_editions`, une édition prend l'id et le slug de son jeu parent mais garde son titre
    /// (OBSERVÉ, E05 « zelda links »). Le champ `year` reste celui de l'édition (indiscernable dans E05).
    pub fn from_game(catalog: &Catalog, game: &Game, filter_editions: bool) -> Self {
        let (id, slug) = match (filter_editions, game.edition_of.and_then(|p| catalog.get(p))) {
            (true, Some(parent)) => (parent.id, parent.slug.clone()),
            _ => (game.id, game.slug.clone()),
        };
        Suggestion {
            value: game.title.clone(),
            data: SuggestionData { slug, title: game.title.clone(), year: game.year.clone(), id },
        }
    }
}

/// Prédicat de recherche : le texte normalisé de la requête est une sous-chaîne du titre normalisé.
pub fn matches(game: &Game, normalized_query: &str) -> bool {
    normalize(&game.title).contains(normalized_query)
}

pub fn autocomplete(catalog: &Catalog, query: &str, filter_editions: bool) -> AutocompleteResponse {
    let q = normalize(query);
    if q.chars().count() < AUTOCOMPLETE_MIN_CHARS {
        return AutocompleteResponse::default();
    }
    let suggestions = catalog
        .games()
        .iter()
        .filter(|g| matches(g, &q))
        .take(AUTOCOMPLETE_MAX)
        .map(|g| Suggestion::from_game(catalog, g, filter_editions))
        .collect();
    AutocompleteResponse { suggestions }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchPage<'a> {
    pub games: Vec<&'a Game>,
    pub page: usize,
    /// Page suivante à charger paresseusement (`turbo-frame#pagination`), absente sur la dernière page.
    pub next_page: Option<usize>,
}

/// Page de résultats (numérotée à partir de 1). DÉDUIT : même prédicat que l'autocomplétion.
pub fn search_page<'a>(catalog: &'a Catalog, query: &str, page: usize) -> SearchPage<'a> {
    let page = page.max(1);
    let q = normalize(query);
    if q.is_empty() {
        return SearchPage { games: vec![], page, next_page: None };
    }
    let hits: Vec<&Game> = catalog.games().iter().filter(|g| matches(g, &q)).collect();
    let start = (page - 1) * RESULTS_PER_PAGE;
    let games: Vec<&Game> = hits.iter().skip(start).take(RESULTS_PER_PAGE).copied().collect();
    let next_page = (hits.len() > start + RESULTS_PER_PAGE).then_some(page + 1);
    SearchPage { games, page, next_page }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(titles: &[&str]) -> Catalog {
        let mut c = Catalog::new();
        for t in titles {
            c.add(t, "2000");
        }
        c
    }

    #[test]
    fn sous_chaine_normalisee() {
        let c = catalog(&["Taisen Puzzle-Dama", "F-Zero", "Hades"]);
        assert_eq!(autocomplete(&c, "zleda", true).suggestions.len(), 1);
        assert_eq!(autocomplete(&c, "ZE", true).suggestions[0].value, "F-Zero");
        assert!(autocomplete(&c, "xx", true).suggestions.is_empty());
    }

    #[test]
    fn minimum_deux_caracteres() {
        let c = catalog(&["Hades"]);
        assert!(autocomplete(&c, "h", true).suggestions.is_empty());
        assert!(autocomplete(&c, "", true).suggestions.is_empty());
        assert!(autocomplete(&c, " - ", true).suggestions.is_empty());
        assert_eq!(autocomplete(&c, "ha", true).suggestions.len(), 1);
    }

    #[test]
    fn plafond_de_cinquante() {
        let titles: Vec<String> = (0..80).map(|i| format!("Zelda {i}")).collect();
        let mut c = Catalog::new();
        for t in &titles {
            c.add(t, "2000");
        }
        assert_eq!(autocomplete(&c, "zelda", true).suggestions.len(), 50);
    }

    #[test]
    fn editions_prennent_l_id_du_parent_si_filtre() {
        let mut c = Catalog::new();
        let mk = |id, slug: &str, title: &str, edition_of| Game {
            id,
            slug: slug.into(),
            title: title.into(),
            year: "2019".into(),
            edition_of,
        };
        c.insert(mk(1, "link", "Link", None)).unwrap();
        c.insert(mk(2, "link-deluxe", "Link - Deluxe Edition", Some(1))).unwrap();
        let on = autocomplete(&c, "link", true).suggestions;
        let off = autocomplete(&c, "link", false).suggestions;
        assert_eq!(on[1].data.id, 1);
        assert_eq!(on[1].data.slug, "link");
        assert_eq!(on[1].data.title, "Link - Deluxe Edition");
        assert_eq!(off[1].data.id, 2);
        assert_eq!(off[1].data.slug, "link-deluxe");
    }

    #[test]
    fn pagination_par_onze() {
        let titles: Vec<String> = (0..25).map(|i| format!("Zelda {i}")).collect();
        let mut c = Catalog::new();
        for t in &titles {
            c.add(t, "2000");
        }
        let p1 = search_page(&c, "zelda", 1);
        assert_eq!((p1.games.len(), p1.next_page), (11, Some(2)));
        let p2 = search_page(&c, "zelda", 2);
        assert_eq!((p2.games.len(), p2.next_page), (11, Some(3)));
        let p3 = search_page(&c, "zelda", 3);
        assert_eq!((p3.games.len(), p3.next_page), (3, None));
        assert!(search_page(&c, "zelda", 9).games.is_empty());
        assert!(search_page(&c, "", 1).games.is_empty());
    }
}
