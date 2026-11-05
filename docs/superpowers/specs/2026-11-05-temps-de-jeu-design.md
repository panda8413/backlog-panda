# Temps pour finir un jeu (durées IGDB)

Date : 5 novembre 2026 · Statut : validé à l'oral, à relire

## En bref

Chaque jeu de la bibliothèque affiche combien de temps il faut pour le finir : **rapide**, **normal** et **complet**. Ces durées viennent d'IGDB (déjà utilisé par l'app, mêmes clés). On peut trier et filtrer le backlog par durée, et comparer son propre temps de jeu à la durée normale.

## Pourquoi IGDB et pas HowLongToBeat

HowLongToBeat n'a pas d'API publique. Son API interne est protégée contre les robots (jeton de session et champ piège qui change à chaque requête) : la contourner irait contre la volonté du site et casserait à chaque changement. IGDB fournit les mêmes trois catégories via `POST /v4/game_time_to_beats`, dans un cadre autorisé.

Contrepartie : moins de votes que sur HLTB, donc certains jeux peu connus n'auront pas de durée. Un lien vers HLTB permet de comparer à la main.

## Ce que l'utilisateur voit

### Fiche du jeu : bloc « Temps pour finir »

Placé sous la liste d'informations (Sortie, Studio, Éditeur…).

| Rapide | Normal | Complet |
|---|---|---|
| ~12 h | ~25 h | ~48 h |

- Ligne de source : « d'après 312 joueurs (IGDB) ». Sous 5 joueurs : « d'après 3 joueurs (IGDB), peu de votes ».
- Une colonne absente chez IGDB affiche « – ».
- Sans aucune durée : « Pas de durée connue sur IGDB ».
- Toujours un lien **« Comparer sur HowLongToBeat »** qui ouvre `https://howlongtobeat.com/?q=<nom du jeu>` dans le navigateur. Rien n'est chargé depuis HLTB dans l'app.
- Sans clés IGDB (mode hors ligne) : le bloc n'apparaît pas.

### Fiche du jeu : comparaison avec mon temps

Sous le champ « Temps de jeu », dès qu'un temps est saisi et qu'une durée de référence existe :

- une barre horizontale avec trois repères (rapide, normal, complet) et un curseur à la position de mon temps ;
- une phrase : « 18 h, soit ~72 % du temps normal ».
- Au-delà de la durée complète : barre pleine, phrase « 60 h, au-delà du temps complet (~48 h) ».
- La barre a un texte équivalent pour les lecteurs d'écran (la phrase ci-dessus).

### Cartes de la bibliothèque

La ligne sous la jaquette (`année · plateforme · temps`) devient :

| Situation | Affichage |
|---|---|
| Mon temps et une durée de référence | `2020  Switch  18 h / ~25 h` |
| Pas de temps saisi, durée connue | `2020  Switch  ~25 h` |
| Pas de durée connue | inchangé |

### Tri et filtre

- Nouveau tri **« Durée estimée »** : du plus court au plus long, jeux sans durée à la fin (puis par nom).
- Nouveau filtre **« Durée »** à côté de Genre, Plateforme et Studio : *Toutes les durées · Moins de 5 h · 5 à 15 h · 15 à 40 h · Plus de 40 h*. Bornes : `< 5 h`, `5 h ≤ d < 15 h`, `15 h ≤ d < 40 h`, `≥ 40 h`. Les jeux sans durée n'apparaissent dans aucune tranche.
- « Effacer les filtres » remet aussi la durée à « Toutes ».
- Le tri existant « Temps de jeu » (mon temps) ne change pas.

### Règles communes

- **Durée de référence** (cartes, tri, filtre, comparaison) : la durée normale ; à défaut la rapide ; à défaut la complète.
- **Arrondi** : à la demi-heure sous 10 h (`~4 h 30`), à l'heure au-delà (`~25 h`). Moins de 30 min : `~30 min`. Le `~` signale une moyenne.

## Fonctionnement

### Données

`GameInfo` (fiche stockée dans `bibliotheque.json`) gagne un champ facultatif :

```rust
pub struct TimeToBeat {
    pub hastily: Option<u32>,    // secondes, « rapide »
    pub normally: Option<u32>,   // secondes, « normal »
    pub completely: Option<u32>, // secondes, « complet »
    pub count: u32,              // nombre de joueurs
}
// dans GameInfo :
#[serde(default)]
pub time_to_beat: Option<TimeToBeat>,
```

Les anciens fichiers se chargent sans changement (`serde(default)`).

### Niveaux de détail

`REV_FULL` passe de 2 à 3. Trois états distincts :

| `rev` | `time_to_beat` | Sens |
|---|---|---|
| < 3 | – | durées pas encore demandées |
| 3 | `None` | demandées, IGDB n'en a pas : on ne redemande plus |
| 3 | `Some(..)` | durées connues |

### Récupération

- `IgdbClient::by_ids` fait, après la requête `games`, une seconde requête `POST /v4/game_time_to_beats` : `fields game_id,hastily,normally,completely,count; where game_id = (…); limit 500;` pour les mêmes identifiants. Résultat : `rev = 3`, seulement si la requête `games` a réussi au niveau complet (les replis actuels vers des champs réduits restent à `rev = 1`).
- Si cette seconde requête échoue (réseau, limite IGDB), les fiches sont gardées sans durées et restent à `rev = 2` : elles seront redemandées au passage suivant. L'erreur n'empêche pas l'enregistrement des fiches.
- `search` ne demande pas les durées (une requête de plus à chaque frappe, inutile) : ses résultats sont à `rev = 2`.
- Remplissage de la bibliothèque existante : `sync_metadata` sélectionne déjà les fiches `rev < REV_FULL`, par paquets de 100. Rien d'autre à faire.
- Bouton « Recharger » d'une fiche (`refresh`) : passe par `by_ids`, donc met aussi les durées à jour.
- À l'ouverture d'une fiche, l'enrichissement automatique existant (`rev < 2` dans l'interface) passe à `rev < 3`.

### Interface (`ui/app.js`)

- `Card` (côté Rust) expose les durées (`ttb`, avec leurs textes formatés), le lien `hltb_url` et une durée de référence déjà formatée (`ttb_text`, ex. `~25 h`) plus sa valeur en secondes (`ttb_ref`). Le formatage reste dans le cœur Rust, comme `hours_text`.
- Le tri `ttb` et le filtre `ttb` (`lt5`, `5to15`, `15to40`, `ge40`) sont appliqués côté Rust dans `library`, comme les tris et filtres existants.

### Lien HowLongToBeat

- URL construite côté Rust : `https://howlongtobeat.com/?q=` + nom du jeu encodé (pourcent-encodage). Hôte fixe, jamais tiré d'une donnée externe.
- Mode navigateur (`cargo run`) : simple lien `<a target="_blank" rel="noopener noreferrer">`.
- Fenêtre Tauri : la WebView n'ouvre pas les liens externes seule. On ajoute une commande Tauri `open_hltb(id)` : elle demande l'URL au cœur Rust (hôte fixe) et l'ouvre avec `tauri-plugin-opener` côté Rust. L'interface ne reçoit aucune permission d'ouvrir des URL quelconques. Pas de `cmd /C start` avec un nom de jeu (risque d'injection).

## Tests

- `igdb.rs` : lecture de la réponse `game_time_to_beats` (champs absents, jeu sans entrée) ; `by_ids` envoie bien deux requêtes avec la clause `where game_id = (…)` ; échec de la seconde requête → fiches gardées à `rev = 2`.
- `service.rs` : `sync_metadata` reprend les fiches `rev = 2` ; une fiche `rev = 3` sans durée n'est pas redemandée ; tri `ttb` (jeux sans durée à la fin) ; chaque tranche du filtre, bornes comprises ; champs `ttb_text` / `ttb_ref` de la carte ; ancien fichier sans `time_to_beat` chargé correctement.
- Formatage : `29 min → ~30 min`, `4 h 20 → ~4 h 30`, `9 h 50 → ~10 h`, `25 h 20 → ~25 h`.
- URL HLTB : nom avec espaces, `&`, `:`, accents et guillemets correctement encodé.
- Interface : vérification à la main dans le navigateur (fiche, carte, tri, filtre, barre de comparaison, lien) et dans la fenêtre Tauri (ouverture du lien).

## Hors périmètre

- Colonnes de durée dans l'export CSV.
- Statistiques de durée (temps total estimé du backlog…).
- Toute récupération de données depuis HowLongToBeat.
- Saisie manuelle d'une durée de référence.
