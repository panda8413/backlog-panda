# Temps pour finir un jeu : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Objectif :** afficher, trier et filtrer les jeux selon leur durée IGDB (rapide / normal / complet), comparer son temps de jeu à la durée normale, et proposer un lien « Comparer sur HowLongToBeat ».

**Architecture :** `IgdbClient::by_ids` fait une seconde requête `POST /v4/game_time_to_beats` et range le résultat dans `GameInfo.time_to_beat` ; le niveau de détail `REV_FULL` passe à 3, ce qui fait compléter toute la bibliothèque par `sync_metadata`. Le cœur Rust formate les durées et applique tri et filtre ; `ui/app.js` ne fait qu'afficher. La fenêtre Tauri reçoit une commande `open_hltb` qui ouvre l'URL construite côté Rust.

**Stack :** Rust (cœur `backlog-panda`, serde, serde_json), Tauri 2 + `tauri-plugin-opener`, interface HTML/CSS/JS sans dépendance.

**Cahier des charges :** [docs/superpowers/specs/2026-11-05-temps-de-jeu-design.md](../specs/2026-11-05-temps-de-jeu-design.md)

## Contraintes globales

- Travailler sur une branche `feat/temps-de-jeu` (pas sur `main`).
- Source unique : IGDB `POST https://api.igdb.com/v4/game_time_to_beats`, champs `game_id,hastily,normally,completely,count` (secondes). Aucune requête vers howlongtobeat.com.
- Lien HLTB : `https://howlongtobeat.com/?q=` + `text::url_encode(nom)`. Hôte fixe.
- Niveaux : `REV_DETAILS = 2` (fiche détaillée sans durées), `REV_FULL = 3` (détaillée + durées demandées).
- Durée de référence : normale, sinon rapide, sinon complète.
- Arrondi : `< 30 min` → `~30 min` ; `< 10 h` → demi-heure la plus proche (`~4 h 30`, `~2 h`) ; `≥ 10 h` → heure la plus proche (`~25 h`).
- Filtre : `lt5` (< 5 h), `5to15` (5 h ≤ d < 15 h), `15to40` (15 h ≤ d < 40 h), `ge40` (≥ 40 h) ; jeux sans durée exclus de toute tranche.
- « Peu de votes » : `count < 5`.
- Libellés exacts : « Temps pour finir », « Rapide », « Normal », « Complet », « d'après N joueurs (IGDB) », « peu de votes », « Pas de durée connue sur IGDB », « Comparer sur HowLongToBeat », tri « Durée estimée », filtre « Durée » / « Toutes les durées » / « Moins de 5 h » / « 5 à 15 h » / « 15 à 40 h » / « Plus de 40 h ».
- Anciens `bibliotheque.json` lisibles sans migration (`#[serde(default)]`).
- `cargo test` doit rester vert à la fin de chaque tâche (151 tests aujourd'hui + les nouveaux).

## Points d'attention (Review Focus)

1. **« Recharger » quand la requête des durées échoue** : la fiche ne doit pas perdre les durées déjà connues. → test `refresh_garde_les_durees_si_la_requete_echoue` (Tâche 3).
2. **IGDB renvoie `0` pour une durée** : traiter comme absente, jamais « ~0 min ». → test `lecture_durees_zero_et_champs_absents` (Tâche 2).
3. **Nom de jeu avec `&`, `#`, `:`, guillemets ou accents** dans le lien HLTB : l'URL reste sur howlongtobeat.com et la recherche porte sur le nom entier. → test `url_hltb_encodee` (Tâche 3).
4. **IGDB ne renvoie des durées que pour une partie des jeux demandés, ou deux entrées pour un même `game_id`** : chaque jeu reçoit ses durées (la première entrée gagne), les autres passent à `rev = 3` sans durées. → test `by_ids_associe_les_durees_par_game_id` (Tâche 2).
5. **Ancienne bibliothèque (fiches `rev = 2`, pas de champ `time_to_beat`)** : se charge, puis `sync_metadata` la complète ; une fiche `rev = 3` sans durée n'est pas redemandée. → test `sync_complete_les_fiches_rev2_seulement` (Tâche 3).

---

### Tâche 1 : formatage des durées estimées

**Fichiers :**
- Modifier : `src/l10n.rs` (à côté de `duration_fr`, l. 85-93 ; tests dans `mod tests`)

**Interfaces :**
- Produit : `pub fn estimate_fr(secs: u32) -> String` (ex. `"~25 h"`).

- [ ] **Étape 1 : test qui échoue**

```rust
#[test]
fn durees_estimees() {
    assert_eq!(estimate_fr(29 * 60), "~30 min");
    assert_eq!(estimate_fr(60), "~30 min");
    assert_eq!(estimate_fr(4 * 3600 + 20 * 60), "~4 h 30");
    assert_eq!(estimate_fr(2 * 3600 + 10 * 60), "~2 h");
    assert_eq!(estimate_fr(9 * 3600 + 50 * 60), "~10 h");
    assert_eq!(estimate_fr(25 * 3600 + 20 * 60), "~25 h");
    assert_eq!(estimate_fr(25 * 3600 + 30 * 60), "~26 h");
}
```

- [ ] **Étape 2 : lancer** `cargo test --lib l10n::tests::durees_estimees` → échec de compilation (`estimate_fr` inconnu).
- [ ] **Étape 3 : implémenter `estimate_fr`** : arrondir en minutes selon les règles des contraintes globales, puis `format!("~{}", duration_fr(m))`.
- [ ] **Étape 4 : lancer** `cargo test --lib l10n` → tout passe.
- [ ] **Étape 5 : commit** `git commit -am "feat(l10n): format des durées estimées"`

---

### Tâche 2 : durées IGDB dans `GameInfo` et `by_ids`

**Fichiers :**
- Modifier : `src/igdb.rs` (struct `GameInfo` l. 81-124, `Default` l. 130-154, `REV_FULL` l. 157, `post_games` l. 340-359, `run` l. 364-377, `by_ids` l. 387-393, tests)

**Interfaces :**
- Produit :
  ```rust
  #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
  pub struct TimeToBeat { pub hastily: Option<u32>, pub normally: Option<u32>, pub completely: Option<u32>, pub count: u32 }
  impl TimeToBeat { pub fn reference(&self) -> Option<u32> } // normally, sinon hastily, sinon completely
  // GameInfo : #[serde(default)] pub time_to_beat: Option<TimeToBeat>,
  pub const REV_DETAILS: u8 = 2;
  pub const REV_FULL: u8 = 3;
  pub const TTB_URL: &str = "https://api.igdb.com/v4/game_time_to_beats";
  pub fn parse_time_to_beats(json: &str) -> Result<HashMap<u64, TimeToBeat>>;
  ```
- `run` étiquette désormais le palier complet `REV_DETAILS` (plus `REV_FULL`) ; `search` renvoie donc des fiches `rev = 2`.
- `post_games(apicalypse)` devient un appel à `fn post_to(&self, url: &str, apicalypse: &str) -> Result<String>` (même logique jeton / 401 / `scrub`).

- [ ] **Étape 1 : tests qui échouent**

```rust
#[test]
fn lecture_durees_zero_et_champs_absents() {
    let m = parse_time_to_beats(r#"[{"id":1,"game_id":10,"hastily":3600,"normally":0,"count":4},
                                     {"id":2,"game_id":11},{"id":3}]"#).unwrap();
    assert_eq!(m[&10], TimeToBeat { hastily: Some(3600), normally: None, completely: None, count: 4 });
    assert_eq!(m[&10].reference(), Some(3600), "normal absent : on prend rapide");
    assert_eq!(m[&11], TimeToBeat::default());
    assert_eq!(m.len(), 2, "entrée sans game_id ignorée");
}

#[test]
fn by_ids_associe_les_durees_par_game_id() {
    let ttb = r#"[{"game_id":7346,"hastily":180000,"normally":330000,"completely":680000,"count":312},
                  {"game_id":7346,"normally":1,"count":1}]"#;
    let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(GAMES_SAMPLE.into()), Ok(ttb.into())]);
    let c = IgdbClient::new(t.clone(), "id", "s3cr3t");
    let games = c.by_ids(&[7346, 41826, 113112]).unwrap();
    let calls = t.calls();
    assert_eq!(calls[2].0, TTB_URL);
    assert!(calls[2].2.contains("where game_id = (7346,41826,113112)"));
    let zelda = games.iter().find(|g| g.id == 7346).unwrap();
    assert_eq!(zelda.time_to_beat.as_ref().unwrap().normally, Some(330000), "la première entrée gagne");
    assert!(games.iter().all(|g| g.rev == REV_FULL));
    assert!(games.iter().find(|g| g.id == 113112).unwrap().time_to_beat.is_none());
}

#[test]
fn by_ids_sans_durees_si_la_requete_echoue() {
    let t = FakeTransport::with(vec![Ok(TOKEN_OK.into()), Ok(GAMES_SAMPLE.into()), Err(Error::Http(429, "trop".into()))]);
    let games = IgdbClient::new(t, "id", "s").by_ids(&[7346]).unwrap();
    assert!(!games.is_empty());
    assert!(games.iter().all(|g| g.rev == REV_DETAILS && g.time_to_beat.is_none()));
}

#[test]
fn ancienne_fiche_sans_durees() {
    let g: GameInfo = serde_json::from_str(r#"{"id":1,"name":"Hades","slug":"hades","rev":2}"#).unwrap();
    assert_eq!((g.rev, g.time_to_beat), (2, None));
}
```

- [ ] **Étape 2 : lancer** `cargo test --lib igdb` → échec de compilation.
- [ ] **Étape 3 : implémenter.** `parse_time_to_beats` : valeurs `0` ou absentes → `None`, `count` absent → `0`, entrée sans `game_id` ignorée, doublon de `game_id` : garder la première. `by_ids` : après `run`, si les fiches sont `rev == REV_DETAILS`, envoyer `fields game_id,hastily,normally,completely,count; where game_id = ({list}); limit 500;` à `TTB_URL` ; en cas de succès, poser `time_to_beat` (map par `game_id`) et `rev = REV_FULL` sur chaque fiche ; en cas d'erreur, renvoyer les fiches inchangées (`rev = 2`). Paliers de repli (`rev = 1`) : pas de seconde requête.
- [ ] **Étape 4 : corriger les tests existants** de `igdb.rs` : `search` attend désormais `REV_DETAILS` (l. 574) ; les tests qui appellent `by_ids` (`by_ids_construit_la_clause_where`, l. 624) reçoivent une réponse `Ok("[]".into())` supplémentaire pour les durées.
- [ ] **Étape 5 : lancer** `cargo test --lib igdb` → tout passe.
- [ ] **Étape 6 : commit** `git commit -am "feat(igdb): temps de jeu via game_time_to_beats"`

---

### Tâche 3 : fiches, cartes et lien HLTB dans le service

**Fichiers :**
- Modifier : `src/service.rs` (`Card` l. 64-86, `card()` l. 93-127, `keep_translation` l. 133-138 et ses appels l. 720, l. 963, `sync_metadata` l. 941-967, tests)

**Interfaces :**
- Consomme : `TimeToBeat`, `TimeToBeat::reference`, `REV_FULL`, `estimate_fr`, `text::url_encode`.
- Produit (champs JSON de chaque carte, lus par `ui/app.js` en Tâche 6) :
  ```rust
  #[derive(Serialize)]
  struct TtbView {
      hastily: Option<u32>, normally: Option<u32>, completely: Option<u32>,          // secondes
      hastily_text: Option<String>, normally_text: Option<String>, completely_text: Option<String>, // estimate_fr
      count: u32, few_votes: bool,                                                   // few_votes = count < 5
  }
  // Card : ttb: Option<TtbView>, ttb_ref: Option<u32>, ttb_text: Option<String>, hltb_url: String
  pub fn hltb_url(name: &str) -> String; // dans service.rs, `pub` pour la coque Tauri
  ```
  Méthode RPC `hltb_url` : `{ "id": u64 }` → `{ "url": String }` (pour la commande Tauri, Tâche 5).
- `keep_translation` devient `keep_cached(old, new)` : garde la traduction (règle actuelle) **et**, si `new.rev < REV_FULL` et que `old` a des durées, reprend `old.time_to_beat` et `old.rev`.

- [ ] **Étape 1 : tests qui échouent** (dans `mod tests` de `service.rs`, avec les aides existantes `temp_dir`, `ok`, `FakeTransport`, `TOKEN_OK`)

```rust
#[test]
fn url_hltb_encodee() {
    assert_eq!(hltb_url("Hades"), "https://howlongtobeat.com/?q=Hades");
    assert_eq!(hltb_url("Ratchet & Clank: \"Rift\" #2 é"),
               "https://howlongtobeat.com/?q=Ratchet%20%26%20Clank%3A%20%22Rift%22%20%232%20%C3%A9");
}

#[test]
fn carte_avec_durees() {
    // fiche stockée : normally 25 h 20, count 3 → ttb_text "~25 h", few_votes true, hltb_url présent
    // fiche sans durée : ttb null, ttb_ref null, hltb_url présent
}

#[test]
fn refresh_garde_les_durees_si_la_requete_echoue() {
    // fiche stockée rev 3 avec normally Some(90000) ; refresh avec [TOKEN_OK, DETAILED, Err(Network)]
    // → la carte renvoyée a toujours ttb.normally == 90000 et rev == 3
}

#[test]
fn sync_complete_les_fiches_rev2_seulement() {
    // bibliothèque : jeu A rev 2, jeu B rev 3 sans durée
    // sync_metadata → un seul appel games avec "where id = (A)", suivi de l'appel durées
    // second sync_metadata → aucun appel réseau
}
```

Écrire les corps de `carte_avec_durees`, `refresh_garde_les_durees_si_la_requete_echoue` et `sync_complete_les_fiches_rev2_seulement` sur le modèle des tests `refresh` existants (l. 1763-1777), avec exactement les assertions décrites en commentaire.

- [ ] **Étape 2 : lancer** `cargo test --lib service` → échecs attendus.
- [ ] **Étape 3 : implémenter** `TtbView`, les quatre champs de `Card`, `hltb_url`, `keep_cached` (remplace les deux appels à `keep_translation`) et la méthode RPC `hltb_url` (ajoutée au `match` de `call`, l. 488 environ, et à la liste du commentaire de module l. 6).
- [ ] **Étape 4 : corriger les tests existants** qui passent par `by_ids` (refresh l. 1770, sync, import CSV l. 1010) : ajouter une réponse `Ok("[]".into())` après chaque réponse `games` pour que les réponses simulées restent dans l'ordre.
- [ ] **Étape 5 : lancer** `cargo test` (tout le dépôt, `tests/` compris) → tout passe.
- [ ] **Étape 6 : commit** `git commit -am "feat(service): durées et lien HowLongToBeat sur les cartes"`

---

### Tâche 4 : tri « Durée estimée » et filtre « Durée »

**Fichiers :**
- Modifier : `src/service.rs` (`Filters` l. 149-170, `library` l. 614-629, tests)

**Interfaces :**
- Consomme : `TimeToBeat::reference`.
- Produit : argument `sort: "ttb"` et argument `ttb: "lt5" | "5to15" | "15to40" | "ge40"` de la méthode `library` (valeur inconnue ou vide = pas de filtre).

- [ ] **Étape 1 : tests qui échouent**

```rust
#[test]
fn tri_par_duree_estimee() {
    // jeux : A ref 20 h, B sans durée, C ref 3 h, D sans durée (nom "Alpha")
    // library {sort:"ttb"} → ordre C, A, puis D ("Alpha"), B : sans durée à la fin, par nom
}

#[test]
fn filtre_par_tranche_de_duree() {
    // refs : 4 h 59, 5 h, 14 h 59, 15 h, 39 h 59, 40 h, sans durée
    // lt5 → [4 h 59] ; 5to15 → [5 h, 14 h 59] ; 15to40 → [15 h, 39 h 59] ; ge40 → [40 h]
    // "" ou "nimporte" → les 7 jeux
}
```

- [ ] **Étape 2 : lancer** `cargo test --lib service::tests::tri_par_duree_estimee service::tests::filtre_par_tranche_de_duree` → échec.
- [ ] **Étape 3 : implémenter** : champ `ttb: Option<(u32, Option<u32>)>` (bornes en secondes, `[min, max)`) dans `Filters`, lu depuis `args["ttb"]` ; condition ajoutée à `keeps` ; branche `"ttb"` dans le `match sort` avec la clé `(ref.is_none(), ref, nom en minuscules)`.
- [ ] **Étape 4 : lancer** `cargo test` → tout passe.
- [ ] **Étape 5 : commit** `git commit -am "feat(bibliotheque): tri et filtre par durée estimée"`

---

### Tâche 5 : commande Tauri `open_hltb`

**Fichiers :**
- Modifier : `src-tauri/Cargo.toml` (ajouter `tauri-plugin-opener = "2"`), `src-tauri/src/main.rs` (l. 17-37), `Cargo.lock` / `src-tauri/Cargo.lock` selon celui que la CI `--locked` utilise.

**Interfaces :**
- Consomme : méthode RPC `hltb_url` (Tâche 3).
- Produit : commande Tauri `open_hltb(id: u64) -> Value` appelée par l'interface avec `invoke('open_hltb', { id })` ; renvoie `{ "ok": true }` ou `{ "ok": false, "error": … }` comme `rpc`.

- [ ] **Étape 1 : implémenter** : enregistrer `tauri_plugin_opener::init()` ; `open_hltb` appelle `core.0.call("hltb_url", &json!({"id": id}))`, vérifie que l'URL commence par `https://howlongtobeat.com/`, puis `app.opener().open_url(url, None::<&str>)`. Aucune permission `opener:*` ajoutée à `capabilities/default.json` (l'ouverture se fait côté Rust).
- [ ] **Étape 2 : compiler** `cd src-tauri && cargo build` → réussit, `Cargo.lock` mis à jour.
- [ ] **Étape 3 : commit** `git commit -am "feat(tauri): ouverture du lien HowLongToBeat"`

---

### Tâche 6 : interface

**Fichiers :**
- Modifier : `ui/app.js` (`SORTS` l. 45-53, état `filters` l. 79, sous-titre de carte l. 274-275, `enrich` l. 408, `activeFilters` l. 427-430, `loadLibrary` l. 432-440, `filterRow` l. 526-545, `hoursField` l. 1294-1306, `sheet` l. 1392-1407), `ui/app.css`.

**Interfaces :**
- Consomme : champs de carte `ttb`, `ttb_ref`, `ttb_text`, `hltb_url` (Tâche 3), arguments `sort:"ttb"` et `ttb` (Tâche 4), commande `open_hltb` (Tâche 5).

- [ ] **Étape 1 : tri et filtre** : ajouter `['ttb', 'Durée estimée']` à `SORTS` ; `filters.ttb: ''` (aussi dans la remise à zéro de « Effacer les filtres ») ; `activeFilters` et `loadLibrary` prennent `ttb` en compte ; `filterRow` ajoute un `select` « Durée » avec les options des contraintes globales, sur le modèle de `filterSelect`.
- [ ] **Étape 2 : cartes** : sous-titre = `[année, plateforme, temps]` où temps vaut `"18 h / ~25 h"` si `hours_text` et `ttb_text`, sinon `ttb_text`, sinon `hours_text`.
- [ ] **Étape 3 : fiche, bloc « Temps pour finir »** : section après la liste `facts`, affichée seulement si `state.mode === 'igdb'`. Trois colonnes (libellé + `*_text` ou « – »), ligne « d'après N joueurs (IGDB) » (+ « , peu de votes » si `few_votes`), ou « Pas de durée connue sur IGDB » si `ttb` est vide ; lien « Comparer sur HowLongToBeat » : `<a href=hltb_url target="_blank" rel="noopener noreferrer">`, et si `tauri` est présent, `preventDefault()` puis `window.__TAURI__.core.invoke('open_hltb', { id: c.id })`.
- [ ] **Étape 4 : fiche, comparaison** : sous le champ « Temps de jeu », si `entry.minutes` et `ttb_ref` : barre (`role="img"`, `aria-label` = la phrase) avec repères aux positions rapide / normal / complet sur une échelle `0 → max(completely, ref, minutes)`, curseur à `minutes` ; phrase « 18 h, soit ~72 % du temps normal » (pourcentage `minutes*60 / ttb_ref`, arrondi à l'unité) ; si `minutes*60 > completely` : barre pleine et « 60 h, au-delà du temps complet (~48 h) ». Mise à jour après `setHours`.
- [ ] **Étape 5 : seuils de niveau** : `enrich` relance `refresh` si `c.rev < 3` ; `detailed` (l. 1403) reste à `c.rev >= 2`.
- [ ] **Étape 6 : CSS** dans `ui/app.css`, avec les variables de couleur existantes ; lisible à la largeur minimale de la fenêtre (860 px).
- [ ] **Étape 7 : vérifier dans le navigateur.** Copier `%APPDATA%\BacklogPanda` dans un dossier temporaire, lancer `cargo run -- app` avec `BACKLOG_PANDA_DIR` pointant vers la copie (clés IGDB saisies). Contrôler : remplissage automatique des durées, bloc de fiche (jeu connu, jeu sans durée, jeu à peu de votes), barre (en dessous, au-dessus du temps complet), sous-titres de cartes, tri, chaque tranche du filtre, « Effacer les filtres », lien HLTB (nouvel onglet). Aucune erreur dans la console.
- [ ] **Étape 8 : vérifier dans la fenêtre Tauri** (`cd src-tauri && cargo tauri dev`, même dossier de données) : le lien HLTB ouvre le navigateur par défaut.
- [ ] **Étape 9 : commit** `git commit -am "feat(ui): temps pour finir, comparaison, tri et filtre"`

---

### Tâche 7 : documentation

**Fichiers :**
- Modifier : `README.md` (paragraphe « Source des jeux » et nombre de tests), `docs/TUTORIEL.md` (fiche du jeu, tri et filtres).

- [ ] **Étape 1 : écrire** deux ou trois phrases : d'où viennent les durées (IGDB), ce que veut dire « peu de votes », le lien HLTB. Mettre à jour le nombre de tests d'après `cargo test`.
- [ ] **Étape 2 : commit** `git commit -am "docs: temps pour finir"`
