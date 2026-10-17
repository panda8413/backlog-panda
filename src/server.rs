//! Application HTTP : routes calquées sur ce qui a été observé (recherche) + API de bibliothèque (conception propre).
//!
//! Routes OBSERVÉES sur Backloggd, reproduites avec le même contrat :
//! - `GET /autocomplete.json?query=…&filter_editions=true|false` → JSON `{"suggestions":[…]}` échappé à la Rails ;
//! - `GET /search/games/<q>/` → page HTML (11 résultats + cadre paresseux) ;
//! - `GET /search/results.turbo_stream?page=N&query=Q&type=games` → Turbo Streams.
//!
//! Routes de CONCEPTION PROPRE : `POST /search` (redirection 303, comportement réel NON DÉTERMINÉ),
//! `GET /api/library`, `PUT|DELETE /api/library/<id>`, `GET /api/library/stats`, `POST /api/sync`.

use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::http::{parse_form, parse_request, Request, Response};
use crate::library::{Library, Status};
use crate::rating::Rating;
use crate::search::{autocomplete, search_page};
use crate::sync::handle_sync;
use crate::text::{url_decode, url_encode};
use crate::{rails_json, turbo};
use serde::Deserialize;
use std::io::{BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;

pub struct App {
    pub catalog: Catalog,
    pub library: Library,
    pub library_path: Option<PathBuf>,
}

#[derive(Deserialize)]
struct PutBody {
    status: Status,
    #[serde(default)]
    rating: Option<f64>,
}

impl App {
    pub fn new(catalog: Catalog, library: Library) -> Self {
        App { catalog, library, library_path: None }
    }

    fn persist(&self) -> Result<()> {
        match &self.library_path {
            Some(p) => self.library.save(p),
            None => Ok(()),
        }
    }

    pub fn handle(&mut self, req: &Request) -> Response {
        match self.route(req) {
            Ok(r) => r,
            Err(e @ (Error::UnknownGame(_) | Error::NotInLibrary(_))) => Response::error(404, &e.to_string()),
            Err(e @ Error::Io(_)) => Response::error(500, &e.to_string()),
            Err(e) => Response::error(400, &e.to_string()),
        }
    }

    fn route(&mut self, req: &Request) -> Result<Response> {
        let segments: Vec<&str> = req.path.split('/').filter(|s| !s.is_empty()).collect();
        let method = req.method.as_str();
        match (method, segments.as_slice()) {
            ("GET", ["autocomplete.json"]) => {
                let query = req.param("query").unwrap_or("");
                // OBSERVÉ (E05) : sans paramètre, la réponse est identique à `filter_editions=true`.
                let filter = req.param("filter_editions") != Some("false");
                let body = rails_json::to_string(&autocomplete(&self.catalog, query, filter))?;
                Ok(Response::json(200, body))
            }
            ("GET", ["search", "games", q]) => {
                let query = url_decode(q);
                let page = search_page(&self.catalog, &query, page_param(req));
                Ok(Response::html(turbo::render_search_page(&query, &page)))
            }
            ("GET", ["search", "results.turbo_stream"]) => {
                let query = req.param("query").unwrap_or("");
                let page = search_page(&self.catalog, query, page_param(req));
                Ok(Response::new(200, turbo::TURBO_STREAM_CONTENT_TYPE, turbo::render_turbo_stream_page(query, &page)))
            }
            ("POST", ["search"]) => {
                let form = parse_form(&req.body);
                let q = form.iter().find(|(k, _)| k == "query").map_or("", |(_, v)| v.as_str());
                Ok(Response::redirect(format!("/search/games/{}/", url_encode(q))))
            }
            ("GET", ["api", "library"]) => Ok(Response::json(200, serde_json::to_string(&self.library)?)),
            ("GET", ["api", "library", "stats"]) => {
                let h = self.library.histogram();
                Ok(Response::json(
                    200,
                    serde_json::json!({ "total": h.total(), "average": h.average(), "counts": h.counts }).to_string(),
                ))
            }
            ("PUT", ["api", "library", id]) => {
                let id: u64 = id.parse().map_err(|_| Error::Parse(format!("identifiant invalide : {id}")))?;
                if self.catalog.get(id).is_none() {
                    return Err(Error::UnknownGame(id));
                }
                let body: PutBody = serde_json::from_str(&req.body)?;
                let rating = body.rating.map(Rating::from_stars).transpose()?;
                self.library.upsert(id, body.status, rating);
                self.persist()?;
                Ok(Response::json(200, serde_json::to_string(&self.library.get(id))?))
            }
            ("DELETE", ["api", "library", id]) => {
                let id: u64 = id.parse().map_err(|_| Error::Parse(format!("identifiant invalide : {id}")))?;
                self.library.remove(id)?;
                self.persist()?;
                Ok(Response::json(200, "{\"deleted\":true}".into()))
            }
            ("POST", ["api", "sync"]) => {
                let (body, _) = handle_sync(&mut self.library, &req.body)?;
                self.persist()?;
                Ok(Response::json(200, body))
            }
            (_, ["autocomplete.json"] | ["search", ..] | ["api", ..]) => Ok(Response::error(405, "méthode non autorisée")),
            _ => Ok(Response::error(404, "introuvable")),
        }
    }
}

fn page_param(req: &Request) -> usize {
    req.param("page").and_then(|p| p.parse().ok()).unwrap_or(1)
}

/// Sert les connexions une par une ; s'arrête après `max_requests` requêtes si fourni.
pub fn serve(listener: &TcpListener, app: &mut App, max_requests: Option<usize>) -> std::io::Result<()> {
    for (served, stream) in listener.incoming().enumerate() {
        let mut stream = stream?;
        let response = match parse_request(&mut BufReader::new(&stream)) {
            Ok(req) => app.handle(&req),
            Err(e) => Response::error(400, &e.to_string()),
        };
        stream.write_all(&response.to_bytes())?;
        stream.flush()?;
        if max_requests.is_some_and(|m| served + 1 >= m) {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut c = Catalog::new();
        c.add("The Legend of Zelda: Link's Awakening", "1993");
        c.add("Hades", "2020");
        App::new(c, Library::new("serveur"))
    }

    fn get(app: &mut App, target: &str) -> Response {
        app.handle(&Request::new("GET", target, ""))
    }

    #[test]
    fn autocomplete_json_echappe_a_la_rails() {
        let mut a = app();
        a.catalog.add("Ratchet & Clank", "2002");
        let r = get(&mut a, "/autocomplete.json?filter_editions=true&query=ratchet");
        assert_eq!(r.status, 200);
        assert!(r.body.contains("Ratchet \\u0026 Clank"));
        let r = get(&mut a, "/autocomplete.json?query=h");
        assert_eq!(r.body, "{\"suggestions\":[]}");
    }

    #[test]
    fn pages_de_recherche() {
        let mut a = app();
        let r = get(&mut a, "/search/games/link%27s/");
        assert!(r.body.contains("game_id=\"1\""));
        let r = get(&mut a, "/search/results.turbo_stream?page=1&query=hades&type=games");
        assert!(r.content_type.starts_with("text/vnd.turbo-stream.html"));
        let r = a.handle(&Request::new("POST", "/search", "query=zelda+link"));
        assert_eq!((r.status, r.location.as_deref()), (303, Some("/search/games/zelda%20link/")));
    }

    #[test]
    fn api_bibliotheque() {
        let mut a = app();
        let put = |a: &mut App, id: u64, body: &str| a.handle(&Request::new("PUT", &format!("/api/library/{id}"), body));
        assert_eq!(put(&mut a, 2, r#"{"status":"playing","rating":4.5}"#).status, 200);
        assert_eq!(put(&mut a, 99, r#"{"status":"playing"}"#).status, 404);
        assert_eq!(put(&mut a, 2, r#"{"status":"playing","rating":4.7}"#).status, 400);
        assert_eq!(put(&mut a, 2, r#"{"status":"nimporte"}"#).status, 400);
        assert_eq!(put(&mut a, 2, "pas du json").status, 400);
        let stats = get(&mut a, "/api/library/stats");
        assert!(stats.body.contains("\"average\":4.5"));
        assert_eq!(a.handle(&Request::new("DELETE", "/api/library/2", "")).status, 200);
        assert_eq!(a.handle(&Request::new("DELETE", "/api/library/2", "")).status, 404);
        assert_eq!(a.handle(&Request::new("DELETE", "/api/library/abc", "")).status, 400);
    }

    #[test]
    fn methodes_et_routes_inconnues() {
        let mut a = app();
        assert_eq!(a.handle(&Request::new("POST", "/autocomplete.json", "")).status, 405);
        assert_eq!(get(&mut a, "/nimporte/quoi/").status, 404);
    }
}
