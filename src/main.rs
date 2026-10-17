//! CLI : `app` (application de bureau dans le navigateur), `import-fixtures`, `search`, `lib`, `serve`.

use backlog_panda::catalog::Catalog;
use backlog_panda::library::{Library, Status};
use backlog_panda::rating::Rating;
use backlog_panda::search::autocomplete;
use backlog_panda::igdb::UreqTransport;
use backlog_panda::server::{serve, App};
use backlog_panda::service::Service;
use backlog_panda::store::default_data_dir;
use backlog_panda::webapp::{self, WebApp};
use backlog_panda::{rails_json, Error, Result};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const USAGE: &str = "usage :
  backlog-panda [app] [--no-open] [--demo] [--data-dir <dossier>] [--port <n>]
  backlog-panda import-fixtures <dossier_preuves> <catalogue.json>
  backlog-panda search <catalogue.json> <requête> [--toutes-editions]
  backlog-panda lib <bibliotheque.json> <catalogue.json> add <id> <statut> [étoiles]
  backlog-panda lib <bibliotheque.json> <catalogue.json> rate <id> <étoiles>
  backlog-panda lib <bibliotheque.json> <catalogue.json> rm <id>
  backlog-panda lib <bibliotheque.json> <catalogue.json> list
  backlog-panda serve <catalogue.json> <bibliotheque.json> [adresse (défaut 127.0.0.1:8080)]";

fn load_library(path: &Path) -> Result<Library> {
    if path.exists() {
        Library::load(path)
    } else {
        Ok(Library::new("local"))
    }
}

/// Port habituel de l'application : s'il est déjà pris, une instance tourne et on la rouvre
/// (deux instances écriraient dans le même fichier de bibliothèque).
const APP_PORT: u16 = 47821;

fn open_url(url: &str) {
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if result.is_err() {
        eprintln!("Ouvre {url} dans ton navigateur.");
    }
}

fn app(opts: &[&str]) -> Result<()> {
    let (mut no_open, mut demo, mut dir, mut port) = (false, false, None, None);
    let mut it = opts.iter();
    while let Some(o) = it.next() {
        match *o {
            "--no-open" => no_open = true,
            "--demo" => demo = true,
            "--data-dir" => dir = Some(PathBuf::from(it.next().ok_or_else(|| Error::Parse(USAGE.into()))?)),
            "--port" => {
                let p = it.next().and_then(|p| p.parse::<u16>().ok());
                port = Some(p.ok_or_else(|| Error::Parse("--port attend un numéro de port".into()))?);
            }
            _ => return Err(Error::Parse(USAGE.into())),
        }
    }
    let dir = dir.unwrap_or_else(|| if demo { std::env::temp_dir().join("backlog-panda-demo") } else { default_data_dir() });
    let listener = match TcpListener::bind(("127.0.0.1", port.unwrap_or(APP_PORT))) {
        Ok(l) => l,
        Err(e) if port.is_none() && e.kind() == std::io::ErrorKind::AddrInUse => {
            let url = format!("http://127.0.0.1:{APP_PORT}");
            eprintln!("Backlog Panda tourne déjà : {url}");
            if !no_open {
                open_url(&url);
            }
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    let port = listener.local_addr()?.port();
    let service = Arc::new(Service::open(&dir, Arc::new(UreqTransport::new()?))?);
    if demo {
        service.seed_demo()?;
    }
    let url = format!("http://127.0.0.1:{port}");
    eprintln!("Backlog Panda est prêt : {url}\nDonnées : {}\nFerme cette fenêtre pour quitter.", dir.display());
    if !no_open {
        open_url(&url);
    }
    webapp::serve(&listener, Arc::new(WebApp::new(service, port)), None)?;
    Ok(())
}

fn run(args: &[String]) -> Result<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        [] => app(&[])?,
        ["app", opts @ ..] => app(opts)?,
        [flag, ..] if flag.starts_with("--") => app(&a)?,
        ["import-fixtures", dir, out] => {
            let c = backlog_panda::fixtures::import_catalog(Path::new(dir))?;
            c.save(Path::new(out))?;
            println!("{} jeux écrits dans {out}", c.len());
        }
        ["search", cat, query, rest @ ..] => {
            let c = Catalog::load(Path::new(cat))?;
            let filter = !rest.contains(&"--toutes-editions");
            println!("{}", rails_json::to_string(&autocomplete(&c, query, filter))?);
        }
        ["lib", lib, cat, cmd @ ..] => {
            let (lib_path, c) = (Path::new(lib), Catalog::load(Path::new(cat))?);
            let mut l = load_library(lib_path)?;
            let id = |s: &str| s.parse::<u64>().map_err(|_| Error::Parse(format!("identifiant invalide : {s}")));
            let stars = |s: &str| s.parse::<f64>().map_err(|_| Error::InvalidRating(s.into())).and_then(Rating::from_stars);
            match cmd {
                ["add", i, status, rest @ ..] => {
                    let i = id(i)?;
                    c.get(i).ok_or(Error::UnknownGame(i))?;
                    l.upsert(i, status.parse::<Status>()?, rest.first().map(|s| stars(s)).transpose()?);
                }
                ["rate", i, s] => l.set_rating(id(i)?, Some(stars(s)?))?,
                ["rm", i] => l.remove(id(i)?)?,
                ["list"] => {
                    for e in l.live() {
                        let title = c.get(e.game_id).map_or("?", |g| g.title.as_str());
                        let note = e.rating.map_or("-".to_string(), |r| r.to_string());
                        println!("{:>7}  {:<10?}  {:>3} ★  {title}", e.game_id, e.status, note);
                    }
                    return Ok(());
                }
                _ => return Err(Error::Parse(USAGE.into())),
            }
            l.save(lib_path)?;
        }
        ["serve", cat, lib, rest @ ..] => {
            let addr = rest.first().copied().unwrap_or("127.0.0.1:8080");
            let mut app = App::new(Catalog::load(Path::new(cat))?, load_library(Path::new(lib))?);
            app.library_path = Some(PathBuf::from(lib));
            let listener = TcpListener::bind(addr)?;
            eprintln!("écoute sur http://{}", listener.local_addr()?);
            serve(&listener, &mut app, None)?;
        }
        _ => return Err(Error::Parse(USAGE.into())),
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
