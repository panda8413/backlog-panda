//! Protocole de synchronisation — CONCEPTION PROPRE (le protocole réel de Backloggd n'a pas été observé).
//!
//! État complet échangé en un aller-retour : le client envoie sa bibliothèque, le serveur fusionne et
//! répond avec la sienne ; le client fusionne la réponse. Les deux côtés convergent.

use crate::error::Result;
use crate::library::{Library, MergeReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    pub server: MergeReport,
    pub client: MergeReport,
}

/// Côté serveur : fusionne l'état reçu, retourne l'état fusionné à renvoyer.
pub fn handle_sync(server: &mut Library, body: &str) -> Result<(String, MergeReport)> {
    let incoming: Library = serde_json::from_str(body)?;
    let report = server.merge(&incoming);
    Ok((serde_json::to_string(server)?, report))
}

/// Côté client : fusionne la réponse du serveur.
pub fn apply_response(client: &mut Library, body: &str) -> Result<MergeReport> {
    let remote: Library = serde_json::from_str(body)?;
    Ok(client.merge(&remote))
}

/// Échange complet en mémoire (utile pour les tests et la CLI).
pub fn exchange(client: &mut Library, server: &mut Library) -> Result<SyncOutcome> {
    let body = serde_json::to_string(client)?;
    let (response, server_report) = handle_sync(server, &body)?;
    let client_report = apply_response(client, &response)?;
    Ok(SyncOutcome { server: server_report, client: client_report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Status;

    #[test]
    fn deux_appareils_hors_ligne_convergent() {
        let mut server = Library::new("serveur");
        let mut phone = Library::new("telephone");
        let mut pc = Library::new("pc");
        phone.upsert(1, Status::Playing, None);
        pc.upsert(2, Status::Backlog, None);
        exchange(&mut phone, &mut server).unwrap();
        exchange(&mut pc, &mut server).unwrap();
        exchange(&mut phone, &mut server).unwrap();
        assert_eq!(phone.entries, server.entries);
        assert_eq!(pc.entries.len(), 2);
        assert_eq!(phone.entries.len(), 2);
    }

    #[test]
    fn corps_invalide_est_une_erreur() {
        let mut s = Library::new("s");
        assert!(handle_sync(&mut s, "pas du json").is_err());
    }
}
