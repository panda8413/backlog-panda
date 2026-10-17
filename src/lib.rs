//! Backlog Panda — application minimale de gestion de backlog de jeux vidéo.
//!
//! Les modules `catalog`, `search`, `rating`, `turbo`, `rails_json` reproduisent ce qui a été
//! OBSERVÉ sur backloggd.com (voir `docs/REPORT.md` et `docs/evidence/`). Les modules `library`,
//! `sync`, `http`, `server` sont une CONCEPTION PROPRE : le stockage et la synchronisation
//! authentifiés de Backloggd n'ont pas pu être observés.

pub mod catalog;
pub mod config;
pub mod csv_import;
pub mod deepl;
pub mod error;
pub mod fixtures;
pub mod http;
pub mod igdb;
pub mod l10n;
pub mod library;
pub mod rails_json;
pub mod rating;
pub mod search;
pub mod secrets;
pub mod server;
pub mod service;
pub mod store;
pub mod sync;
pub mod text;
pub mod transfer;
pub mod turbo;
pub mod webapp;

pub use error::{Error, Result};
