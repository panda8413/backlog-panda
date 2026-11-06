# Thèmes clair / sombre et bibliothèque rangée par état

Date : 6 novembre 2026 · Statut : validé à l'oral, à relire

## En bref

1. L'app propose deux thèmes : **clair** (nouveau, blanc chaud avec des bulles cuivrées claires) et **sombre** (le thème actuel, un cran plus clair). Par défaut elle suit Windows.
2. La page Bibliothèque, vue « Tous », range les jeux en **sections par état** : En cours, Backlog, Envie, Terminé, Joué, Abandonné. Chaque section montre une rangée de jaquettes et se déplie ou se replie.

Rien ne change dans le cœur Rust : tout se passe dans `ui/`.

## 1. Thèmes

### Choisir

- **Réglages → Apparence** : trois boutons *Selon Windows* (défaut), *Clair*, *Sombre*.
- **Barre latérale** : en bas, à côté de l'indicateur IGDB, un bouton icône soleil/lune. Il bascule vers le thème opposé à celui affiché et enregistre ce choix explicite (*Clair* ou *Sombre*), ce qui quitte *Selon Windows*. Libellé accessible : « Passer au thème clair » / « Passer au thème sombre ».
- *Selon Windows* suit `prefers-color-scheme` en direct : si Windows change de mode pendant que l'app est ouverte, l'app suit.

### Mémoriser sans éclair

- Le choix est rangé dans `localStorage` sous la clé `bp.theme` (`system` | `light` | `dark`). Lecture et écriture sont protégées par `try/catch` : si le stockage est indisponible, l'app retombe sur *Selon Windows*.
- Un petit script `ui/theme.js`, chargé de façon synchrone dans `<head>` avant la feuille de style (pas de script en ligne : la CSP n'autorise que `script-src 'self'`), pose `data-theme="light"` ou `data-theme="dark"` sur `<html>` avant le premier affichage. Il expose `window.BPTheme = { get(), set(choice), resolved() }`, et `app.js` l'utilise pour le bouton et les réglages.
- `<meta name="color-scheme">` passe à `light dark`, et `color-scheme` est posé par thème dans le CSS : champs de date, listes déroulantes natives et barres de défilement suivent le thème.

Limite acceptée : dans la fenêtre Tauri, la couleur de fond avant chargement (`backgroundColor` de `tauri.conf.json`) reste sombre. En thème clair, un éclair sombre très bref est possible au démarrage.

### Palette

Toutes les couleurs passent par des variables CSS sur `:root[data-theme="…"]`. Les 66 couleurs écrites en dur dans `app.css` (fonds translucides, ombres, focus, liens, boutons primaires, menus déroulants, barres de statistiques, toasts, voile de la fiche…) sont remplacées par des variables. Sans attribut (script bloqué), le thème sombre s'applique.

**Sombre (éclairci)**

| Variable | Avant | Après |
|---|---|---|
| `--bg` | `#080b17` | `#0e1326` |
| `--bg-rail` | `#0c1022` | `#131a33` |
| `--panel` | `#12172e` | `#1a2140` |
| `--panel-2` | `#1a2040` | `#222a50` |
| `--dim` | `#a6acca` | `#b0b6d4` |
| `--faint` | `#70769a` | `#8c92b3` |

Les autres valeurs (texte, couleurs d'état, accent lavande) sont inchangées. Les fonds translucides calés sur l'ancien `#080b17` (`rgba(8, 11, 23, …)`) suivent le nouveau fond.

**Clair**

| Rôle | Valeur |
|---|---|
| Fond | `#f7f5f2` |
| Barre latérale | `#efebe6` |
| Panneau / carte | `#ffffff` |
| Panneau secondaire | `#f3efea` |
| Lignes | `rgba(60, 40, 20, 0.10)` / forte `rgba(60, 40, 20, 0.20)` |
| Texte | `#1f1a16` |
| Texte secondaire | `#5e554c` |
| Texte discret | `#736860` |
| Bulle cuivrée : fond / bord / texte | `#f6e3d3` / `#e8bf9f` / `#8a4a22` |
| Accent cuivre (focus, barres, curseur) | `#c8794a` |
| Étoiles | `#d99a00` |
| Ombres | douces, brunes et peu opaques (`rgba(60, 40, 20, 0.08–0.18)`) au lieu des halos lumineux |

Couleurs d'état en clair, plus foncées pour rester lisibles (≥ 4,5:1) sur le fond et sur blanc : Backlog `#2f6fd6`, En cours `#107956`, Terminé `#926100`, Joué `#6b4fd1`, Abandonné `#c2344d`, Envie `#c2408e`.

Les **bulles cuivrées** habillent, en clair : l'onglet d'état actif, les filtres actifs, l'entrée de menu sélectionnée, les puces (genres…), le bouton principal et les boutons de choix de mode (import, CSV). En sombre, ces éléments gardent leur style actuel (accent lavande).

Les jaquettes et leurs badges posés sur l'image (statut, note, durée) gardent un fond sombre translucide dans les deux thèmes : ils doivent rester lisibles sur n'importe quelle image.

Contraste : tout texte sur son fond atteint au moins 4,5:1 (WCAG AA), vérifié dans les deux thèmes.

## 2. Bibliothèque rangée par état

### Vue « Tous »

- Sections dans l'ordre : **En cours, Backlog, Envie, Terminé, Joué, Abandonné**. Les titres « Reprendre » et « Autres jeux » disparaissent.
- En-tête de section : pastille de la couleur de l'état, nom, nombre de jeux, puis à droite le bouton « Voir les N » ou « Réduire » (s'il y a plus d'une rangée), et un chevron pour replier ou ouvrir la section.
- Trois états par section :
  - **ouverte, une rangée** (défaut) : une seule ligne de jaquettes, coupée à la largeur disponible. Si des jeux ne tiennent pas, le bouton « Voir les N » est affiché (N = nombre de jeux de la section) ;
  - **ouverte, dépliée** : toute la grille, bouton « Réduire » ;
  - **fermée** : seul l'en-tête.
- **Abandonné** est fermée par défaut, les autres ouvertes sur une rangée.
- Une section sans jeu n'apparaît pas.
- L'état de chaque section est retenu dans `localStorage` (`bp.sections`, objet `{ statut: "row" | "all" | "closed" }`), avec la même protection que le thème.
- Le chevron est un `<button aria-expanded>` relié à la section ; le bouton « Voir les N / Réduire » porte aussi `aria-expanded`.

« Une rangée » est calculée par le navigateur : la grille garde une seule ligne visible (`grid-template-rows` à une ligne, lignes suivantes à hauteur nulle et masquées, sans être retirées du DOM). Le bouton « Voir les N » s'affiche quand des cartes débordent, mesuré au rendu et à chaque redimensionnement (`ResizeObserver`). Les cartes masquées ne reçoivent pas le focus clavier (`inert` sur les cartes cachées, recalculé avec la mesure).

### Onglets, tri et filtres

- Les onglets d'état restent. Un onglet précis (ex. « Backlog ») affiche une grille simple de cet état, sans sections, comme aujourd'hui. « Tous » affiche les sections.
- Le tri choisi s'applique dans chaque section.
- Les filtres (genre, plateforme, studio, note, durée) s'appliquent dans chaque section. Le nombre affiché est celui des jeux qui correspondent, et les sections vides disparaissent. Si aucune section ne reste, le message « Aucun jeu ne correspond à ces filtres. » s'affiche.
- Le mode hors ligne, la bannière et l'état vide de la bibliothèque ne changent pas.

### Ce qui ne change pas

Recherche, fiche d'un jeu, statistiques, réglages (hormis la ligne Apparence), cœur Rust, API.

## Fichiers touchés

- `ui/theme.js` (nouveau) : choix, stockage, application du thème.
- `ui/index.html` : chargement de `theme.js`, `color-scheme`.
- `ui/app.css` : variables par thème, remplacement des couleurs en dur, styles des sections.
- `ui/app.js` : sections de la vue « Tous », bouton soleil/lune, ligne Apparence dans les réglages.
- `README.md`, `docs/TUTORIEL.md` : mention des thèmes et des sections.

## Vérification

- `cargo test` : les 167 tests restent verts (le cœur n'est pas modifié).
- À la main, dans le navigateur avec la copie de test des données : les deux thèmes sur chaque page (bibliothèque, recherche, fiche, statistiques, réglages, import CSV), *Selon Windows* qui suit un changement de mode, mémorisation après rechargement, sections (rangée, dépliée, fermée, mémorisées), filtres et tri dans les sections, onglet d'état précis, fenêtre étroite, navigation au clavier (pas de focus sur les cartes cachées).
- Contraste mesuré des textes cuivrés et des couleurs d'état en clair.
- Fenêtre Tauri : thème appliqué et mémorisé après redémarrage.

## Hors périmètre

- Thème lu par Rust pour la couleur de fond de la fenêtre.
- Thèmes supplémentaires ou couleur d'accent personnalisable.
- Glisser-déposer des jeux entre sections.
- Ordre des sections personnalisable.
