//! Type d'erreur unique de la bibliothèque.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Note hors de 0,5–5,0 ou non multiple de 0,5.
    InvalidRating(String),
    /// Identifiant de jeu absent du catalogue.
    UnknownGame(u64),
    /// Jeu absent de la bibliothèque (ou supprimé).
    NotInLibrary(u64),
    /// Identifiant déjà présent dans le catalogue.
    DuplicateGame(u64),
    /// Statut inconnu.
    InvalidStatus(String),
    /// Document (HTML, JSON, requête HTTP) mal formé.
    Parse(String),
    Io(String),
    Json(String),
    /// Réponse HTTP d'erreur d'un service distant (statut, début du corps).
    Http(u16, String),
    /// Échec réseau (DNS, connexion, délai…), sans URL ni secret.
    Network(String),
    /// Clés IGDB non renseignées.
    NotConfigured,
    /// Argument d'appel invalide.
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidRating(s) => write!(f, "note invalide : {s} (attendu : 0,5 à 5,0 par pas de 0,5)"),
            Error::UnknownGame(id) => write!(f, "jeu inconnu du catalogue : {id}"),
            Error::NotInLibrary(id) => write!(f, "jeu absent de la bibliothèque : {id}"),
            Error::DuplicateGame(id) => write!(f, "identifiant déjà présent : {id}"),
            Error::InvalidStatus(s) => write!(f, "statut invalide : {s}"),
            Error::Parse(s) => write!(f, "document mal formé : {s}"),
            Error::Io(s) => write!(f, "erreur d'E/S : {s}"),
            Error::Json(s) => write!(f, "JSON invalide : {s}"),
            Error::Http(code, body) => write!(f, "le service distant a répondu {code} : {body}"),
            Error::Network(s) => write!(f, "réseau indisponible : {s}"),
            Error::NotConfigured => write!(f, "clés IGDB non renseignées"),
            Error::Invalid(s) => write!(f, "argument invalide : {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e.to_string())
    }
}
