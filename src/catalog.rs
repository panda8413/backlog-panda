//! Catalogue de jeux (équivalent des fiches `/games/<slug>/`).
//!
//! OBSERVÉ (E05) : chaque jeu a un `id` numérique, un `slug` stocké, un `title`, une `year` en chaîne
//! (« TBD » possible). Les doublons de slug reçoivent le suffixe `--1` (un seul cas observé, `--2`+ DÉDUIT).
//! Certaines lignes sont des « éditions » rattachées à un jeu parent (voir `filter_editions`).

use crate::error::{Error, Result};
use crate::search::Suggestion;
use crate::text::slugify;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub id: u64,
    pub slug: String,
    pub title: String,
    pub year: String,
    /// Identifiant du jeu parent si cette ligne est une édition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition_of: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    games: Vec<Game>,
    #[serde(skip)]
    by_id: HashMap<u64, usize>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn games(&self) -> &[Game] {
        &self.games
    }

    pub fn len(&self) -> usize {
        self.games.len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    pub fn get(&self, id: u64) -> Option<&Game> {
        self.by_id.get(&id).map(|&i| &self.games[i])
    }

    pub fn get_by_slug(&self, slug: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.slug == slug)
    }

    pub fn insert(&mut self, game: Game) -> Result<()> {
        if self.by_id.contains_key(&game.id) {
            return Err(Error::DuplicateGame(game.id));
        }
        self.by_id.insert(game.id, self.games.len());
        self.games.push(game);
        Ok(())
    }

    /// Slug unique : `base`, puis `base--1`, `base--2`, …
    pub fn unique_slug(&self, title: &str) -> String {
        let base = slugify(title);
        if self.get_by_slug(&base).is_none() {
            return base;
        }
        (1..).map(|n| format!("{base}--{n}")).find(|s| self.get_by_slug(s).is_none()).unwrap()
    }

    /// Ajoute un jeu local (identifiant = max + 1).
    pub fn add(&mut self, title: &str, year: &str) -> &Game {
        let id = self.games.iter().map(|g| g.id).max().unwrap_or(0) + 1;
        let slug = self.unique_slug(title);
        self.insert(Game { id, slug, title: title.to_string(), year: year.to_string(), edition_of: None })
            .expect("id frais");
        self.games.last().unwrap()
    }

    /// Absorbe des suggestions d'autocomplétion : un identifiant déjà connu n'est jamais écrasé.
    /// Retourne le nombre de jeux ajoutés.
    pub fn absorb(&mut self, suggestions: &[Suggestion]) -> usize {
        let mut added = 0;
        for s in suggestions {
            let game = Game {
                id: s.data.id,
                slug: s.data.slug.clone(),
                title: s.data.title.clone(),
                year: s.data.year.clone(),
                edition_of: None,
            };
            if self.insert(game).is_ok() {
                added += 1;
            }
        }
        added
    }

    /// Détecte les éditions en comparant deux réponses à la même requête : la variante non filtrée
    /// (`filter_editions=false`) et la variante filtrée (`true`). Une ligne dont l'id diffère à la même position
    /// est une édition ; l'id de la ligne filtrée est celui du parent. DÉDUIT de E05 (cas « Zelda: Link's » seulement).
    pub fn mark_editions(&mut self, unfiltered: &[Suggestion], filtered: &[Suggestion]) -> usize {
        let mut marked = 0;
        for (u, f) in unfiltered.iter().zip(filtered) {
            if u.data.id != f.data.id {
                if let Some(&i) = self.by_id.get(&u.data.id) {
                    self.games[i].edition_of = Some(f.data.id);
                    marked += 1;
                }
            }
        }
        marked
    }

    pub fn reindex(&mut self) {
        self.by_id = self.games.iter().enumerate().map(|(i, g)| (g.id, i)).collect();
    }

    pub fn from_json(s: &str) -> Result<Self> {
        let mut c: Catalog = serde_json::from_str(s)?;
        c.reindex();
        Ok(c)
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.to_json()?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: u64, slug: &str, title: &str) -> Game {
        Game { id, slug: slug.into(), title: title.into(), year: "2000".into(), edition_of: None }
    }

    #[test]
    fn insert_refuse_les_doublons_d_id() {
        let mut c = Catalog::new();
        c.insert(game(1, "a", "A")).unwrap();
        assert_eq!(c.insert(game(1, "b", "B")), Err(Error::DuplicateGame(1)));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn slugs_uniques_avec_suffixe() {
        let mut c = Catalog::new();
        assert_eq!(c.add("Hades", "2020").slug, "hades");
        assert_eq!(c.add("Hades", "2001").slug, "hades--1");
        assert_eq!(c.add("Hades", "1999").slug, "hades--2");
        assert_eq!(c.add("Hades", "1999").id, 4);
    }

    #[test]
    fn json_aller_retour_et_index() {
        let mut c = Catalog::new();
        c.add("Hades", "2020");
        let back = Catalog::from_json(&c.to_json().unwrap()).unwrap();
        assert_eq!(back.get(1).unwrap().title, "Hades");
        assert_eq!(back, c);
    }
}
