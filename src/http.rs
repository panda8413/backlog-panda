//! Mini HTTP/1.1 (bibliothèque standard uniquement) : une requête par connexion.

use crate::error::{Error, Result};
use crate::text::url_decode;
use std::io::BufRead;

/// 32 Mio : assez pour importer une grosse bibliothèque (l'import passe par le corps de la requête).
pub const MAX_BODY: usize = 32 << 20;

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    /// En-têtes, noms en minuscules.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Request {
    pub fn new(method: &str, target: &str, body: &str) -> Self {
        let (path, q) = target.split_once('?').unwrap_or((target, ""));
        Request {
            method: method.to_string(),
            path: path.to_string(),
            query: parse_form(q),
            headers: Vec::new(),
            body: body.to_string(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_ascii_lowercase(), value.to_string()));
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    pub fn param(&self, key: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// `a=1&b=x%20y` → paires décodées.
pub fn parse_form(s: &str) -> Vec<(String, String)> {
    s.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (url_decode(k), url_decode(v))
        })
        .collect()
}

pub fn parse_request<R: BufRead>(r: &mut R) -> Result<Request> {
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Err(Error::Parse("connexion fermée avant la requête".into()));
    }
    let mut parts = line.split_whitespace();
    let (method, target) = match (parts.next(), parts.next()) {
        (Some(m), Some(t)) => (m.to_string(), t.to_string()),
        _ => return Err(Error::Parse(format!("ligne de requête invalide : {line:?}"))),
    };
    let mut content_length = 0usize;
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse().map_err(|_| Error::Parse("Content-Length invalide".into()))?;
            }
            if headers.len() < 64 {
                headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
            }
        }
    }
    if content_length > MAX_BODY {
        return Err(Error::Parse("corps trop volumineux".into()));
    }
    let mut body = vec![0u8; content_length];
    r.read_exact(&mut body)?;
    let mut req = Request::new(&method, &target, &String::from_utf8_lossy(&body));
    req.headers = headers;
    Ok(req)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub location: Option<String>,
    pub body: String,
    /// Corps binaire (polices, images) : prioritaire sur `body` s'il est présent.
    pub raw: Option<Vec<u8>>,
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: String) -> Self {
        Response { status, content_type: content_type.to_string(), location: None, body, raw: None, headers: Vec::new() }
    }

    pub fn bytes(status: u16, content_type: &str, data: &[u8]) -> Self {
        let mut r = Self::new(status, content_type, String::new());
        r.raw = Some(data.to_vec());
        r
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn html(body: String) -> Self {
        Self::new(200, "text/html; charset=utf-8", body)
    }

    pub fn json(status: u16, body: String) -> Self {
        Self::new(status, "application/json; charset=utf-8", body)
    }

    pub fn error(status: u16, message: &str) -> Self {
        Self::json(status, serde_json::json!({ "error": message }).to_string())
    }

    pub fn redirect(location: String) -> Self {
        let mut r = Self::new(303, "text/plain", String::new());
        r.location = Some(location);
        r
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            303 => "See Other",
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            _ => "Internal Server Error",
        };
        let len = self.raw.as_ref().map_or(self.body.len(), Vec::len);
        let mut head = format!(
            "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {len}\r\nConnection: close\r\n",
            self.status, self.content_type
        );
        for (k, v) in &self.headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        if let Some(l) = &self.location {
            head.push_str(&format!("Location: {l}\r\n"));
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        match &self.raw {
            Some(raw) => bytes.extend_from_slice(raw),
            None => bytes.extend_from_slice(self.body.as_bytes()),
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requete_get_avec_parametres() {
        let raw = "GET /autocomplete.json?filter_editions=true&query=zelda%3A+link HTTP/1.1\r\nHost: x\r\n\r\n";
        let req = parse_request(&mut raw.as_bytes()).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/autocomplete.json");
        assert_eq!(req.param("query"), Some("zelda: link"));
        assert_eq!(req.param("filter_editions"), Some("true"));
        assert_eq!(req.param("absent"), None);
    }

    #[test]
    fn requete_post_avec_corps() {
        let raw = "POST /search HTTP/1.1\r\ncontent-length: 10\r\n\r\nquery=zeld";
        let req = parse_request(&mut raw.as_bytes()).unwrap();
        assert_eq!(req.body, "query=zeld");
        assert_eq!(parse_form(&req.body), vec![("query".to_string(), "zeld".to_string())]);
    }

    #[test]
    fn requetes_invalides() {
        assert!(parse_request(&mut "".as_bytes()).is_err());
        assert!(parse_request(&mut "GET\r\n\r\n".as_bytes()).is_err());
        assert!(parse_request(&mut "POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n".as_bytes()).is_err());
        assert!(parse_request(&mut "POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nab".as_bytes()).is_err());
        assert!(parse_request(&mut "POST / HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n".as_bytes()).is_err());
        // un import de grosse bibliothèque (plusieurs Mo) doit passer
        let big = "x".repeat(5 << 20);
        let raw = format!("POST /rpc/import_preview HTTP/1.1\r\nContent-Length: {}\r\n\r\n{big}", big.len());
        assert_eq!(parse_request(&mut raw.as_bytes()).unwrap().body.len(), big.len());
    }

    #[test]
    fn en_tetes_lus_et_ecrits() {
        let raw = "GET / HTTP/1.1\r\nHost: 127.0.0.1:1\r\nX-Panda: 1\r\n\r\n";
        let req = parse_request(&mut raw.as_bytes()).unwrap();
        assert_eq!(req.header("HOST"), Some("127.0.0.1:1"));
        assert_eq!(req.header("x-panda"), Some("1"));
        assert_eq!(req.header("absent"), None);
        let b = Response::bytes(200, "font/woff2", &[1, 2, 3]).with_header("X-A", "b").to_bytes();
        assert!(b.ends_with(&[1, 2, 3]));
        assert!(String::from_utf8_lossy(&b).contains("Content-Length: 3\r\n"));
        assert!(String::from_utf8_lossy(&b).contains("X-A: b\r\n"));
    }

    #[test]
    fn reponse_serialisee() {
        let b = String::from_utf8(Response::json(200, "{}".into()).to_bytes()).unwrap();
        assert!(b.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(b.contains("Content-Length: 2\r\n"));
        assert!(b.ends_with("\r\n\r\n{}"));
        let r = String::from_utf8(Response::redirect("/x/".into()).to_bytes()).unwrap();
        assert!(r.contains("303 See Other") && r.contains("Location: /x/\r\n"));
    }
}
