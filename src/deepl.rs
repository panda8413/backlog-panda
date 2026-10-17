//! Traduction du résumé des jeux (anglais -> français) avec l'API DeepL.
//!
//! ÉTAT DE VÉRIFICATION : testé avec un transport simulé uniquement ; jamais appelé contre le vrai
//! service depuis l'environnement de développement. Format de requête (JSON, en-tête
//! `Authorization: DeepL-Auth-Key …`, clés gratuites terminant par `:fx` sur `api-free.deepl.com`)
//! écrit d'après la documentation DeepL telle que je la connais.
//!
//! Seul le texte du résumé du jeu est envoyé. La clé n'apparaît pas dans les messages d'erreur.

use crate::error::{Error, Result};
use crate::igdb::Transport;
use serde_json::{json, Value};
use std::sync::Arc;

pub const FREE_URL: &str = "https://api-free.deepl.com/v2/translate";
pub const PRO_URL: &str = "https://api.deepl.com/v2/translate";
/// Garde-fou : un résumé IGDB fait quelques centaines de caractères.
const MAX_CHARS: usize = 5_000;

pub struct Translator {
    transport: Arc<dyn Transport>,
    key: String,
}

impl Translator {
    pub fn new(transport: Arc<dyn Transport>, key: &str) -> Self {
        Translator { transport, key: key.trim().to_string() }
    }

    fn url(&self) -> &'static str {
        if self.key.ends_with(":fx") {
            FREE_URL
        } else {
            PRO_URL
        }
    }

    fn scrub(&self, s: String) -> String {
        if self.key.is_empty() {
            s
        } else {
            s.replace(&self.key, "***")
        }
    }

    /// Traduit `text` de l'anglais vers le français.
    pub fn to_french(&self, text: &str) -> Result<String> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::Invalid("rien à traduire".into()));
        }
        if text.chars().count() > MAX_CHARS {
            return Err(Error::Invalid("texte trop long pour la traduction".into()));
        }
        let body = json!({ "text": [text], "source_lang": "EN", "target_lang": "FR" }).to_string();
        let auth = format!("DeepL-Auth-Key {}", self.key);
        let resp = self
            .transport
            .post(self.url(), &[("Authorization", &auth), ("Content-Type", "application/json"), ("Accept", "application/json")], &body)
            .map_err(|e| match e {
                Error::Http(403, _) => Error::Invalid("clé DeepL refusée : vérifie-la dans les réglages".into()),
                Error::Http(456, _) => Error::Invalid("quota DeepL du mois épuisé".into()),
                Error::Http(c, m) => Error::Http(c, self.scrub(m)),
                Error::Network(m) => Error::Network(self.scrub(m)),
                other => other,
            })?;
        let v: Value = serde_json::from_str(&resp)?;
        v.get("translations")
            .and_then(|t| t.get(0))
            .and_then(|t| t.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| Error::Parse("réponse DeepL : traduction absente".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::igdb::fake::FakeTransport;

    #[test]
    fn traduit_et_choisit_le_bon_point_d_acces() {
        let t = FakeTransport::with(vec![
            Ok(r#"{"translations":[{"detected_source_language":"EN","text":"Explore Hyrule."}]}"#.into()),
            Ok(r#"{"translations":[{"text":"Bonjour"}]}"#.into()),
        ]);
        let free = Translator::new(t.clone(), " abc:fx ");
        assert_eq!(free.to_french("Explore Hyrule.").unwrap(), "Explore Hyrule.");
        let pro = Translator::new(t.clone(), "pro-key");
        pro.to_french("Hello").unwrap();
        let calls = t.calls();
        assert_eq!(calls[0].0, FREE_URL);
        assert_eq!(calls[1].0, PRO_URL);
        assert!(calls[0].1.contains(&("Authorization".into(), "DeepL-Auth-Key abc:fx".into())));
        assert!(calls[0].1.contains(&("Content-Type".into(), "application/json".into())));
        let body: Value = serde_json::from_str(&calls[0].2).unwrap();
        assert_eq!((body["target_lang"].as_str(), body["text"][0].as_str()), (Some("FR"), Some("Explore Hyrule.")));
        assert!(!calls[0].0.contains("abc"), "la clé ne doit pas figurer dans l'URL");
    }

    #[test]
    fn erreurs_claires_sans_fuite_de_la_cle() {
        for (code, expected) in [(403, "clé DeepL refusée"), (456, "quota DeepL")] {
            let t = FakeTransport::with(vec![Err(Error::Http(code, "x".into()))]);
            let e = Translator::new(t, "k:fx").to_french("Hi").unwrap_err().to_string();
            assert!(e.contains(expected), "{e}");
        }
        let t = FakeTransport::with(vec![Err(Error::Http(500, "boom k:fx boom".into())), Err(Error::Network("échec k:fx".into()))]);
        let tr = Translator::new(t, "k:fx");
        assert!(!tr.to_french("Hi").unwrap_err().to_string().contains("k:fx"));
        assert!(!tr.to_french("Hi").unwrap_err().to_string().contains("k:fx"));
        let tr = Translator::new(FakeTransport::with(vec![Ok("{}".into())]), "k");
        assert!(tr.to_french("Hi").is_err(), "réponse sans traduction");
        assert!(tr.to_french("   ").is_err());
        assert!(tr.to_french(&"a".repeat(MAX_CHARS + 1)).is_err());
    }
}
