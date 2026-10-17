//! Notes en demi-étoiles et histogramme des notes.
//!
//! OBSERVÉ (E08, jeu 7346, 10 compteurs) :
//! - 10 barres, de 0,5 ★ à 5,0 ★ ; liens `…/plays/rating:N;/` avec N = 1..10 (= nombre de demi-étoiles) ;
//! - infobulle : `"<n> | <étoiles> ★ Ratings (<p>%)"` avec p = n / Σn × 100, une décimale ;
//! - hauteur de barre = n / max(n) × 100 (en %) ;
//! - moyenne affichée/JSON-LD = Σ(demi-étoiles × n) / 2 / Σn, identique au bit près à `ratingValue`
//!   (E13 : 4.462687817015983) ; le dénominateur est la somme de l'histogramme (125388) et non `ratingCount` (125431).
//!
//! NON DÉTERMINÉ : seuil exact de la classe CSS `empty` (observé entre 0,636 % et 1,964 %) ;
//! la valeur 1,0 % ci-dessous est une hypothèse.

use crate::error::{Error, Result};
use crate::text::html_escape;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const EMPTY_BAR_THRESHOLD_PERCENT: f64 = 1.0;

/// Note en demi-étoiles, de 1 (0,5 ★) à 10 (5,0 ★).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Rating(u8);

impl Rating {
    pub fn from_half_stars(n: u8) -> Result<Self> {
        if (1..=10).contains(&n) {
            Ok(Rating(n))
        } else {
            Err(Error::InvalidRating(format!("{n} demi-étoiles")))
        }
    }

    pub fn from_stars(stars: f64) -> Result<Self> {
        let doubled = stars * 2.0;
        if !doubled.is_finite() || doubled.fract() != 0.0 || !(1.0..=10.0).contains(&doubled) {
            return Err(Error::InvalidRating(stars.to_string()));
        }
        Ok(Rating(doubled as u8))
    }

    pub fn half_stars(self) -> u8 {
        self.0
    }

    pub fn stars(self) -> f64 {
        f64::from(self.0) / 2.0
    }
}

impl TryFrom<u8> for Rating {
    type Error = Error;
    fn try_from(n: u8) -> Result<Self> {
        Rating::from_half_stars(n)
    }
}

impl From<Rating> for u8 {
    fn from(r: Rating) -> u8 {
        r.0
    }
}

impl fmt::Display for Rating {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1}", self.stars())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Histogram {
    /// `counts[i]` = nombre de notes de `(i + 1) / 2` étoiles.
    pub counts: [u64; 10],
}

impl Histogram {
    pub fn new(counts: [u64; 10]) -> Self {
        Histogram { counts }
    }

    pub fn add(&mut self, r: Rating) {
        self.counts[usize::from(r.half_stars()) - 1] += 1;
    }

    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }

    pub fn average(&self) -> Option<f64> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        let weighted: u64 = self.counts.iter().enumerate().map(|(i, n)| (i as u64 + 1) * n).sum();
        Some(weighted as f64 / 2.0 / total as f64)
    }

    /// Pourcentage affiché dans l'infobulle.
    pub fn percent(&self, i: usize) -> f64 {
        match self.total() {
            0 => 0.0,
            t => self.counts[i] as f64 / t as f64 * 100.0,
        }
    }

    /// Hauteur de barre en % du maximum.
    pub fn bar_heights(&self) -> [f64; 10] {
        let max = self.counts.iter().copied().max().unwrap_or(0);
        let mut out = [0.0; 10];
        if max > 0 {
            for (o, &n) in out.iter_mut().zip(&self.counts) {
                *o = n as f64 / max as f64 * 100.0;
            }
        }
        out
    }

    /// Texte d'infobulle, au format observé.
    pub fn tooltip(&self, i: usize) -> String {
        format!("{} | {:.1} ★ Ratings ({:.1}%)", self.counts[i], (i + 1) as f64 / 2.0, self.percent(i))
    }

    /// Balisage des barres, au format observé (`col px-0 top-tooltip` > `a` > `div.bar-h`).
    pub fn render_bars(&self, game_slug: &str) -> String {
        let heights = self.bar_heights();
        let mut out = String::from("<div class=\"row mx-0\" id=\"ratings-bars-height\">\n");
        for (i, &height) in heights.iter().enumerate() {
            let empty = if height < EMPTY_BAR_THRESHOLD_PERCENT { "empty" } else { "" };
            out.push_str(&format!(
                "\t<div class=\"col px-0 top-tooltip\" data-tippy-content=\"{}\">\n\t\t<a class=\"d-block h-100\" href=\"/logs/{}/plays/rating:{};/\" data-turbo-frame=\"_top\">\n\t\t\t<div class=\"bar-h {}\" style=\"height: {:?}%;\"></div>\n\t\t</a>\n\t</div>\n",
                html_escape(&self.tooltip(i)),
                html_escape(game_slug),
                i + 1,
                empty,
                height,
            ));
        }
        out.push_str("</div>\n");
        out
    }

    /// Relit les 10 compteurs à partir des attributs `data-tippy-content` (format observé).
    /// Les barres sont repérées par leur étoile (0,5…5,0), pas par leur position.
    pub fn parse_tooltips(html: &str) -> Result<Histogram> {
        let marker = "data-tippy-content=\"";
        let mut counts = [None::<u64>; 10];
        let mut rest = html;
        while let Some(i) = rest.find(marker) {
            rest = &rest[i + marker.len()..];
            let end = rest.find('"').ok_or_else(|| Error::Parse("infobulle non terminée".into()))?;
            let content = &rest[..end];
            rest = &rest[end..];
            let Some((n, tail)) = content.split_once('|') else { continue };
            let Some((stars, _)) = tail.split_once('★') else { continue };
            let (Ok(n), Ok(stars)) = (n.trim().parse::<u64>(), stars.trim().parse::<f64>()) else { continue };
            let Ok(r) = Rating::from_stars(stars) else { continue };
            let slot = &mut counts[usize::from(r.half_stars()) - 1];
            if slot.is_none() {
                *slot = Some(n);
            }
        }
        let mut out = [0u64; 10];
        for (i, c) in counts.iter().enumerate() {
            out[i] = c.ok_or_else(|| Error::Parse(format!("barre {} absente", (i + 1) as f64 / 2.0)))?;
        }
        Ok(Histogram { counts: out })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bornes_et_pas_de_demi_etoile() {
        assert!(Rating::from_stars(0.5).is_ok());
        assert!(Rating::from_stars(5.0).is_ok());
        for bad in [0.0, 0.25, 5.5, -1.0, f64::NAN, f64::INFINITY, 4.7] {
            assert!(Rating::from_stars(bad).is_err(), "{bad}");
        }
        assert!(Rating::from_half_stars(0).is_err());
        assert!(Rating::from_half_stars(11).is_err());
        assert_eq!(Rating::from_stars(4.5).unwrap().half_stars(), 9);
        assert_eq!(Rating::from_half_stars(7).unwrap().to_string(), "3.5");
    }

    #[test]
    fn serde_en_demi_etoiles_et_validation() {
        let r: Rating = serde_json::from_str("9").unwrap();
        assert_eq!(r.stars(), 4.5);
        assert!(serde_json::from_str::<Rating>("11").is_err());
        assert_eq!(serde_json::to_string(&r).unwrap(), "9");
    }

    #[test]
    fn moyenne_et_vide() {
        let mut h = Histogram::default();
        assert_eq!(h.average(), None);
        h.add(Rating::from_stars(4.0).unwrap());
        h.add(Rating::from_stars(5.0).unwrap());
        assert_eq!(h.average(), Some(4.5));
        assert_eq!(h.total(), 2);
    }

    #[test]
    fn rendu_puis_relecture() {
        let h = Histogram::new([258, 333, 384, 1186, 1722, 4433, 8218, 20502, 27955, 60397]);
        let html = h.render_bars("zelda");
        assert_eq!(Histogram::parse_tooltips(&html).unwrap(), h);
        assert!(html.contains("rating:10;"));
        assert!(html.contains("height: 100.0%;"));
        assert!(Histogram::parse_tooltips("<p>rien</p>").is_err());
    }
}
