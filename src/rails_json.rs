//! Sérialisation JSON « à la Rails ».
//!
//! OBSERVÉ (E05) : `&` est émis `&` dans les réponses de `/autocomplete.json`, l'apostrophe reste telle quelle.
//! DÉDUIT (comportement documenté d'ActiveSupport, non observé ici) : `<`, `>`, U+2028 et U+2029 sont
//! aussi échappés. Comme ces caractères n'apparaissent jamais en dehors des chaînes JSON, un simple
//! remplacement sur le texte sérialisé est sûr.

use crate::error::Result;
use serde::Serialize;

pub fn escape(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '&' => out.push_str("\\u0026"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

pub fn to_string<T: Serialize>(value: &T) -> Result<String> {
    Ok(escape(&serde_json::to_string(value)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esperluette_echappee_apostrophe_conservee() {
        let v = serde_json::json!({"t": "A & B's <x>"});
        let attendu = format!("{{\"t\":\"A {b}u0026 B's {b}u003cx{b}u003e\"}}", b = '\\');
        assert_eq!(to_string(&v).unwrap(), attendu);
    }

    #[test]
    fn relecture_identique() {
        let s = to_string(&serde_json::json!({"t": "A & B"})).unwrap();
        let back: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(back["t"], "A & B");
    }
}
