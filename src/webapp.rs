//! Enveloppe web locale : sert l'interface (`ui/`) et expose `POST /rpc/<méthode>` vers [`Service`].
//!
//! Sécurité (le serveur n'écoute que sur 127.0.0.1, mais n'importe quelle page web ouverte dans le
//! navigateur peut tenter d'y envoyer des requêtes) :
//! - en-tête `Host` limité à `127.0.0.1:<port>` / `localhost:<port>` (anti DNS-rebinding) ;
//! - en-tête `Origin`, s'il existe, doit être celui de l'application ;
//! - les appels RPC exigent `X-Panda: 1` : un en-tête personnalisé force une requête CORS
//!   préalable, que ce serveur ne permet jamais ;
//! - CSP stricte, `nosniff`, pas de référent, pas d'incrustation.

use crate::http::{parse_request, Request, Response};
use crate::service::Service;
use serde_json::{json, Value};
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

const HTML: &str = "text/html; charset=utf-8";

/// (chemin, type MIME, contenu) : l'interface est compilée dans l'exécutable.
const ASSETS: [(&str, &str, &[u8]); 5] = [
    ("/", HTML, include_bytes!("../ui/index.html")),
    ("/app.css", "text/css; charset=utf-8", include_bytes!("../ui/app.css")),
    ("/app.js", "text/javascript; charset=utf-8", include_bytes!("../ui/app.js")),
    ("/fonts/bricolage-grotesque-latin-wght-normal.woff2", "font/woff2", include_bytes!("../ui/fonts/bricolage-grotesque-latin-wght-normal.woff2")),
    ("/fonts/figtree-latin-wght-normal.woff2", "font/woff2", include_bytes!("../ui/fonts/figtree-latin-wght-normal.woff2")),
];

pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' https://images.igdb.com data:; \
font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

pub struct WebApp {
    service: Arc<Service>,
    hosts: Vec<String>,
}

impl WebApp {
    pub fn new(service: Arc<Service>, port: u16) -> WebApp {
        WebApp { service, hosts: vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")] }
    }

    pub fn handle(&self, req: &Request) -> Response {
        secure(self.route(req))
    }

    fn route(&self, req: &Request) -> Response {
        let host = req.header("host").unwrap_or("");
        if !self.hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
            return Response::error(403, "hôte non autorisé");
        }
        if let Some(origin) = req.header("origin") {
            if origin != format!("http://{host}") {
                return Response::error(403, "origine non autorisée");
            }
        }
        if let Some(method) = req.path.strip_prefix("/rpc/") {
            if req.method != "POST" {
                return Response::error(405, "méthode non autorisée");
            }
            if req.header("x-panda") != Some("1") {
                return Response::error(403, "en-tête X-Panda manquant");
            }
            let args: Value = if req.body.trim().is_empty() {
                json!({})
            } else {
                match serde_json::from_str(&req.body) {
                    Ok(v) => v,
                    Err(e) => return Response::error(400, &format!("JSON invalide : {e}")),
                }
            };
            return Response::json(200, self.service.call(method, &args).to_string());
        }
        if req.method != "GET" {
            return Response::error(405, "méthode non autorisée");
        }
        let path = if req.path == "/index.html" { "/" } else { req.path.as_str() };
        match ASSETS.iter().find(|(p, _, _)| *p == path) {
            Some((_, mime, data)) => {
                let cache = if *mime == "font/woff2" { "public, max-age=31536000, immutable" } else { "no-cache" };
                Response::bytes(200, mime, data).with_header("Cache-Control", cache)
            }
            None => Response::error(404, "introuvable"),
        }
    }
}

fn secure(r: Response) -> Response {
    r.with_header("Content-Security-Policy", CSP)
        .with_header("X-Content-Type-Options", "nosniff")
        .with_header("Referrer-Policy", "no-referrer")
        .with_header("X-Frame-Options", "DENY")
}

fn handle_connection(app: &WebApp, mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    // en-têtes (64 Kio au plus) + corps (32 Mio au plus : l'import d'une grosse bibliothèque passe par là)
    let limited = (&stream).take((crate::http::MAX_BODY + (64 << 10)) as u64);
    let response = match parse_request(&mut BufReader::new(limited)) {
        Ok(req) => app.handle(&req),
        Err(e) => secure(Response::error(400, &e.to_string())),
    };
    stream.write_all(&response.to_bytes())?;
    stream.flush()
}

/// Une tâche par connexion (les navigateurs ouvrent des connexions de réserve qui bloqueraient
/// une boucle séquentielle). S'arrête après `max` connexions si fourni (tests).
pub fn serve(listener: &TcpListener, app: Arc<WebApp>, max: Option<usize>) -> std::io::Result<()> {
    let mut workers = Vec::new();
    for (n, stream) in listener.incoming().enumerate() {
        let stream = stream?;
        let app = app.clone();
        workers.push(std::thread::spawn(move || {
            let _ = handle_connection(&app, stream);
        }));
        if max.is_some_and(|m| n + 1 >= m) {
            break;
        }
        workers.retain(|w| !w.is_finished());
    }
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::igdb::fake::FakeTransport;
    use crate::store::temp_dir;

    fn app(tag: &str) -> (WebApp, std::path::PathBuf) {
        let dir = temp_dir(tag);
        let svc = Arc::new(Service::open(&dir, FakeTransport::with(vec![])).unwrap());
        (WebApp::new(svc, 4000), dir)
    }

    fn req(method: &str, target: &str, body: &str) -> Request {
        Request::new(method, target, body).with_header("Host", "127.0.0.1:4000")
    }

    #[test]
    fn sert_l_interface_avec_en_tetes_de_securite() {
        let (a, dir) = app("w1");
        for (path, mime) in [("/", "text/html"), ("/index.html", "text/html"), ("/app.css", "text/css"), ("/app.js", "text/javascript"), ("/fonts/figtree-latin-wght-normal.woff2", "font/woff2")] {
            let r = a.handle(&req("GET", path, ""));
            assert_eq!(r.status, 200, "{path}");
            assert!(r.content_type.starts_with(mime), "{path}");
            assert!(r.headers.iter().any(|(k, v)| k == "Content-Security-Policy" && v.contains("default-src 'none'")));
            assert!(r.headers.iter().any(|(k, v)| k == "X-Content-Type-Options" && v == "nosniff"));
        }
        let font = a.handle(&req("GET", "/fonts/figtree-latin-wght-normal.woff2", ""));
        assert_eq!(&font.raw.as_ref().unwrap()[..4], b"wOF2");
        assert_eq!(a.handle(&req("GET", "/../Cargo.toml", "")).status, 404);
        assert_eq!(a.handle(&req("GET", "/rpc", "")).status, 404);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn la_csp_n_autorise_ni_script_en_ligne_ni_site_tiers() {
        assert!(!CSP.contains("unsafe-inline") && !CSP.contains("unsafe-eval"));
        assert!(CSP.contains("script-src 'self'") && CSP.contains("https://images.igdb.com"));
        let html = std::str::from_utf8(ASSETS[0].2).unwrap();
        assert!(!html.contains("style=\"") && !html.contains("onclick"), "pas de code ni style en ligne dans la page");
        assert!(!html.contains("http://") && !html.contains("https://"), "aucune ressource externe");
    }

    #[test]
    fn rpc_exige_post_et_en_tete_personnalise() {
        let (a, dir) = app("w2");
        assert_eq!(a.handle(&req("POST", "/rpc/status", "")).status, 403);
        assert_eq!(a.handle(&req("GET", "/rpc/status", "").with_header("X-Panda", "1")).status, 405);
        let ok = a.handle(&req("POST", "/rpc/status", "").with_header("X-Panda", "1"));
        assert_eq!(ok.status, 200);
        let v: Value = serde_json::from_str(&ok.body).unwrap();
        assert_eq!((v["ok"].as_bool(), v["result"]["mode"].as_str()), (Some(true), Some("offline")));
        let bad = a.handle(&req("POST", "/rpc/search", "{pas du json").with_header("X-Panda", "1"));
        assert_eq!(bad.status, 400);
        let unknown = a.handle(&req("POST", "/rpc/nimporte", "{}").with_header("X-Panda", "1"));
        assert_eq!(serde_json::from_str::<Value>(&unknown.body).unwrap()["ok"], false);
        assert_eq!(a.handle(&req("DELETE", "/", "")).status, 405);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuse_hote_et_origine_etrangers() {
        let (a, dir) = app("w3");
        let rebinding = Request::new("POST", "/rpc/status", "{}").with_header("Host", "evil.example:4000").with_header("X-Panda", "1");
        assert_eq!(a.handle(&rebinding).status, 403);
        assert_eq!(a.handle(&Request::new("GET", "/", "")).status, 403, "sans Host");
        assert_eq!(a.handle(&Request::new("GET", "/", "").with_header("Host", "127.0.0.1:9999")).status, 403, "mauvais port");
        let cross = req("POST", "/rpc/status", "{}").with_header("X-Panda", "1").with_header("Origin", "https://evil.example");
        assert_eq!(a.handle(&cross).status, 403);
        let same = req("POST", "/rpc/status", "{}").with_header("X-Panda", "1").with_header("Origin", "http://127.0.0.1:4000");
        assert_eq!(a.handle(&same).status, 200);
        assert_eq!(a.handle(&Request::new("GET", "/", "").with_header("Host", "LOCALHOST:4000")).status, 200);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serveur_tcp_reel_connexions_paralleles_et_requete_lente() {
        let dir = temp_dir("w4");
        let svc = Arc::new(Service::open(&dir, FakeTransport::with(vec![])).unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Arc::new(WebApp::new(svc, port));
        let server = std::thread::spawn(move || serve(&listener, app, Some(3)).unwrap());
        // connexion de réserve qui n'envoie rien (comportement des navigateurs) : ne doit pas bloquer les autres
        let _idle = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let fetch = |raw: String| {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(raw.as_bytes()).unwrap();
            let mut out = Vec::new();
            s.read_to_end(&mut out).unwrap();
            String::from_utf8_lossy(&out).into_owned()
        };
        let page = fetch(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"));
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("Backlog Panda") && page.contains("Content-Security-Policy"));
        let body = r#"{"query":"hades"}"#;
        let rpc = fetch(format!("POST /rpc/search HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Panda: 1\r\nContent-Length: {}\r\n\r\n{body}", body.len()));
        assert!(rpc.contains("200 OK") && rpc.contains("\"ok\":true") && rpc.contains("Hades"), "{rpc}");
        drop(_idle);
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
