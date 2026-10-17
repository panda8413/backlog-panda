> **Note de publication.** Les fichiers `docs/evidence/` cités dans ce rapport ne sont pas publiés dans ce dépôt (extraits de pages d'un site tiers, gardés en local). Le rapport décrit nos observations ; Backlog Panda ne contacte jamais Backloggd.

# Rétro-ingénierie de Backloggd et application minimale « Backlog Panda »

**Périmètre.** Une seule source : le site public https://backloggd.com/ observé avec **REA** (captures de navigateur sur le PC de l'utilisateur), **sans compte ni connexion**. Aucun code source de Backloggd (GitHub ou autre) n'a été lu ni cloné. Tout le code livré est en **Rust**.

**Conventions.** Chaque affirmation porte un statut :

- **OBSERVÉ** : lu directement dans une capture (référence `E..` dans `docs/evidence/`, voir `INDEX.md`) ;
- **DÉDUIT** : conclusion que je tire d'indices observés (les indices sont cités) ; peut être fausse ;
- **NON DÉTERMINÉ** : je n'ai pas pu le savoir avec cet outil et cette source.

Les chiffres sont ceux des captures du 16 octobre 2026 ; le site évolue (les empreintes des bundles changent, par exemple).

> **Suite (application de bureau).** Ce rapport décrit la rétro-ingénierie et la maquette initiale. L'application de bureau construite ensuite (Tauri + cœur Rust, jeux fournis par IGDB, données locales) est décrite dans `README.md` et `docs/DESIGN-CHOICES.md` ; elle ne contacte jamais Backloggd.

---

## 1. Ce que « processus principal, moteur de rendu, IPC » veut dire ici

Backloggd est un **site web**, pas une application Electron ou native : il n'y a ni processus principal, ni moteur de rendu embarqué, ni IPC au sens Electron. Les équivalents sont :

| Notion demandée | Équivalent pour un site web | Statut |
|---|---|---|
| Processus principal | Serveur d'application (Ruby on Rails, hébergé sur Render, derrière Cloudflare et BunnyCDN) | OBSERVÉ pour l'hébergement/CDN (A1–A4), DÉDUIT pour Rails (A5) |
| Moteur de rendu | Le navigateur de l'utilisateur + Turbo + Stimulus qui assemblent des fragments HTML | OBSERVÉ (B1, B2) |
| IPC | Requêtes HTTP `fetch`/XHR ; réponses JSON ou fragments HTML « Turbo Stream » | OBSERVÉ (E05, E07, E08, E15) |
| Modules / fonctions | Contrôleurs Rails (`pages#home`, `pages#search`, `pages#game`…) côté serveur ; contrôleurs Stimulus côté navigateur | OBSERVÉ pour les noms, NON DÉTERMINÉ pour le code (bundle illisible, E14) |

Le code JavaScript (`application-d49a8b53.js`) et le CSS dépassent le budget de lecture de REA (64 KiB par corps, E14) et ne sont pas atteignables en navigation directe (403, E14) : **les modules, fonctions et le détail des contrôleurs Stimulus n'ont pas pu être lus.**

---

## 2. Carte de l'architecture

### 2.1 Infrastructure et serveur

| ID | Affirmation | Statut | Preuve |
|---|---|---|---|
| A1 | Le trafic passe par BunnyCDN (`server: BunnyCDN-FR1-…`, en-têtes `cdn-*`) | OBSERVÉ | E15, E14 |
| A2 | Cloudflare est aussi dans la chaîne (`cf-ray`, `cf-cache-status: DYNAMIC`) ; l'ordre exact des deux CDN n'est pas connu | OBSERVÉ / NON DÉTERMINÉ (ordre) | E15 |
| A3 | L'origine tourne sur Render (`x-render-origin-server: Render`, `rndr-id`) | OBSERVÉ | E15 |
| A4 | Bunny Shield est actif : cookie `bunny_shield_id_146448`, script `/.bunny-shield/bd/bunnyprint.js`, `POST /.bunny-shield/bunnyprint/collect` | OBSERVÉ | E03, E01 |
| A5 | Application Ruby on Rails | **DÉDUIT** de : `x-request-id` + `x-runtime` (E15), cookie `_backloggd_session` (E03), `<meta name="csrf-param" content="authenticity_token">` (E02), champs `authenticity_token` (E12), `data-route="pages#home"` de forme `controleur#action` (E02, E06, E13), échappement JSON `\u0026` (E05), `turbo-stream`/`turbo-frame` | — |
| A6 | Authentification par Devise | **DÉDUIT** : formulaire `id="new_user"` vers `/users/sign_in/`, champs `user[login]`, `user[password]`, `user[remember_me]` (case + champ caché à 0) | E12 |
| A7 | Pagination par Pagy | **DÉDUIT** (faible) : règle robots `Disallow: *__pagy_page__` ; les pages de résultats utilisent `?page=N` | E11, E06 |
| A8 | JS et CSS applicatifs : `static.backloggd.com/assets/application-<empreinte>.js|css`, compression zstd, `cache-control: max-age=25600000` | OBSERVÉ | E14 |
| A9 | L'accueil déclenche 385 requêtes, très majoritairement tierces (publicité, mesure d'audience, images IGDB) ; les seules requêtes `fetch` propres à Backloggd sont celles de Bunny Shield | OBSERVÉ | E01 |
| A10 | Aucune connexion WebSocket dans la capture de l'accueil (types des requêtes propres : document, fetch, font, script, stylesheet) | OBSERVÉ (accueil seulement) | E01 |
| A11 | Les images de jaquettes viennent d'IGDB (`images.igdb.com/igdb/image/upload/t_cover_big/…`) | OBSERVÉ | E01, E06 |
| A12 | Un slug de jeu inexistant répond **HTTP 200** (le contenu n'a pas été analysé) | OBSERVÉ | E16 |

### 2.2 Navigateur

| ID | Affirmation | Statut | Preuve |
|---|---|---|---|
| B1 | Hotwire **Turbo** : style `.turbo-progress-bar` injecté, éléments `<turbo-frame>` et `<turbo-stream>`, `<meta name="turbo-prefetch" content="false">`, `data-turbo-frame="_top"`, `data-turbo="false"` sur le formulaire de recherche | OBSERVÉ | E02, E06, E07, E12 |
| B2 | Des contrôleurs **Stimulus** sont référencés par `data-controller` : `postload` (types `game-covers`, `game-detail-section`, `onmount`, `reviews`, `game-covers-search`), `selectpicker`, `reviews-link` | OBSERVÉ (attributs) ; DÉDUIT que c'est Stimulus qui les exécute | E07, E09, E12, E13 |
| B3 | Listes déroulantes par tom-select (classes `tomselected`, `ts-hidden-accessible`, ids `tomselect-N`) | OBSERVÉ (classes) / DÉDUIT (bibliothèque) | E12, E13 |
| B4 | Info-bulles par tippy.js (`data-tippy-content`) | OBSERVÉ (attribut) / DÉDUIT (bibliothèque) | E08 |
| B5 | L'autocomplétion est un plugin jQuery de type « devbridge » : le JSON `{"suggestions":[{"value","data"}]}` est exactement le format de ce plugin ; Bootstrap est chargé (`netdna.bootstrapcdn.com`) | DÉDUIT (format) / OBSERVÉ (requête CSS Bootstrap) | E05, E01 |
| B6 | Les listes (résultats, statistiques, avis, DLC) sont rendues **côté serveur en HTML** et insérées par Turbo ; ce ne sont pas des JSON interprétés par le navigateur | OBSERVÉ (réponses `text/vnd.turbo-stream.html`) | E07, E08, E09, E10, E15 |
| B7 | Chargement paresseux par `turbo-frame[loading="lazy"][src]` (pagination, avis) | OBSERVÉ | E06, E13 |

---

## 3. API observée

| Requête | Réponse | Statut | Preuve |
|---|---|---|---|
| `GET /autocomplete.json?query=Q&filter_editions=true\|false` | `application/json`, `cache-control: public, max-age=0`, `x-runtime` 0,03–0,27 s | OBSERVÉ | E04, E05, E15 |
| `GET /search/games/Q/` (aussi `/search/users/Q/`, `/search/lists/Q/`) | Page HTML, `data-route="pages#search"`, 11 résultats, cadre paresseux `#pagination` | OBSERVÉ (games) ; les deux autres onglets : liens seulement | E06 |
| `POST /search` (formulaire, champ `query`, `data-turbo="false"`) | **Réponse non observée** (formulaire non soumis) | NON DÉTERMINÉ | E12 |
| `GET /search/results.turbo_stream?page=N&query=Q&type=games` | `text/vnd.turbo-stream.html`, `no-cache` : `append → search-results` (11 cartes) puis `replace → pagination` (cadre vers la page N+1) | OBSERVÉ (page 2) | E07, E15 |
| `GET /fetch_game_stats/<game_id>/` et `/<game_id>/<edition_id>/` | Turbo Stream `replace → game-stats` : barres de l'histogramme, moyenne | OBSERVÉ (variante sans édition) ; variante avec édition : lien seulement | E08, E13 |
| `GET /update_game_detail/<game_id>/dlc/` | Turbo Stream `replace → game_detail_section` | OBSERVÉ | E09 |
| `GET /reviews/preview/<slug>/?sort_by=recent` | Cadre `game-reviews-section`, 10 avis | OBSERVÉ | E10, E13 |
| `GET /games/<slug>/` | Page HTML `pages#game`, JSON-LD `AggregateRating` | OBSERVÉ | E13 |
| `GET /logs/<slug>/plays/rating:N;/` (N = 1…10) | Liens des barres d'histogramme ; **non requêtés** | OBSERVÉ (liens) | E08 |
| `POST /users/sign_in/`, `POST /filters/_games_lib_popular_` | Formulaires ; **non soumis** | OBSERVÉ (formulaires) / NON DÉTERMINÉ (réponses) | E12 |
| `GET /robots.txt` | Texte | OBSERVÉ | E11 |

---

## 4. Recherche

### 4.1 Autocomplétion (`/autocomplete.json`)

| ID | Affirmation | Statut | Preuve |
|---|---|---|---|
| S1 | Saisir « zelda » (5 frappes) dans `#nav-bar-search` n'émet **qu'une** requête `autocomplete.json?filter_editions=true&query=zelda`. Le champ a `data-behavior="autocomplete"` et `autocomplete="off"` | OBSERVÉ | E04, E12 |
| S2 | Le navigateur regroupe donc les frappes (délai ou annulation). La valeur du délai et le nombre minimal de caractères côté navigateur ne sont pas connus | OBSERVÉ (une seule requête) / NON DÉTERMINÉ (délai, `minChars`) | E04, E14 |
| S3 | Forme exacte : `{"suggestions":[{"value":T,"data":{"slug":S,"title":T,"year":"A","id":N}}]}` ; `year` est une **chaîne** (`"TBD"` possible), `id` un entier, `value == title` | OBSERVÉ | E05 (162 jeux) |
| S4 | JSON compact (sans espaces), `&` écrit `\u0026`, apostrophe non échappée. Les autres échappements Rails (`<`, `>`, U+2028/9) ne sont pas vus | OBSERVÉ (`&`) / DÉDUIT (reste) | E05 |
| S5 | Au plus **50** lignes : « zelda », « ze » et « hades » renvoient exactement 50 | OBSERVÉ (50) / DÉDUIT (c'est un plafond) | E05 |
| S6 | Une requête de 0 ou 1 caractère (`""`, `"h"`) renvoie `{"suggestions":[]}` | OBSERVÉ (un exemple de chaque) / DÉDUIT (minimum = 2) | E05 |
| S7 | Règle de correspondance : le texte de la requête, réduit à ses caractères alphanumériques en minuscules, est une **sous-chaîne** du titre réduit de la même façon. Preuves : « zleda » renvoie 21 jeux dont les titres contiennent « zleda » une fois espaces et ponctuation retirés, y compris **à cheval sur des mots** : « Taisen Puzzle-Dama », « Frazzle Dazzle » (« frazzledazzle »), « Dazzle Dash », « The Grizzled: Armistice Digital » ; « ze » renvoie « F-Zero » (« fzero ») ; « Zelda: Link's » et « zelda links » renvoient les mêmes 13 jeux | OBSERVÉ (résultats) / DÉDUIT (règle) | E05 |
| S8 | Pas de tolérance aux fautes dans cet échantillon : « zleda » ne renvoie aucun jeu « Zelda » | OBSERVÉ | E05 |
| S9 | `filter_editions=true` remplace l'`id` et le `slug` des lignes d'édition par ceux du jeu parent, en gardant le titre de l'édition : pour « zelda links », trois lignes (ids 136334, 134231, 136899) deviennent id 115284, slug `the-legend-of-zelda-links-awakening--1` | OBSERVÉ (un cas) | E05 (`zelda_links__fe_false` vs `Zelda_3A_Link_27s__fe_true`) |
| S10 | Sans paramètre, la réponse est identique **octet pour octet** à `filter_editions=true` (« zelda », « hades »). « hades » est identique dans les trois variantes (aucune édition dans ces 50 lignes) | OBSERVÉ | E05 |
| S11 | Les slugs sont des **valeurs stockées**, pas recalculées : ma règle simple (minuscules, `&`→`and`, `+`→`plus`, apostrophes supprimées, reste → `-`) retrouve 157 slugs sur 162 ; les 5 écarts sont `hades-zero` (pour « H.A.D.E.S Zero »), `the-legend-of-zelda-link-s-awakening-dx`, `ratchet-clank-size-matters` (pour « Ratchet & Clank »), `zero-escape-virtue-s-last-reward`, `the-legend-of-zelda-majora-s-mask` | OBSERVÉ (écarts) / DÉDUIT (valeurs stockées) | E05, test `les_slugs_stockes_ne_se_recalculent_pas_toujours` |
| S12 | Doublons de slug : suffixe `--1` (`hades--1`, `…links-awakening--1`). Que le suivant soit `--2` n'est pas observé | OBSERVÉ (`--1`) / DÉDUIT (`--N`) | E05 |
| S13 | **Classement** : l'ordre des lignes est stable entre variantes d'une même requête, mais je n'ai pas trouvé sa règle (ni alphabétique, ni par année, ni par id). Il diffère de celui de la page de résultats (premier : « Zelda II » en autocomplétion, « Breath of the Wild » en page de résultats) | OBSERVÉ (différence) / NON DÉTERMINÉ (règle) | E05, E06 |

### 4.2 Page de résultats et pagination

| ID | Affirmation | Statut | Preuve |
|---|---|---|---|
| R1 | `/search/games/zelda/` : titre « Search results for zelda \| Backloggd », onglets Games / Users / Lists, **11** cartes `div.col-12.result[game_id]` | OBSERVÉ | E06 |
| R2 | Chaque carte : jaquette IGDB, titre + année (`span.subtitle-text`), badge de type (`main_game`, `remaster`…), plateformes | OBSERVÉ | E06, E07 |
| R3 | Sous les 11 cartes : `turbo-frame#pagination[loading=lazy][src=/search/results.turbo_stream?page=2&query=zelda&type=games]` | OBSERVÉ | E06 |
| R4 | La page 2 renvoie deux `turbo-stream` : `append` sur `search-results` (11 cartes) puis `replace` sur `pagination` avec le cadre de la page 3. Le défilement charge donc les pages en chaîne | OBSERVÉ (pages 1→2→3) | E07 |
| R5 | Les 2 cartes de E06 (DOM **après** exécution du JS) portent `loaded_game_log="true"`, celles de E07 (réponse **brute** du serveur) portent `loaded_game_log=false`. Le JS (contrôleur `postload`, type `game-covers-search`) passe donc l'attribut à `true` après chargement ; ce qu'il charge (état du journal de l'utilisateur ?) n'est pas établi | OBSERVÉ (attribut avant/après) / DÉDUIT (c'est le JS) / NON DÉTERMINÉ (contenu chargé) | E06, E07 |
| R6 | La page de résultats utilise-t-elle le même prédicat que l'autocomplétion ? Mon moteur le suppose | DÉDUIT (non testé séparément) | — |
| R7 | Réponse sur la **dernière** page, nombre total de résultats, comportement de `POST /search` | NON DÉTERMINÉ | — |

---

## 5. Notes

| ID | Affirmation | Statut | Preuve |
|---|---|---|---|
| N1 | Échelle de 0,5 à 5,0 par pas de 0,5 (10 barres) ; JSON-LD : `bestRating: 5`, `worstRating: 0.5` ; liens des barres `rating:1;`…`rating:10;` (nombre de demi-étoiles) | OBSERVÉ | E08, E13 |
| N2 | Infobulle de chaque barre : `"<n> \| <étoiles> ★ Ratings (<p>%)"`. Jeu 7346 : 258, 333, 384, 1 186, 1 722, 4 433, 8 218, 20 502, 27 955, 60 397 ; somme **125 388** | OBSERVÉ | E08 |
| N3 | Pourcentage = n / Σn × 100, une décimale. Le dénominateur est la somme des barres, pas `ratingCount` : avec 125 431 la barre 4,0 donnerait 16,3 % au lieu de 16,4 % | OBSERVÉ (+ calcul reproduit par test) | E08, E13 |
| N4 | Hauteur de barre = n / max(n) × 100 (en %) ; 60 397 → `100.0%`, 258 → `0.42717353510935974%` | OBSERVÉ (identique à 1e-12 près) | E08 |
| N5 | Moyenne = Σ(demi-étoiles × n) / 2 / Σn = **4,462687817015983**, identique au bit près à `ratingValue` du JSON-LD ; la page affiche « 4.5 » (arrondie à une décimale) | OBSERVÉ | E13, E08 |
| N6 | `ratingCount` du JSON-LD = 125 431, soit 43 de plus que la somme des barres. Cause inconnue | OBSERVÉ (écart) / NON DÉTERMINÉ (cause) | E13, E08 |
| N7 | Classe CSS `empty` : présente sur les barres à 0,43 %, 0,55 %, 0,64 %, absente à 1,96 %. Seuil exact inconnu ; mon code utilise 1,0 % | OBSERVÉ (encadrement) / NON DÉTERMINÉ (seuil) | E08 |
| N8 | Les statistiques sont **calculées côté serveur** (agrégat de toutes les notes) et chargées après coup par `/fetch_game_stats/<id>/` ; le navigateur ne calcule rien | DÉDUIT de N4/N5 + E08 | E08, E13 |

---

## 6. Stockage et synchronisation des notes

**Ce que je n'ai pas pu observer :** l'enregistrement d'une note par un utilisateur connecté (aucun compte n'a été utilisé), donc ni l'endpoint d'écriture, ni le format, ni le protocole de synchronisation entre appareils. Les anciens fichiers de ce dossier qui affirmaient connaître `/delete-rating`, `POST /log/`, `/api/user/games/logs` ou `/library/entry` n'étaient étayés par aucune capture : ils sont conservés à part dans `_prior_unverified/` et **aucune de leurs affirmations n'est reprise ici**.

| ID | Affirmation | Statut | Preuve |
|---|---|---|---|
| T1 | Visiteur anonyme : `data-authenticated="false"` sur `<body>` | OBSERVÉ | E02, E06, E13 |
| T2 | Session : cookie `_backloggd_session` (`Secure`, `HttpOnly`, `SameSite=Lax`, valeur hachée par REA) ; jeton CSRF dans `<meta name="csrf-token">` et champ `authenticity_token` | OBSERVÉ | E03, E02, E12 |
| T3 | Les données de notes vivent donc **côté serveur** : les agrégats arrivent du serveur (N8), l'identité passe par un cookie de session `HttpOnly` | DÉDUIT | E03, E08 |
| T4 | `localStorage` (visiteur anonyme) : 24 clés ; 23 portent des préfixes de scripts publicitaires/mesure (`cto_*`, `id5id_*`, `pbjs-*`, `_lc2_*`, `__tamLIResolveResult*`, `vms--*`, `_li_duid`, `__atmtd_*`, `__mileSession`, `_pubcid*`) et `loglevel` est un nom générique d'origine non établie ; aucune clé n'a de nom applicatif évident (nom seulement : les valeurs n'ont pas été lues) | OBSERVÉ (anonyme seulement) | E03 |
| T5 | Mécanisme de synchronisation (rechargement, Turbo, WebSocket, interrogation périodique) pour les notes d'un utilisateur | **NON DÉTERMINÉ** : seul le chargement paresseux des statistiques est observé | — |
| T6 | Cohérence entre les 43 notes de différence (N6) : peut-être des notes sans étoiles, des suppressions ou un cache ; non établi | NON DÉTERMINÉ | — |

**Conséquence pour l'application :** le stockage et la synchronisation de `backlog-panda` (\7, modules `library` et `sync`) sont une **conception propre**, présentée comme telle.

---

## 7. L'application Rust « backlog-panda »

Bibliothèque + CLI + serveur HTTP en bibliothèque standard ; dépendances : `serde`, `serde_json` uniquement. L'application **ne contacte jamais Backloggd** : son catalogue d'exemple (`data/catalogue_hors_ligne.json`, 162 jeux) est reconstruit hors ligne à partir des captures de `docs/evidence/E05_*`.

| Module | Rôle | Fidélité |
|---|---|---|
| `text` | normalisation de recherche, slugs, échappements HTML/URL | S7, S11 (observé + déduit) |
| `rails_json` | JSON compact avec `&`→`\u0026` | S3, S4 |
| `catalog`, `fixtures` | jeux (`id`, `slug`, `title`, `year` en chaîne, édition → parent), slugs uniques `--N`, import des captures | S3, S9, S12 |
| `search` | autocomplétion (sous-chaîne normalisée, ≥ 2 caractères, ≤ 50, `filter_editions`) et pages de 11 | S5–S10, R1 |
| `turbo` | rendu et relecture des cartes, `turbo-stream append/replace`, cadre paresseux | R1–R4, B6, B7 |
| `rating` | notes en demi-étoiles, histogramme, moyenne, pourcentages, hauteurs, `empty` | N1–N5 (N7 = hypothèse à 1,0 %) |
| `library`, `sync` | bibliothèque personnelle (statut + note), tampon de Lamport, fusion « dernier écrivain gagnant », suppression par pierre tombale, synchronisation en un aller-retour | **Conception propre** (T5) |
| `http`, `server` | routes `/autocomplete.json`, `/search/games/Q/`, `/search/results.turbo_stream`, `POST /search` (303), API `/api/library…`, `/api/sync` | routes de recherche calquées sur le comportement observé (E05–E07) ; `POST /search` et l'API sont de conception propre |
| `main` | CLI `import-fixtures`, `search`, `lib`, `serve` | — |

### Utilisation

```
cargo test
cargo run -- import-fixtures docs/evidence data/catalogue_hors_ligne.json
cargo run -- search data/catalogue_hors_ligne.json "Zelda: Link's"
cargo run -- lib ma_biblio.json data/catalogue_hors_ligne.json add 1028 playing 4.5
cargo run -- serve data/catalogue_hors_ligne.json ma_biblio.json 127.0.0.1:8080
```

### Tests (55, tous passants ; `cargo clippy --all-targets` sans avertissement)

| Fichier | Nb | Ce qui est vérifié |
|---|---|---|
| `src/*` (unitaires) | 35 | normalisation, slugs, échappements, bornes des notes, moyenne, pagination par 11, plafond de 50, minimum de 2 caractères, éditions, cycle de vie d'une bibliothèque, requêtes HTTP mal formées, routes du serveur |
| `tests/fixtures_autocomplete.rs` | 10 | **contre les 12 réponses réelles** : relecture/réécriture **identique octet pour octet**, prédicat de sous-chaîne vrai pour chaque ligne, plafond à 50, réponses vides, le moteur reproduit **exactement** les 2 réponses complètes (« zleda », « zelda links ») sur le catalogue de 162 jeux (aucun faux positif ni faux négatif), les deux écritures de la requête donnent le même résultat, `filter_editions` reproduit les deux variantes, absence de paramètre = `true`, règle de slug à 157/162 |
| `tests/fixtures_pages.rs` | 5 | cartes réelles de E06 et E07 (11 résultats, page suivante = 3), histogramme réel de E08 (somme 125 388), **moyenne égale à `ratingValue`**, pourcentages, hauteurs, classe `empty`, aller-retour du rendu des barres |
| `tests/server_tcp.rs` | 2 | vraie socket TCP : autocomplétion, page de résultats, Turbo Streams chaînés ; API de bibliothèque et synchronisation d'un appareil hors ligne |
| `tests/sync_properties.rs` | 3 | sur 200 tirages pseudo-aléatoires : fusion commutative, associative, idempotente ; suppression qui se propage ; horloge qui ne recule pas |

**Limites des tests.** Seules deux réponses d'autocomplétion sont complètes (< 50 lignes) ; les autres sont plafonnées, donc ne permettent de vérifier que la validité des lignes renvoyées, pas l'absence de lignes manquantes. Le classement n'est pas reproduit (S13) : l'ordre du moteur est celui du catalogue.

---

## 8. Ce que je n'ai pas réussi à déterminer

1. **Le code JavaScript** : modules, fonctions, contrôleurs Stimulus, délai et `minChars` exacts de l'autocomplétion (le bundle dépasse le budget REA de 64 KiB et répond 403 en navigation directe — E14).
2. **Le stockage et la synchronisation des notes d'un utilisateur** : aucun compte n'a été utilisé ; endpoints d'écriture, format et protocole inconnus (\6).
3. **La réponse de `POST /search`**, des formulaires de filtre et de connexion (non soumis).
4. **Le classement** des suggestions et des résultats, et la différence d'ordre entre les deux.
5. **L'origine des slugs** : règle de génération (stockés, 5 incohérences sur 162).
6. **Le seuil de la classe `empty`** (entre 0,64 % et 1,96 %).
7. **L'écart de 43** entre `ratingCount` et la somme des barres.
8. **La dernière page de la pagination** et le nombre total de résultats.
9. **La portée de `filter_editions`** : un seul cas observé (trois éditions d'un seul jeu) ; le critère qui fait d'une ligne une « édition », et l'origine de l'`year` renvoyée, ne sont pas connus.
10. **La structure interne du serveur** : Rails, Devise et Pagy sont des déductions (A5–A7), jamais observés directement.
11. **L'ordre Cloudflare / BunnyCDN** (A2).
12. **Les autres onglets de recherche** (utilisateurs, listes) et les pages de profil, de listes et de journaux (`/u/…`, `/logs/…`, `/lists/…`), non capturés.
13. **L'état d'un utilisateur connecté** (statuts de jeu, journal, favoris) : toute la partie « backlog » de l'application est donc de conception propre.

---

## 9. Méthode, limites et réserves

- **Outil.** Uniquement les outils REA (`capture_browser_scenario`, Edge sans interface). Chrome a échoué (`execution_failure`), Edge a fonctionné. Limites rencontrées : corps ≤ 64 KiB, 16 lectures de corps simultanées, budget d'événements (pages publicitaires), sortie ≤ 10 Mo (E14). Les réponses volumineuses ont été capturées en navigant directement vers l'URL (une requête, un corps).
- **Traitement.** Des scripts jetables Python/jq (non livrés) ont servi à extraire les extraits `E..` des captures brutes ; ils n'ont pas servi à fabriquer des preuves. Tout le code livré est en Rust.
- **robots.txt (E11).** Le fichier interdit tous les agents d'IA connus (dont ClaudeBot) et, pour les agents génériques, entre autres `/api/*`, `/logs/*`, `/lists/*` et `/games/*/*/`. J'ai appliqué ces règles aux **liens** (je n'ai pas requêté `/logs/…`, `/lists/…`, `/api/…`), mais la page `/games/lib/popular/` (E12), que j'ai chargée pour lire ses formulaires, tombe sous `/games/*/*/`. Les autres URL capturées (`/autocomplete.json`, `/search/…`, `/fetch_game_stats/…`, `/update_game_detail/…`, `/reviews/preview/…`, `/games/<slug>/`) n'y figurent pas. Ces captures ont été faites sur ordre de l'utilisateur, avec peu de requêtes, depuis son navigateur. **L'application Rust, elle, ne scrape pas Backloggd.**
- **Données personnelles.** Le fragment d'avis E10 contenait le pseudo et le texte d'un tiers : ils sont **caviardés** dans les preuves livrées.
- **Fichiers préexistants.** Le dossier contenait `Cargo.toml` et quatre fichiers `.rs` d'une session antérieure, dont les commentaires prétendaient avoir « observé » des endpoints et constantes que je n'ai pas retrouvés dans les captures. Je ne les ai pas supprimés : ils sont dans `_prior_unverified/` et ne servent d'appui à aucune affirmation de ce rapport. `Cargo.toml` a été réécrit.
- **Un instantané.** Chaque observation date du 16 octobre 2026 ; une capture ne prouve pas que le comportement est stable, et plusieurs conclusions (S5, S6, S9) reposent sur un ou deux exemples.
