//! Rendu et lecture des fragments HTML / Turbo Streams de la recherche.
//!
//! OBSERVÉ (E06, E07) : la page `/search/games/<q>/` contient `div#search-results` avec 11 cartes
//! `div.col-12.result[game_id]`, suivies d'un `turbo-frame#pagination[loading=lazy][src=/search/results.turbo_stream?page=N&query=Q&type=games]`.
//! Ce cadre charge `text/vnd.turbo-stream.html` : un `turbo-stream append → search-results` (11 cartes) puis
//! un `turbo-stream replace → pagination` contenant le cadre de la page suivante.
//! NON DÉTERMINÉ : contenu de la réponse sur la dernière page (ici : cadre vide).

use crate::catalog::Game;
use crate::search::SearchPage;
use crate::text::{html_escape, html_unescape, url_encode};

pub const TURBO_STREAM_CONTENT_TYPE: &str = "text/vnd.turbo-stream.html; charset=utf-8";

/// Carte de résultat (balisage modelé sur E06/E07, sans images ni plateformes).
pub fn render_result(game: &Game) -> String {
    format!(
        "<div class=\"col-12 result\">\n\t<div class=\"row\" game_id=\"{id}\">\n\t\t<div class=\"col my-auto\">\n\t\t\t<a href=\"/games/{slug}/\">\n\t\t\t\t<h3 class=\"mb-0 mt-2\">{title} <span class=\"subtitle-text\">{year}</span></h3>\n\t\t\t</a>\n\t\t</div>\n\t</div>\n\t<hr class=\"my-3\">\n</div>\n",
        id = game.id,
        slug = html_escape(&game.slug),
        title = html_escape(&game.title),
        year = html_escape(&game.year),
    )
}

fn pagination_frame(query: &str, next_page: Option<usize>) -> String {
    match next_page {
        Some(n) => format!(
            "<turbo-frame loading=\"lazy\" id=\"pagination\" src=\"/search/results.turbo_stream?page={n}&amp;query={}&amp;type=games\"></turbo-frame>",
            html_escape(&url_encode(query))
        ),
        None => "<turbo-frame id=\"pagination\"></turbo-frame>".to_string(),
    }
}

/// Page complète `/search/games/<q>/`.
pub fn render_search_page(query: &str, page: &SearchPage<'_>) -> String {
    let cards: String = page.games.iter().map(|g| render_result(g)).collect();
    format!(
        "<!DOCTYPE html>\n<html lang=\"fr\"><head><meta charset=\"utf-8\"><title>Résultats pour {q} | Backlog Panda</title></head>\n<body data-route=\"pages#search\">\n<main role=\"main\" class=\"main\">\n<div class=\"row mt-2\" id=\"search-title\"><h1>Résultats pour {q}</h1></div>\n<div class=\"row mt-3\" id=\"search-results\">\n{cards}</div>\n{frame}\n</main>\n</body></html>\n",
        q = html_escape(query),
        frame = pagination_frame(query, page.next_page),
    )
}

/// Réponse de `/search/results.turbo_stream?page=N&query=Q&type=games`.
pub fn render_turbo_stream_page(query: &str, page: &SearchPage<'_>) -> String {
    let cards: String = page.games.iter().map(|g| render_result(g)).collect();
    format!(
        "<turbo-stream action=\"append\" target=\"search-results\"><template>\n{cards}</template></turbo-stream>\n<turbo-stream action=\"replace\" target=\"pagination\"><template>\n{}</template></turbo-stream>\n",
        pagination_frame(query, page.next_page)
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedResult {
    pub game_id: u64,
    pub slug: String,
    pub title: String,
    pub year: String,
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)?;
    Some(&s[i..i + j])
}

/// Extrait les cartes `div.col-12.result` d'une page ou d'un fragment.
pub fn parse_results(html: &str) -> Vec<ParsedResult> {
    const MARK: &str = "class=\"col-12 result\"";
    let mut positions: Vec<usize> = html.match_indices(MARK).map(|(i, _)| i).collect();
    positions.push(html.len());
    let mut out = Vec::new();
    for w in positions.windows(2) {
        let chunk = &html[w[0]..w[1]];
        let Some(game_id) = between(chunk, "game_id=\"", "\"").and_then(|s| s.parse().ok()) else { continue };
        let Some(slug) = between(chunk, "href=\"/games/", "/\"") else { continue };
        let Some(h3) = between(chunk, "<h3", "</h3>") else { continue };
        let inner = h3.split_once('>').map_or(h3, |(_, t)| t);
        let (title, year) = match inner.split_once("<span class=\"subtitle-text\">") {
            Some((t, y)) => (t, y.split("</span>").next().unwrap_or("")),
            None => (inner, ""),
        };
        out.push(ParsedResult {
            game_id,
            slug: html_unescape(slug),
            title: html_unescape(title.trim()),
            year: html_unescape(year.trim()),
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurboStream {
    pub action: String,
    pub target: String,
    pub template: String,
}

/// Lit les éléments `<turbo-stream action=… target=…><template>…</template></turbo-stream>`.
pub fn parse_turbo_streams(body: &str) -> Vec<TurboStream> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("<turbo-stream ") {
        rest = &rest[i + "<turbo-stream ".len()..];
        let Some(tag_end) = rest.find('>') else { break };
        let attrs = &rest[..tag_end];
        let action = between(attrs, "action=\"", "\"").unwrap_or("").to_string();
        let target = between(attrs, "target=\"", "\"").unwrap_or("").to_string();
        rest = &rest[tag_end + 1..];
        let Some(t0) = rest.find("<template>") else { break };
        let after = &rest[t0 + "<template>".len()..];
        let Some(t1) = after.find("</template>") else { break };
        out.push(TurboStream { action, target, template: after[..t1].to_string() });
        rest = &after[t1..];
    }
    out
}

/// Numéro de page demandé par un `turbo-frame#pagination` (`…?page=N&…`).
pub fn next_page(fragment: &str) -> Option<usize> {
    let frame = &fragment[fragment.find("id=\"pagination\"")?..];
    let digits: String = between(frame, "page=", "&")?.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::search::search_page;

    #[test]
    fn rendu_puis_relecture_des_cartes() {
        let mut c = Catalog::new();
        c.add("Champions' Ballad & <more>", "TBD");
        let page = search_page(&c, "champions", 1);
        let html = render_search_page("champions", &page);
        let parsed = parse_results(&html);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].title, "Champions' Ballad & <more>");
        assert_eq!(parsed[0].year, "TBD");
        assert_eq!(parsed[0].game_id, 1);
        assert_eq!(next_page(&html), None);
    }

    #[test]
    fn flux_turbo_chaine_la_page_suivante() {
        let mut c = Catalog::new();
        for i in 0..14 {
            c.add(&format!("Zelda {i}"), "2000");
        }
        let body = render_turbo_stream_page("zelda link", &search_page(&c, "zelda", 1));
        let streams = parse_turbo_streams(&body);
        assert_eq!(streams.len(), 2);
        assert_eq!((streams[0].action.as_str(), streams[0].target.as_str()), ("append", "search-results"));
        assert_eq!((streams[1].action.as_str(), streams[1].target.as_str()), ("replace", "pagination"));
        assert_eq!(parse_results(&streams[0].template).len(), 11);
        assert_eq!(next_page(&streams[1].template), Some(2));
        assert!(streams[1].template.contains("query=zelda%20link"));
    }

    #[test]
    fn fragment_sans_pagination() {
        assert_eq!(next_page("<p>x</p>"), None);
        assert!(parse_turbo_streams("rien").is_empty());
        assert!(parse_results("<div></div>").is_empty());
    }
}
