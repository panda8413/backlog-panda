# Backlog Panda

Application Windows (gratuite, sans compte) pour suivre ton backlog de jeux vidéo.

**Installer et utiliser : voir le [tutoriel](docs/TUTORIEL.md).** Télécharge `Backlog Panda.exe` dans l'onglet *Releases*.

---

Application de bureau pour suivre son backlog de jeux vidéo : jaquettes, statuts (backlog, en cours, terminé, joué, abandonné, envie), notes en demi-étoiles, recherche. Données stockées **sur ton ordinateur**, dans un fichier.

- **Cœur en Rust** : recherche, statuts, notes, stockage, appels réseau (`src/`). L'interface (`ui/`, HTML/CSS/JS sans dépendance) ne fait qu'afficher.
- **Source des jeux** : l'API IGDB (clés Twitch gratuites, à saisir dans *Réglages* ou via les variables `IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET`). Une clé **DeepL** facultative (*Réglages* ou `DEEPL_API_KEY`) permet de traduire les résumés en français ; sans elle, genres, thèmes et modes de jeu sont tout de même traduits localement. Les durées « Temps pour finir » (rapide, normal, complet) viennent aussi d'IGDB, rien n'est lu ailleurs ; « peu de votes » signale une durée tirée de moins de 5 joueurs, à prendre avec prudence, et un lien ouvre la page HowLongToBeat du jeu pour comparer. Sans clés, un catalogue hors ligne de 162 jeux (sans jaquettes) permet d'essayer l'application.
- **Deux enveloppes, même moteur** : une fenêtre native Tauri (`src-tauri/`) ou un serveur local ouvert dans le navigateur (`backlog-panda app`).
- Modélisée au départ sur le comportement **observé** de backloggd.com (voir `docs/REPORT.md`) ; l'application ne contacte jamais Backloggd.

## Lancer

Prérequis Windows : Rust (rustup) **et** les « Outils de build Visual Studio » avec la charge de travail *Développement Desktop en C++* + SDK Windows (sinon : erreur `LNK1104 msvcrt.lib`).

```
cargo run                               # ouvre l'application dans ton navigateur
cargo run -- app --demo                 # avec quelques jeux d'exemple (dossier temporaire)
cargo test                              # 164 tests
```

Fenêtre native (Tauri 2, WebView2 déjà présent sur Windows 11) :

```
cargo install tauri-cli --version "^2" --locked
cd src-tauri
cargo tauri build                       # installateur dans src-tauri/target/release/bundle/nsis/
cargo tauri dev                         # fenêtre de développement
```

## Où sont mes données ?

`%APPDATA%\BacklogPanda\` (ou `$BACKLOG_PANDA_DIR`) : `bibliotheque.json` (écriture atomique + copie `.bak`), `sauvegardes\` (copies datées, voir plus bas) et `reglages.json` (identifiant client IGDB ; le secret IGDB et la clé DeepL sont rangés dans le **Gestionnaire d'identifiants de Windows**, donc chiffrés et liés à ta session ; sur un système sans coffre pris en charge ils restent dans ce fichier, en clair).

## Documentation

- `docs/DESIGN-CHOICES.md` : journal des choix de design et d'architecture (pour itérer sur le visuel).
- `docs/REPORT.md` : rétro-ingénierie, statut de chaque affirmation (OBSERVÉ / DÉDUIT / NON DÉTERMINÉ) et ce qui n'a pas pu être déterminé.
- `docs/evidence/` : extraits de pages publiques d'un site tiers, **gardés en local et non publiés dans ce dépôt** ; les tests qui s'en servent s'ignorent s'ils sont absents.
- `data/catalogue_hors_ligne.json` : petit catalogue de 162 jeux (identifiant IGDB, titre, année) pour essayer l'application sans clé.

Anciennes commandes (maquette Backloggd, conservées) : `import-fixtures`, `search`, `lib`, `serve`.


## Fiche d'un jeu et filtres
- **Fiche** : fenêtre centrée avec studio, éditeur, date de sortie, franchise, plateformes, modes de jeu, perspective, thèmes et note IGDB.
- **Ta part** : statut, plateforme sur laquelle tu joues, note en demi-étoiles, commentaire libre (enregistré automatiquement).
- **Bibliothèque** : filtres par genre, plateforme, studio et note minimale, combinables avec les statuts et le tri (dernière modification, note, nom, année, temps de jeu, date d'ajout, plateforme).
- **Dates** : la date d'ajout se pose toute seule ; la date de fin aussi quand le statut passe à Terminé, Joué ou Abandonné (elle s'efface si tu reprends le jeu). Les deux se corrigent dans la fiche (champs de date). Tri « Date d'ajout » dans la bibliothèque ; colonnes « Ajouté le » / « Terminé le » dans le CSV. Les jeux ajoutés avant cette version n'ont pas de date d'ajout : à renseigner à la main si tu la connais.
- **Temps de jeu** : saisie libre (`12h30`, `12,5`, `45 min`) dans la fiche ; tri par temps de jeu dans la bibliothèque.
- **Statistiques** : ancienneté moyenne du backlog, les plus anciens jeux en attente, jeux terminés par année et délai moyen entre ajout et fin, temps total, taux de finition, note moyenne et médiane, coups de cœur, mieux notés, plus longues parties, répartition par genre, plateforme, studio, éditeur et époque.
- **Réglages** : guides pas à pas pour obtenir la clé IGDB (via Twitch) et la clé DeepL.

## Sauvegarde, export et import
Réglages → **Sauvegarde et export**.
- **Sauvegardes datées automatiques** : au premier changement de la journée (et au lancement si aucune n'existe depuis 20 h), l'état *d'avant* est copié dans `sauvegardes\bibliotheque-AAAAMMJJ-HHMMSS-auto.json` (heure UTC dans le nom ; l'écran affiche l'heure locale). Les 15 plus récentes sont gardées. « Sauvegarder maintenant » en crée une à la demande ; chaque ligne a un bouton **Restaurer** (avec confirmation ; l'état actuel est sauvegardé juste avant).
- **Fichier illisible au lancement** (coupure pendant l'écriture, disque plein…) : le fichier abîmé est gardé sous `bibliotheque.json.corrompu-…`, puis l'application restaure la copie `.bak`, sinon la dernière sauvegarde datée lisible, et le dit dans un bandeau. Une simple erreur de lecture (fichier verrouillé) ne déclenche rien.
- **Exporter** : JSON (tout : statuts, notes, heures, commentaires, fiches ; réimportable) ou CSV pour Excel (séparateur `;`, virgule décimale, UTF-8 avec BOM, formules neutralisées ; réimportable par « Importer une liste (CSV) »). Fichiers déposés dans *Téléchargements*, jamais en écrasant un fichier existant (`-2`, `-3`…).
- **Importer** un JSON (export ou sauvegarde) avec **aperçu** de ce qui changera, au choix : *Ajouter les jeux qui manquent* (rien d'existant n'est touché), *Fusionner* (modification la plus récente gagnante, pensé pour une copie de ta propre bibliothèque) ou *Tout remplacer*. Une sauvegarde « avant import » est faite d'abord ; si elle échoue, rien n'est importé.

## Importer une liste (CSV)
Réglages → **Importer une liste (CSV)**. Utile pour reprendre un tableur ou la liste d'un autre site, ou pour réimporter l'export CSV de l'application.
1. Choisis un fichier `.csv` : seule une colonne de titres est indispensable. Sont reconnues, par leur titre (français ou anglais) : année, statut, note, plateforme, heures, commentaire, date d'ajout, date de fin, « Identifiant IGDB ». Les autres colonnes sont ignorées et listées à l'écran.
2. **Rechercher les jeux** : chaque titre est rapproché d'un jeu (IGDB si ta clé est configurée, sinon le catalogue intégré de 162 jeux). Rien n'est ajouté à ce stade.
3. **Vérifier** : *trouvés* (cochés d'office), *à vérifier* (choisis le bon jeu dans la liste, puis coche), *introuvables*, *déjà présents* (jamais modifiés).
4. **Ajouter** les jeux cochés. Une sauvegarde « avant import » est faite d'abord.
- Notes : indique l'échelle dans l'en-tête (« Note /10 », « Score /100 »), sinon elle est devinée valeur par valeur (un 4 est lu sur 5).
- Une valeur illisible n'écarte pas la ligne : elle est importée sans cette valeur et signalée. Maximum 3000 lignes, 8 Mo.
- Non testé ici contre le vrai IGDB : si un titre courant tombe en « introuvable », ajoute-le depuis l'onglet Rechercher.

## Sécurité des clés
- Windows : Gestionnaire d'identifiants (entrées « Backlog Panda ») ; macOS : Trousseau d'accès ; ailleurs : fichier de réglages (droits 0600).
- Les clés déjà saisies dans une version précédente sont déplacées dans le coffre au premier lancement, et effacées du fichier seulement après relecture du coffre.
- `BACKLOG_PANDA_NO_KEYRING=1` force le fichier (mode portable). Les variables `IGDB_CLIENT_SECRET` et `DEEPL_API_KEY` restent prioritaires.
- Ce que cela protège : la copie ou la lecture du dossier de données. Ce que cela ne protège pas : un programme malveillant déjà lancé sous ta session Windows.

## Fabriquer le .exe (Windows)
1. Prérequis, une seule fois : Rust (`winget install Rustlang.Rustup`) et les outils C++ de Visual Studio (le script te donne la commande s'ils manquent). WebView2 est déjà présent sur Windows 11 et les Windows 10 à jour.
2. Double-clique sur `build-windows.bat` (ou `powershell -ExecutionPolicy Bypass -File tools\build-windows.ps1`).
3. L'exécutable autonome (environ 5 Mo, sans installation) est créé dans `dist\Backlog Panda.exe`. La première compilation prend 5 à 15 minutes.
- Windows SmartScreen peut avertir au premier lancement (l'exe n'est pas signé) : « Informations complémentaires », puis « Exécuter quand même ».
- En cas d'échec au démarrage, l'application affiche un message et écrit `erreur.txt` dans `%APPDATA%\BacklogPanda\`.
- Pour un installateur : `cargo install tauri-cli --version "^2"` puis `cargo tauri build` dans `src-tauri` (cible NSIS déjà configurée).

## Licence
MIT, © 2026 Captain Panda : voir [LICENSE](LICENSE).
