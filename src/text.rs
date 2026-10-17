//! Utilitaires de texte : normalisation de recherche, slugs, échappements HTML et URL.

/// Normalisation utilisée pour la recherche : minuscules, caractères alphanumériques uniquement.
///
/// OBSERVÉ (docs/evidence/E05_*) : « zleda » trouve « Puzzle-Dama » (« puzzledama »), « ze » trouve
/// « F-Zero » (« fzero »), « Zelda: Link's » trouve « Link's Awakening ». La règle exacte côté
/// serveur n'est pas observable ; celle-ci est la plus simple qui explique ces observations.
pub fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Slug d'un titre. DÉDUIT/APPROCHÉ : sur 162 jeux observés, 156 slugs de Backloggd correspondent à
/// cette règle (hors suffixe `--N`). Les 6 écarts montrent que les slugs sont des valeurs stockées,
/// pas recalculées (ex. `link-s-awakening-dx`, `hades-zero`, `ratchet-clank-...`).
pub fn slugify(title: &str) -> String {
    let s = title
        .to_lowercase()
        .replace('&', " and ")
        .replace('+', " plus ")
        .replace(['\'', '’', '`'], "");
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

pub fn html_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.find(';').filter(|&j| j <= 10).and_then(|j| {
            let name = &rest[1..j];
            let ch = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                n if n.starts_with("#x") || n.starts_with("#X") => {
                    u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32)
                }
                n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            ch.map(|c| (c, j))
        });
        match decoded {
            Some((c, j)) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Décodage « application/x-www-form-urlencoded » (`+` = espace, `%XX`).
pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_garde_alphanumerique_en_minuscules() {
        assert_eq!(normalize("Puzzle-Dama"), "puzzledama");
        assert_eq!(normalize("F-Zero 99"), "fzero99");
        assert_eq!(normalize("  Zelda: Link's "), "zeldalinks");
        assert_eq!(normalize("É-T"), "ét");
    }

    #[test]
    fn slugify_cas_observes() {
        assert_eq!(slugify("The Legend of Zelda: Link's Awakening"), "the-legend-of-zelda-links-awakening");
        assert_eq!(
            slugify("The Legend of Zelda: A Link to the Past & Four Swords"),
            "the-legend-of-zelda-a-link-to-the-past-and-four-swords"
        );
        assert_eq!(slugify("Hades' Star"), "hades-star");
        assert_eq!(slugify("  --  "), "");
    }

    #[test]
    fn html_roundtrip() {
        let s = "Champions' Ballad & <b>\"x\"</b>";
        assert_eq!(html_unescape(&html_escape(s)), s);
        assert_eq!(html_unescape("a&#39;b &amp; c &#x41; &inconnu; &"), "a'b & c A &inconnu; &");
    }

    #[test]
    fn url_roundtrip() {
        assert_eq!(url_decode("zelda+link%27s"), "zelda link's");
        assert_eq!(url_decode("100%"), "100%");
        assert_eq!(url_decode("%zz"), "%zz");
        assert_eq!(url_decode(&url_encode("Zelda: Link's é")), "Zelda: Link's é");
    }
}
