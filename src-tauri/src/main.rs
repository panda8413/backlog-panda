//! Coque Tauri : une fenêtre native (WebView2 sous Windows) qui affiche `ui/`.
//!
//! Toute la logique est dans le cœur Rust (`backlog-panda`) ; ce fichier ne fait que relier la
//! commande `rpc` de l'interface à `Service::call`. Aucun serveur HTTP, aucun port ouvert.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use backlog_panda::igdb::UreqTransport;
use backlog_panda::service::Service;
use backlog_panda::store::default_data_dir;
use serde_json::Value;
use std::sync::Arc;
use tauri::{Manager, State};

struct Core(Arc<Service>);

/// `async` : exécutée hors du fil de l'interface, pour que les appels réseau (IGDB) ne la figent pas.
#[tauri::command(async)]
fn rpc(core: State<'_, Core>, method: String, args: Option<Value>) -> Value {
    core.0.call(&method, &args.unwrap_or(Value::Null))
}

fn main() {
    let dir = default_data_dir();
    let transport = UreqTransport::new().unwrap_or_else(|e| fatal(&e.to_string()));
    let service = Service::open(&dir, Arc::new(transport)).unwrap_or_else(|e| fatal(&e.to_string()));

    tauri::Builder::default()
        // Deux instances écriraient dans le même fichier : on remet la première au premier plan.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .manage(Core(Arc::new(service)))
        .invoke_handler(tauri::generate_handler![rpc])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| fatal(&e.to_string()));
}

/// Arrêt sur erreur. En version Windows sans console, rien ne s'afficherait : on écrit `erreur.txt`
/// dans le dossier de données et on ouvre une boîte de message (le texte passe par une variable
/// d'environnement, jamais dans la ligne de commande).
fn fatal(message: &str) -> ! {
    eprintln!("Backlog Panda : {message}");
    let text = format!("Backlog Panda n'a pas pu démarrer.\n\n{message}");
    let dir = default_data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("erreur.txt"), &text);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", "Add-Type -AssemblyName PresentationFramework; [void][System.Windows.MessageBox]::Show($env:BACKLOG_PANDA_MSG, 'Backlog Panda')"])
            .env("BACKLOG_PANDA_MSG", &text)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status();
    }
    std::process::exit(1)
}
