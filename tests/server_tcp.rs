//! Test de bout en bout par une vraie socket TCP (127.0.0.1, port éphémère).

use backlog_panda::fixtures::import_catalog;
use backlog_panda::library::{Library, Status};
use backlog_panda::server::{serve, App};
use backlog_panda::sync::apply_response;
use backlog_panda::turbo::{next_page, parse_results, parse_turbo_streams};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;

fn start(app: App, requests: usize) -> (SocketAddr, thread::JoinHandle<App>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let mut app = app;
        serve(&listener, &mut app, Some(requests)).unwrap();
        app
    });
    (addr, handle)
}

/// Retourne (code, en-têtes, corps).
fn http(addr: SocketAddr, method: &str, target: &str, body: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    write!(s, "{method} {target} HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    let code = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (code, head.to_string(), body.to_string())
}

fn app() -> App {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/evidence");
    App::new(import_catalog(&dir).unwrap(), Library::new("serveur"))
}

#[test]
fn autocomplete_et_pagination_par_socket() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let (addr, handle) = start(app(), 5);

    let (code, head, body) = http(addr, "GET", "/autocomplete.json?filter_editions=true&query=zleda", "");
    assert_eq!(code, 200);
    assert!(head.contains("application/json"));
    let n_suggestions = body.matches("\"slug\"").count();
    assert_eq!(n_suggestions, 21);
    assert_eq!(http(addr, "GET", "/autocomplete.json?query=z", "").2, "{\"suggestions\":[]}");

    // Page 1 de la recherche : 11 cartes et un cadre paresseux vers la page 2.
    let (code, _, html) = http(addr, "GET", "/search/games/zelda/", "");
    assert_eq!(code, 200);
    assert_eq!(parse_results(&html).len(), 11);
    assert_eq!(next_page(&html), Some(2));

    // Le navigateur suivrait le `src` du cadre : Turbo Streams page 2, qui chaîne la page 3.
    let (code, head, body) = http(addr, "GET", "/search/results.turbo_stream?page=2&query=zelda&type=games", "");
    assert_eq!(code, 200);
    assert!(head.contains("text/vnd.turbo-stream.html"));
    let streams = parse_turbo_streams(&body);
    assert_eq!(parse_results(&streams[0].template).len(), 11);
    assert_eq!(next_page(&streams[1].template), Some(3));

    // Dernière page connue : plus de cadre paresseux (comportement réel NON DÉTERMINÉ, voir rapport).
    let (_, _, body) = http(addr, "GET", "/search/results.turbo_stream?page=99&query=zelda&type=games", "");
    assert_eq!(parse_results(&body).len(), 0);
    handle.join().unwrap();
}

#[test]
fn bibliotheque_et_synchronisation_par_socket() {
    if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/evidence").is_dir() {
        eprintln!("extraits docs/evidence absents (non publiés) : test ignoré");
        return;
    }
    let (addr, handle) = start(app(), 4);
    let id = 1028; // « The Legend of Zelda: Link's Awakening » (1993), dans le catalogue importé

    // Un appareil hors ligne note un jeu, puis se synchronise.
    let mut phone = Library::new("telephone");
    phone.upsert(id, Status::Playing, None);
    phone.set_rating(id, Some(backlog_panda::rating::Rating::from_stars(4.5).unwrap())).unwrap();

    // Pendant ce temps, le navigateur modifie un autre jeu directement sur le serveur.
    let (code, _, _) = http(addr, "PUT", "/api/library/1027", r#"{"status":"backlog"}"#);
    assert_eq!(code, 200);
    let (code, _, _) = http(addr, "PUT", "/api/library/424242", r#"{"status":"backlog"}"#);
    assert_eq!(code, 404);

    let (code, _, response) = http(addr, "POST", "/api/sync", &serde_json::to_string(&phone).unwrap());
    assert_eq!(code, 200);
    let report = apply_response(&mut phone, &response).unwrap();
    assert_eq!(report.adopted, 1);
    assert_eq!(phone.live().count(), 2);

    let (_, _, stats) = http(addr, "GET", "/api/library/stats", "");
    assert!(stats.contains("\"average\":4.5"), "{stats}");

    let server = handle.join().unwrap();
    assert_eq!(server.library.entries, phone.entries);
}
