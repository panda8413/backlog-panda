# Thèmes clair / sombre et sections par état : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** ajouter un thème clair cuivré et un thème sombre éclairci (choix Windows / Clair / Sombre), et ranger la vue « Tous » de la bibliothèque en sections par état.

**Architecture :** tout est dans `ui/`. Un script `ui/theme.js` chargé dans `<head>` pose `data-theme` sur `<html>` avant l'affichage et expose `window.BPTheme`. `app.css` ne contient plus de couleur en dur hors des blocs de thème. `app.js` regroupe côté interface les cartes déjà reçues de `library` en sections. Le cœur Rust n'est pas modifié.

**Tech Stack :** JavaScript sans framework (helper `h()`), CSS à variables, `localStorage`, tests Node (`node --test`, Node 24 présent) pour la logique pure et les contrôles de CSS, vérification à la main dans le navigateur intégré.

**Spec :** `docs/superpowers/specs/2026-11-06-themes-et-sections-design.md`

## Global Constraints

- Aucun fichier de `src/` ni `src-tauri/` modifié. `cargo test` (167 tests) reste vert.
- CSP inchangée : `script-src 'self'; style-src 'self'`. Aucun script en ligne, aucun attribut `style=` ; les variables passent par `vars` → `style.setProperty` comme aujourd'hui. Aucun `innerHTML`.
- Clés `localStorage` : `bp.theme` (`system` | `light` | `dark`) et `bp.sections` (objet `{ statut: "row" | "all" | "closed" }`). Toute lecture et écriture est dans un `try/catch` ; en cas d'échec, valeurs par défaut (`system`, sections par défaut).
- Sans attribut `data-theme`, le thème sombre s'applique.
- Ordre des sections : `playing, backlog, wishlist, completed, played, abandoned`. `abandoned` fermée par défaut, les autres `row`.
- Textes de l'interface en français, repris tels quels de la spec : « Apparence », « Selon Windows », « Clair », « Sombre », « Voir les N », « Réduire », « Passer au thème clair », « Passer au thème sombre », « Aucun jeu ne correspond à ces filtres. ».
- Contraste : tout texte ≥ 4,5:1 sur son fond, dans les deux thèmes.
- Vérifications manuelles avec la copie de test des données uniquement (entrée `backlog-panda-test` de `D:\Claude\DEV\.claude\launch.json`, port 8765). Jamais `%APPDATA%\BacklogPanda`.
- Terminal de l'utilisateur : PowerShell 5.1. Toute commande donnée à l'utilisateur sans `&&`.

## Review Focus

1. **Stockage indisponible ou corrompu** (`bp.theme` = `"bleu"`, `bp.sections` = `"{"`, accès qui lève) : l'app s'affiche avec les valeurs par défaut, sans erreur dans la console. Testé en Tâche 1 (thème) et Tâche 5 (sections).
2. **Fenêtre redimensionnée** alors qu'une section est sur une rangée : le bouton « Voir les N » apparaît ou disparaît, et les cartes passées dans la partie cachée deviennent `inert`. Vérifié à la main en Tâche 5.
3. **Windows change de mode pendant que l'app est ouverte** en *Selon Windows* : le thème suit ; en *Clair* ou *Sombre* explicite, il ne bouge pas. Testé en Tâche 1 (logique) et vérifié en Tâche 4.
4. **Badges sur les jaquettes en thème clair** (statut, note, durée, commentaire) : fond sombre translucide conservé, texte lisible sur image claire. Vérifié en Tâche 3.
5. **Ajout ou changement d'état depuis la fiche** pendant que la vue « Tous » est affichée : la carte change de section au re-rendu, sans perdre l'état replié/déplié des sections. Vérifié en Tâche 5.

---

### Tâche 1 : `theme.js`, choix et application du thème

**Files :**
- Create : `ui/theme.js`
- Create : `tests/ui/theme.test.cjs`
- Modify : `ui/index.html` (balise `color-scheme`, chargement de `theme.js`)

**Interfaces :**
- Produces (Node, via `module.exports` quand `module` existe) : `resolveTheme(choice: string, systemDark: boolean) -> 'light'|'dark'`, `readChoice(storage) -> 'system'|'light'|'dark'`, `saveChoice(storage, choice) -> void`.
- Produces (navigateur) : `window.BPTheme = { get() -> choice, set(choice) -> void, resolved() -> 'light'|'dark', onChange(cb: (resolved) => void) -> void }`. `set` enregistre, applique `document.documentElement.dataset.theme` et appelle les `onChange`. Un écouteur `matchMedia('(prefers-color-scheme: dark)')` réapplique et notifie quand le choix est `system`.

- [ ] **Étape 1 : écrire les tests qui échouent** dans `tests/ui/theme.test.cjs` (`node:test`, `node:assert/strict`, `require('../../ui/theme.js')`) :
  - `resolveTheme('system', true) === 'dark'`, `resolveTheme('system', false) === 'light'`, `resolveTheme('light', true) === 'light'`, `resolveTheme('dark', false) === 'dark'`, `resolveTheme('bleu', false) === 'light'` (valeur inconnue traitée comme `system`).
  - `readChoice` avec un faux stockage `{ getItem: () => 'dark' }` → `'dark'` ; `() => null` → `'system'` ; `() => 'bleu'` → `'system'` ; `getItem` qui lève → `'system'`.
  - `saveChoice` appelle `setItem('bp.theme', 'light')` ; un `setItem` qui lève ne propage pas d'erreur.
- [ ] **Étape 2 : lancer** `node --test tests/ui/` → échec (module introuvable).
- [ ] **Étape 3 : écrire `ui/theme.js`** : script classique enveloppé dans une IIFE ; partie navigateur exécutée seulement si `typeof document !== 'undefined'`, qui applique le thème immédiatement (le script est synchrone dans `<head>`). `localStorage` lui-même peut lever à l'accès : passer par une fonction `store()` protégée.
- [ ] **Étape 4 : `ui/index.html`** : `<meta name="color-scheme" content="light dark">` ; `<script src="theme.js"></script>` (sans `defer`) avant `<link rel="stylesheet" href="app.css">`.
- [ ] **Étape 5 : lancer** `node --test tests/ui/` → tous verts ; `cargo test` → 167 verts.
- [ ] **Étape 6 : commit** `feat(ui): choix du thème mémorisé, appliqué avant l'affichage`.

### Tâche 2 : variables de couleur et thème sombre éclairci

**Files :**
- Modify : `ui/app.css` (bloc `:root`, puis toutes les couleurs en dur)
- Create : `tests/ui/css.test.cjs`

**Interfaces :**
- Produces : les blocs `:root, :root[data-theme="dark"] { … }` et (Tâche 3) `:root[data-theme="light"] { … }`, avec au minimum ces variables, que la Tâche 3 redéfinit toutes :
  `--bg --bg-rail --panel --panel-2 --line --line-strong --text --dim --faint --st-backlog --st-playing --st-completed --st-played --st-abandoned --st-wishlist --star --star-base --focus --accent --track --hover --hover-strong --field-bg --field-hover --field-on --ring --inset-bg --overlay-bg --overlay-fg --scrim --sheet-bg --toast-bg --search-focus-bg --glow-1 --glow-2 --shadow-card --shadow-sheet --shadow-pop --primary-bg --primary-fg --primary-hover --primary-shadow --link --link-hover --bubble-bg --bubble-border --bubble-fg --scrollbar --select-arrow`.
  `--select-arrow` vaut un `url("data:image/svg+xml,…")` complet (la couleur du trait est dans l'URL). `--overlay-bg`/`--overlay-fg` habillent les badges posés sur les jaquettes et restent sombres dans les deux thèmes. `--bubble-*` reprennent, en sombre, le style actuel de l'onglet actif (`.tab[aria-pressed="true"]`). `color-scheme: dark` est posé dans le bloc sombre.

- [ ] **Étape 1 : écrire les tests qui échouent** dans `tests/ui/css.test.cjs` (lit `ui/app.css`) :
  - `aucune couleur en dur hors des blocs de thème` : après avoir retiré les blocs dont le sélecteur commence par `:root`, aucune occurrence de `#[0-9a-fA-F]{3,8}\b`, `rgba?(` ni `hsla?(` ; message d'échec = liste des lignes fautives.
  - `le bloc sombre porte les valeurs de la spec` : `--bg: #0e1326`, `--bg-rail: #131a33`, `--panel: #1a2140`, `--panel-2: #222a50`, `--dim: #b0b6d4`, `--faint: #7f86aa`.
  - `chaque variable du bloc sombre est définie aussi dans le bloc clair` : marqué `{ todo: 'Tâche 3' }` pour l'instant.
- [ ] **Étape 2 : lancer** `node --test tests/ui/` → les deux premiers échouent.
- [ ] **Étape 3 : modifier `ui/app.css`** : nouveau bloc sombre avec les valeurs éclaircies de la spec ; chaque couleur en dur (66 occurrences, y compris `.btn.primary`, `.link`, `.flt select`, `.toast`, `.sheet`, `.scrim`, `.bar .fill` dont le dégradé devient `linear-gradient(var(--star), var(--accent))`… ) remplacée par la variable correspondante. Les `rgba(8, 11, 23, …)` deviennent des variables calées sur le nouveau fond. Les ombres partagées vont dans `--shadow-*`.
- [ ] **Étape 4 : lancer** `node --test tests/ui/` → verts (le test marqué `todo` reste en attente) ; `cargo test` → verts.
- [ ] **Étape 5 : vérifier à la main** (serveur `backlog-panda-test`, onglet du navigateur intégré) : bibliothèque, recherche, fiche, statistiques, réglages, import CSV. Même apparence qu'avant, en un cran plus clair ; aucune couleur cassée (`read_console_messages` sans erreur, capture d'écran de la bibliothèque et d'une fiche).
- [ ] **Étape 6 : commit** `refactor(ui): couleurs en variables, thème sombre éclairci`.

### Tâche 3 : thème clair cuivré

**Files :**
- Modify : `ui/app.css` (bloc `:root[data-theme="light"]`, règles qui utilisent `--bubble-*`)
- Modify : `tests/ui/css.test.cjs`

**Interfaces :**
- Consumes : la liste de variables de la Tâche 2.
- Produces : le bloc clair. Valeurs fixées par la spec : `--bg #f7f5f2`, `--bg-rail #efebe6`, `--panel #ffffff`, `--panel-2 #f3efea`, `--line rgba(60, 40, 20, 0.10)`, `--line-strong rgba(60, 40, 20, 0.20)`, `--text #1f1a16`, `--dim #5e554c`, `--faint #776c62`, `--bubble-bg #f6e3d3`, `--bubble-border #e8bf9f`, `--bubble-fg #8a4a22`, `--accent #c8794a`, `--star #d99a00`, états `--st-backlog #2f6fd6`, `--st-playing #138a66`, `--st-completed #946200`, `--st-played #6b4fd1`, `--st-abandoned #c8364f`, `--st-wishlist #c2408e`. Ombres brunes `rgba(60, 40, 20, 0.08–0.18)`, pas de halo lumineux (`--glow-1`/`--glow-2` transparents ou très légers en cuivre). `--focus` cuivre. `color-scheme: light`.

- [ ] **Étape 1 : tests qui échouent** dans `tests/ui/css.test.cjs` :
  - retirer le `todo` du test « chaque variable du bloc sombre est définie aussi dans le bloc clair » ;
  - `le bloc clair porte les valeurs de la spec` (toutes les valeurs ci-dessus) ;
  - `contraste AA en clair` : ratio WCAG ≥ 4,5 pour `--text`, `--dim`, `--faint` sur `--bg` et sur `--panel` ; `--bubble-fg` sur `--bubble-bg` ; chaque `--st-*` sur `--panel` (fonction `contrast(hexA, hexB)` écrite dans le test, formule de luminance relative WCAG 2.x) ;
  - `contraste AA en sombre` : `--text`, `--dim` sur `--bg` et `--panel` ; `--faint` sur `--bg`.
- [ ] **Étape 2 : lancer** `node --test tests/ui/` → échecs attendus (bloc clair absent).
- [ ] **Étape 3 : écrire le bloc clair** et faire porter les bulles sur `--bubble-*` : onglet d'état actif, `.flt.on select`, entrée de menu `aria-current="page"`, `.chip`, `.btn.primary`, `.bk-mode:has(input:checked)`, `.csv-tabs button.on`. En sombre, ces règles doivent rendre exactement comme à la fin de la Tâche 2.
- [ ] **Étape 4 : lancer** `node --test tests/ui/` → verts. Si un contraste échoue avec une valeur de la spec, foncer la couleur au minimum nécessaire, mettre la spec à jour dans le même commit et le signaler dans le rapport.
- [ ] **Étape 5 : vérifier à la main** en posant le thème clair depuis la console (`BPTheme.set('light')`) : les mêmes pages qu'en Tâche 2, plus badges sur jaquettes claires et sombres (Review Focus 4), champs de date et listes déroulantes. Captures d'écran bibliothèque, fiche, statistiques.
- [ ] **Étape 6 : commit** `feat(ui): thème clair aux bulles cuivrées`.

### Tâche 4 : bouton soleil/lune et réglage Apparence

**Files :**
- Modify : `ui/index.html` (bouton dans `.rail`, après `#conn`)
- Modify : `ui/app.js` (icônes `sun` et `moon` dans `ICONS`, `renderThemeToggle()`, section Apparence dans `viewSettings`)
- Modify : `ui/app.css` (bouton du rail, groupe de trois boutons)

**Interfaces :**
- Consumes : `window.BPTheme` (Tâche 1).
- Produces : `renderThemeToggle() -> void` qui remplit `#theme-toggle` (icône du thème vers lequel on passe, `aria-label` « Passer au thème clair » / « Passer au thème sombre », `title` identique) ; appelée au démarrage et dans `BPTheme.onChange`.

- [ ] **Étape 1 : bouton du rail** : `<button class="theme-toggle" id="theme-toggle" type="button"></button>` à côté de `#conn` (même ligne, `#conn` garde sa largeur restante). Clic → `BPTheme.set(BPTheme.resolved() === 'dark' ? 'light' : 'dark')`.
- [ ] **Étape 2 : Réglages → Apparence** : nouvelle section en tête des réglages, titre « Apparence », groupe `role="radiogroup"` de trois boutons `aria-checked` *Selon Windows* / *Clair* / *Sombre*, état lu dans `BPTheme.get()`, clic → `BPTheme.set(...)` puis re-rendu de la section. Style des boutons cochés : `--bubble-*`.
- [ ] **Étape 3 : vérifier à la main** : bascule par le bouton (le réglage passe de *Selon Windows* à *Clair*/*Sombre*) ; choix dans Réglages ; rechargement → choix conservé ; *Selon Windows* qui suit `resize_window` `colorScheme: 'light'`/`'dark'` (Review Focus 3) ; en *Clair* explicite, changer `colorScheme` ne bouge rien ; navigation clavier sur le groupe. `node --test tests/ui/` et `cargo test` verts.
- [ ] **Étape 4 : commit** `feat(ui): bouton de thème et réglage Apparence`.

### Tâche 5 : sections par état dans la vue « Tous »

**Files :**
- Modify : `ui/app.js` (`viewLibrary`, nouvelles fonctions ci-dessous, icône `chevron`)
- Modify : `ui/app.css` (`.lib-section` et ses états ; `.shelf` supprimée si plus utilisée)

**Interfaces :**
- Produces :
  - `SECTION_ORDER = ['playing', 'backlog', 'wishlist', 'completed', 'played', 'abandoned']`
  - `sectionModes() -> { [status]: 'row'|'all'|'closed' }` : lit `bp.sections` (JSON) protégé, ne garde que les statuts connus et les trois valeurs permises, complète par les défauts (`abandoned: 'closed'`, autres `'row'`).
  - `saveSectionMode(status, mode) -> void`
  - `sectionEl(status, cards) -> HTMLElement` : `<section class="lib-section" data-mode="…" aria-labelledby>` avec en-tête (pastille `--c: var(--st-…)`, `<h2>` nom, nombre, bouton « Voir les N » / « Réduire » avec `aria-expanded`, chevron `aria-expanded` + `aria-controls`) et grille `.grid` des `cardEl`.
  - `fitSection(sectionEl) -> void` : en mode `row`, mesure les cartes dont `offsetTop` dépasse celui de la première, leur pose `inert` (et le retire aux autres), et affiche le bouton « Voir les N » seulement s'il y en a ; en mode `all`, retire `inert` partout et affiche « Réduire » si plus d'une rangée. Appelée après le rendu et par un `ResizeObserver` unique sur `#view`, déconnecté à chaque re-rendu.

- [ ] **Étape 1 : `viewLibrary`** : quand `state.status === 'all'`, remplacer « Reprendre » / « Autres jeux » par une `sectionEl` par statut de `SECTION_ORDER` ayant au moins une carte (cartes prises dans `entries`, ordre du tri conservé). Aucune section → « Aucun jeu ne correspond à ces filtres. » si un filtre est actif. Un onglet d'état précis garde la grille simple actuelle.
- [ ] **Étape 2 : CSS** : `.lib-section[data-mode="row"] .grid` limite l'affichage à une rangée (`grid-template-rows: auto; grid-auto-rows: 0; row-gap: 0; overflow: hidden`, avec une marge intérieure haute et basse suffisante pour ne pas couper l'ombre et le soulèvement des cartes au survol) ; `[data-mode="closed"] .grid { display: none }` ; en-tête en ligne, nombre en `--faint`, chevron qui tourne selon l'état (désactivé avec `prefers-reduced-motion`).
- [ ] **Étape 3 : interactions** : chevron → `closed` ↔ mode précédent ouvert (`row` par défaut) ; « Voir les N » → `all` ; « Réduire » → `row`. Chaque changement enregistre via `saveSectionMode`, met à jour `data-mode` et les `aria-expanded` sur place (pas de re-rendu complet, le défilement ne saute pas), puis `fitSection`.
- [ ] **Étape 4 : vérifier à la main** (copie de test) :
  - vue « Tous » : sections dans l'ordre, Abandonné absente (0 jeu dans la copie) ; mettre un jeu en Abandonné depuis sa fiche → la section apparaît fermée, les autres gardent leur état (Review Focus 5) ;
  - Backlog : une rangée, « Voir les 47 », dépliage, « Réduire » ; rechargement → états conservés ;
  - `localStorage.setItem('bp.sections', '{')` puis rechargement → sections par défaut, pas d'erreur console (Review Focus 1) ;
  - fenêtre à 1440, 1000 et 700 px de large (`resize_window`) : nombre de cartes par rangée, bouton présent ou absent selon le débordement, `inert` sur les cartes cachées (vérifié par `javascript_tool`) (Review Focus 2) ;
  - filtre Genre actif : nombres par section, sections vides masquées ; filtre sans résultat → message ;
  - tri « Durée estimée » : ordre respecté dans chaque section ;
  - onglet « Backlog » : grille simple ;
  - clavier : Tab ne s'arrête pas sur les cartes cachées ;
  - les deux thèmes.
  `node --test tests/ui/` et `cargo test` verts.
- [ ] **Étape 5 : commit** `feat(ui): bibliothèque rangée en sections par état`.

### Tâche 6 : documentation et vérification finale

**Files :**
- Modify : `README.md` (fonctionnalités : thèmes, sections ; commande `node --test tests/ui/`)
- Modify : `docs/TUTORIEL.md` (où changer de thème, comment déplier ou replier une section)

- [ ] **Étape 1 : documenter** en deux ou trois phrases par fichier, dans le ton existant.
- [ ] **Étape 2 : vérification complète** : `cargo test --locked` (167), `node --test tests/ui/`, `cargo build --locked` dans `src-tauri`.
- [ ] **Étape 3 : fenêtre Tauri** (`Set-Location D:\Claude\DEV\backlog-panda\src-tauri; cargo run`, données réelles de l'utilisateur, lecture seule de l'interface : ne rien modifier dans la bibliothèque) : thème clair appliqué, mémorisé après redémarrage de la fenêtre, sections affichées. Noter si l'éclair sombre au démarrage est visible.
- [ ] **Étape 4 : commit** `docs: thèmes et sections`.
