# Journal des choix : Backlog Panda (application de bureau)

Document vivant pour itérer sur le visuel. Chaque choix indique sa raison, et ce qui reste à valider.
Légende : **Vérifié** = testé ou capturé ici ; **Non vérifié** = écrit mais pas exécuté dans les conditions réelles.

## 1. Demande d'origine

Une application personnelle, utilisable sur un PC Windows, « dans l'esprit de Backloggd » (jaquettes, statuts, notes, recherche) sans copier son apparence : sombre, propre, soignée, avec des effets sur les boutons et des animations, et peu gourmande en RAM.

## 2. Architecture

| Choix | Raison | Statut |
|---|---|---|
| **Tauri 2** (fenêtre native + interface web) | Rendu moderne (CSS) et faible RAM : pas de navigateur embarqué, Windows fournit WebView2. | **Vérifié sous Linux** : `cargo build` puis fenêtre lancée dans un écran virtuel (WebKitGTK) : interface, polices, CSP et commande `rpc` fonctionnent (bibliothèque chargée via IPC). **Windows/WebView2 non vérifié** |
| Toute la logique en **Rust** (`service.rs`) | Demande explicite : le JS n'affiche que. Un seul point d'entrée `Service::call(méthode, arguments)`. | Vérifié (68 tests) |
| Même moteur, **deux enveloppes** | Tauri (`invoke('rpc')`) et serveur local (`POST /rpc/<méthode>`) partagent `ui/` : permet de tester l'interface dans un navigateur et de lancer l'app sans installer Tauri. | Vérifié |
| **IGDB** comme source de jeux | Backloggd n'a pas d'API publique ; ses fiches viennent d'IGDB. Les identifiants Backloggd et IGDB semblent identiques (déduit, non vérifié) : le catalogue hors ligne reste donc valide une fois IGDB configuré. | Client testé avec faux transport ; **jamais appelé en vrai** (réseau bloqué ici) |
| Données **locales** : `bibliotheque.json` | Pas de serveur distant. Écriture atomique, copie `.bak`, fichier corrompu mis de côté au lieu d'être écrasé. | Vérifié |
| Secret IGDB et clé DeepL dans le coffre du système (Windows : Gestionnaire d'identifiants ; macOS : Trousseau), sinon dans `reglages.json` | Chiffrer le fichier avec une clé stockée à côté n'aurait rien protégé ; le coffre du système est lié à la session. Migration automatique des anciennes clés, avec relecture avant effacement du fichier ; si le coffre refuse ou « oublie », la clé reste dans le fichier plutôt que d'être perdue. Les variables d'environnement ont priorité. Le secret n'est jamais renvoyé à l'interface ni affiché dans une erreur (testé). | Logique vérifiée par des tests avec faux coffres ; **le coffre Windows réel n'a pas pu être exercé** (compilation sous Linux seulement) |
| Fiche de jeu **copiée dans le fichier local** à l'ajout | La bibliothèque se lit hors ligne, jaquette comprise (l'image elle-même vient d'IGDB, donc du réseau). | Vérifié |
| Une seule instance | Deux instances écriraient le même fichier. Tauri : plugin single-instance ; mode navigateur : port fixe 47821, déjà pris = on rouvre l'existante. | Mode navigateur vérifié ; plugin Tauri compilé, non exécuté sous Windows |

### Sécurité du serveur local (mode navigateur)
127.0.0.1 uniquement ; en-tête `Host` limité (anti DNS-rebinding) ; `Origin` vérifié ; en-tête `X-Panda: 1` obligatoire (force une requête CORS préalable, jamais acceptée) ; CSP stricte sans `unsafe-inline` ; un fil par connexion avec délai de lecture (les navigateurs ouvrent des connexions de réserve). Tests dans `src/webapp.rs`.

## 3. Direction visuelle

**Sujet** : un panda qui range ton backlog. Les jaquettes sont le contenu ; l'interface s'efface autour.

**Palette** (bleu nuit, pas de noir pur)
- fond `#080b17`, rail `#0c1022`, panneaux `#12172e` / `#1a2040`, texte `#eef0fb`, secondaire `#a6acca`, discret `#70769a`
- **un code couleur par statut** : backlog `#6aa8ff`, en cours `#35d6a4`, terminé `#ffc24b`, joué `#a98bff`, abandonné `#ff6f86`, envie `#ff8fd0` (mêmes teintes partout : onglets, pastilles, répartition)
- étoiles `#ffc24b` ; bouton principal clair `#eef0ff` sur fond sombre (contraste net, évite l'accent « chaud » courant)

**Typographie** (polices locales, licence OFL, aucune requête externe)
- *Bricolage Grotesque* (titres, noms de jeux) : caractère, légèrement condensé ; *Figtree* (texte et commandes) : lisible, neutre.
- Casse de phrase partout, pas de majuscules espacées, pas de numérotation décorative.

**Mise en page** : rail de navigation à gauche (icônes seules sous 960 px), recherche toujours visible en haut (`Ctrl K` ou `/`), étagère « Reprendre » (jeux en cours), grille de jaquettes 3:4, fiche de jeu en fenêtre centrée (voir « Retours d'utilisation »). Alignement à gauche.

**Le moment mémorable** : au survol, une **copie floue et saturée de la jaquette** s'allume derrière elle (halo à sa couleur) et la carte s'incline légèrement vers le pointeur avec un reflet. La fiche du jeu reprend la jaquette floue en toile de fond. Tout le reste reste calme.

**Effets sur les boutons** (réponses à une action, pas de mouvement gratuit) : appui = léger rétrécissement ; bouton principal = balayage lumineux au survol ; pastilles de statut = rebond ; étoiles = « pop » en cascade ; bouton « + » = rotation ; notification avec « Annuler » après un retrait.

**Étoiles** : demi-étoiles (1 à 10 demi-points, comme Backloggd) ; survol = aperçu, clic sur la même note = effacer ; clavier : flèches, Début/Fin, Suppr.

**Jaquette de secours** : sans image (mode hors ligne), carte générée avec monogramme du sous-titre et teinte tirée du titre, pour que deux jeux d'une même série restent distinguables.

### Choix pour la RAM
- Halo **créé seulement au survol**, supprimé ensuite (pas des centaines de filtres floutés en mémoire).
- Images `loading="lazy"` en taille `cover_big` (pas de version 2x), `content-visibility: auto` sur les cartes.
- Pas de `backdrop-filter` (le fond derrière la fiche est simplement assombri), pas de bibliothèque JS, pas de framework : environ 55 Ko de HTML, CSS et JS au total.
- Mesure de RAM réelle **non faite** (voir §5).

### Accessibilité
Focus visible, cartes = boutons, rôle `slider` pour la note, fiche (`role=dialog`) `inert` quand fermée, focus rendu au déclencheur à la fermeture, `prefers-reduced-motion` respecté (inclinaison et animations coupées).

## 4. Inspirations consultées (brief design)
scrolltide, artofthetitle, motionographer, stashmedia, schoolofmotion (blog) : consultés pour l'ambiance et le mouvement. Behance n'a pas pu être consulté. Aucune mise en page copiée.

## 4 bis. Retours d'utilisation (premier essai réel) et réponses
| Remarque | Réponse |
|---|---|
| Descriptifs en anglais | IGDB ne fournit que l'anglais. Traduction **DeepL** facultative (clé dans Réglages), mémorisée dans le fichier ; bouton « Voir l'original ». Genres, thèmes, modes et perspectives traduits localement (table fixe). |
| Fiche à droite, peu visible | Fenêtre **centrée** sur fond assombri, 2 colonnes (jaquette + informations / ta part), une colonne sous 820 px. |
| Choix de la plateforme | Liste déroulante : plateformes de sortie du jeu, puis plateformes courantes ; « Non précisée » possible. |
| Commentaire | Zone de texte (10 000 caractères max), enregistrée 0,7 s après la frappe et à la fermeture ; pastille sur la jaquette quand un commentaire existe. |
| Manque d'informations | Date de sortie, studios, éditeurs, franchise, modes, perspective, thèmes. Les fiches anciennes sont complétées en arrière-plan. |
| Pas de filtre | Genre, plateforme, studio, note minimale ; menus avec effectifs ; bouton Réinitialiser. |

Limites de cette étape : les nouveaux champs IGDB (`involved_companies`, `game_modes`…) et DeepL n'ont **jamais été appelés pour de vrai** depuis mon environnement (réseau bloqué) ; seuls des faux serveurs locaux les ont testés. Si IGDB refuse les champs détaillés, le moteur retombe sur la requête de base (la fiche reste utilisable, sans studio/éditeur).

### Chiffrement des clés (demande après la deuxième série)
Réponse : coffre du système plutôt que chiffrement maison. Panneau « Tes données » dans Réglages : pastille « Clés chiffrées » / « Clés en clair » / « Aucune clé enregistrée » et explication adaptée. Limites : une copie du dossier sur un autre PC ne contient plus les clés (à ressaisir) ; un programme malveillant tournant sous ta session peut toujours lire le coffre ; non testé sur un vrai Windows.

### Deuxième série de retours
| Remarque | Réponse |
|---|---|
| Statistiques trop pauvres | 5 indicateurs en tête (jeux suivis, temps de jeu, terminés + taux de finition, note moyenne + médiane + coups de cœur, commentaires), puis mieux notés, plus longues parties, genres, plateformes, studios, éditeurs et époque. Calculs faits côté Rust, un seul appel `stats`. Barres d'une seule teinte : la longueur porte la valeur, les chiffres sont écrits à côté. |
| Heures jouées | Champ « Temps de jeu » dans la fiche. Saisie libre interprétée par le moteur (`12h30`, `12,5`, `45 min`, `1:30`), stockée en minutes, affichée « 12 h 30 ». Texte illisible : message d'erreur, valeur précédente conservée. Tri « Temps de jeu » dans la bibliothèque. |
| Guide des clés | Réglages : étapes numérotées (c'est une vraie séquence) pour IGDB/Twitch et DeepL, avec état « Configurée / Pas encore configurée », adresses à copier (les liens externes ne s'ouvrent pas dans la fenêtre de l'application), mise en garde sur le secret. Replié une fois les clés saisies. |
| Switch 2 absente | Ajoutée à la liste des plateformes courantes (avec Mac). Si IGDB la fournit pour un jeu, elle apparaît aussi dans « Sorti sur ». |

À savoir : les intitulés exacts du formulaire Twitch et du compte DeepL sont donnés de mémoire (non vérifiés en ligne) ; le guide le signale. L'ajout d'une date d'ajout (pour mesurer l'ancienneté du backlog) n'est pas faite : il faudrait horodater chaque entrée.

### Livraison de l'exécutable
Je n'ai pas pu produire de `.exe` moi-même : mon environnement est sous Linux et le téléchargement des composants de compilation Windows y est bloqué. Livré à la place : `build-windows.bat` (double-clic) qui vérifie Rust, la chaîne MSVC et WebView2, charge l'environnement Visual Studio (corrige la cause probable de l'erreur `LNK1104 msvcrt.lib`), compile en `--release` et copie `dist\Backlog Panda.exe`. Vérifié ici : la même version `--release` démarre en autonome sous Linux (interface embarquée, aucune dépendance à un serveur de développement) ; syntaxe du script validée avec PowerShell 7. **Mise à jour : l'utilisateur confirme que le build Windows a fonctionné.** Pas encore confirmé : le comportement du coffre d'identifiants sur son PC (enregistrer une clé, relancer) et la boîte de message d'erreur de démarrage.

### Sauvegarde, export et import (« le plus urgent » de la liste d'améliorations)
**Pourquoi d'abord ça** : toute la bibliothèque tient dans un seul fichier, et jusque-là la seule protection était une copie `.bak` écrasée à chaque écriture. Un fichier tronqué puis une deuxième écriture, et la copie saine disparaissait ; aucun export n'existait côté interface.

**Choix**
- *Un seul format.* L'export JSON est le fichier de données lui-même : export, sauvegarde datée et `bibliotheque.json` se lisent par le même code, donc tout ce que l'application écrit peut être réimporté.
- *Sauvegarde = état d'avant.* La copie du jour est prise juste avant l'écriture qui modifie le fichier (et au lancement s'il n'y en a pas), pas après : c'est ce qui permet de revenir en arrière. 15 copies, au plus une par 20 h, nom horodaté (UTC) ; les types `auto`, `manuelle`, `avant-import`, `avant-restauration` sont dans le nom.
- *Récupération automatique* : fichier principal illisible → conservé en `.corrompu-…`, puis `.bak`, puis dernière sauvegarde lisible. Une erreur d'E/S à la lecture n'est volontairement pas traitée comme une corruption (on ne renomme pas un bon fichier verrouillé par un antivirus). Un bandeau reste affiché jusqu'à « Compris ».
- *Trois modes d'import, aperçu calculé en appliquant réellement le mode sur une copie* : l'aperçu ne peut donc pas promettre autre chose que ce qui arrivera. « Fusionner » reprend la fusion « dernier écrivain gagnant » du module de synchronisation ; elle compare des compteurs de Lamport, pas des dates : fiable pour une copie de la même bibliothèque, **pas** pour deux bibliothèques construites séparément sur deux ordinateurs (d'où « Ajouter les jeux qui manquent », proposé en premier).
- *Aucune importation sans filet* : sauvegarde « avant import » obligatoire (échec = import annulé) ; les entrées dont la fiche de jeu manque sont écartées et comptées ; les fichiers d'une version plus récente sont refusés.
- *Les fichiers sont écrits par le moteur Rust, pas par le navigateur.* Je ne pouvais pas vérifier le comportement des téléchargements de WebView2 ; écrire dans *Téléchargements* et afficher le chemin (avec « Afficher dans le dossier ») fonctionne pareil dans la fenêtre et dans le mode navigateur. Seul l'import passe par le navigateur (`<input type="file">`).
- *CSV* : `;` et virgule décimale pour l'Excel français, BOM UTF-8 pour les accents, cellules commençant par `=`, `+`, `-`, `@` préfixées d'une apostrophe (sinon un commentaire comme « =1+1 » serait exécuté par Excel).
- Limite du serveur local relevée à 32 Mio (elle était à 2 Mio, ce qui aurait refusé l'import d'une bibliothèque de ~2 000 jeux en mode navigateur).

**Vérifié ici** : 130 tests (dont : rotation, collision de secondes, récupération `.bak` / sauvegarde / rien, trois modes d'import et leur aperçu, fusion avec fichier plus récent, fichiers invalides, noms de sauvegarde piégés, CSV) ; parcours complet dans Chromium (export, import avec aperçu, restauration, fichier invalide, bandeau après fichier tronqué) ; import réel d'un fichier de 4 Mo / 4 000 jeux par le serveur local ; l'exécutable Tauri `--release` démarre et affiche l'interface.
**Non vérifié** : le sélecteur de fichier et l'ouverture de l'Explorateur sous Windows (`explorer.exe /select,…`) ; le code Windows de ce dernier n'a pas été compilé ici. Sauvegardes : le fuseau horaire est lu côté fenêtre, donc l'heure affichée est locale mais le nom du fichier est en UTC.

### Dates d'ajout et de fin
**Pourquoi** : sans date, impossible de dire depuis quand un jeu dort dans le backlog ni combien tu en finis par an (limite n°5 du journal, et suite logique de l'export).

**Choix**
- *Deux champs sur l'entrée* (`added`, `finished`, secondes Unix, absents des anciens fichiers donc lus comme « inconnu »). Aucune date n'est inventée pour les jeux déjà présents : `added` reste vide tant que tu ne la saisis pas, et ils sont exclus des moyennes d'ancienneté (la page Statistiques indique combien).
- *Automatique mais jamais intrusif.* Ajout = date du jour. La fin est posée quand le statut **change** vers Terminé, Joué ou Abandonné ; passer de l'un à l'autre ne la déplace pas ; reprendre le jeu ou le remettre en attente l'efface (une nouvelle fin sera datée à nouveau) ; re-cliquer le même statut ne fait rien. Retirer un jeu puis le ré-ajouter repart de zéro, mais « Annuler » juste après un retrait rend les dates d'origine.
- *Modifiables à la main* dans la fiche. Une date de fin n'est acceptée que pour un statut de fin ; dates comprises entre 1980 et aujourd'hui.
- *Fuseau.* L'interface affiche et saisit en heure locale ; une date saisie est enregistrée à midi pour qu'un changement de fuseau ne la décale pas d'un jour. Le moteur compte les années en UTC (écart possible seulement dans les heures autour du 31 décembre) et le CSV écrit des dates ISO en UTC.
- *Statistiques* : ancienneté moyenne du backlog (statut Backlog uniquement, pas Envie), les 5 plus anciens, terminés par année (statut Terminé seulement), délai moyen ajout → fin.
- Les dates font partie de ce que « Fusionner » compare à l'import.

**Vérifié ici** : 137 tests (dont : dates automatiques et cas limites, anciens fichiers sans dates, retrait puis « Annuler », dates refusées, tri, statistiques, CSV) ; parcours dans Chromium (fiche, saisie, changement de statut, tri, statistiques, écran étroit).
**Non vérifié** : l'aspect du champ de date sous WebView2 (Windows). Il devrait suivre la langue du système (jj/mm/aaaa) ; Chromium de test l'affiche en mm/jj/aaaa.

### Tri par plateforme et import d'une liste CSV
**Pourquoi** : le tri par plateforme manquait pour planifier « ce que je joue sur Switch » ; et sans import de liste, repartir de zéro depuis un tableur ou un autre site obligeait à tout ressaisir.

**Choix**
- *Tri « Plateforme »* : ordre alphabétique de la plateforme que tu as choisie dans la fiche, les jeux sans plateforme en dernier.
- *Fichier lu par le moteur Rust* (`csv_import.rs`), pas par l'interface : séparateur `;` `,` ou tabulation deviné, guillemets, BOM, colonnes reconnues par leur titre (français ou anglais, accents et casse ignorés), colonnes inconnues ignorées et montrées à l'écran. Seule la colonne des titres est obligatoire. Statuts français ou anglais, notes sur 5, 10 ou 100 ou en ★, durées (`12h30`, `45 min`, `12,5`), dates ISO ou jj/mm/aaaa (jours vérifiés selon le mois), colonne « Identifiant IGDB » si elle existe. Maximum 3000 lignes.
- *Une valeur illisible n'écarte pas la ligne* : elle est importée sans cette valeur et l'écran de vérification l'indique (« Ligne 8 : note illisible »). Une date de fin n'est gardée que si le statut est un statut de fin.
- *Rapprochement titre → jeu en trois temps, sans rien ajouter* : (1) identifiant IGDB du fichier, par lots ; (2) jeu déjà présent dans la bibliothèque ; (3) recherche par titre (IGDB, environ trois requêtes par seconde pour rester sous la limite, ou catalogue intégré sans clé). Résultat par ligne : **trouvé** (un seul titre identique, avec la bonne année si donnée), **à vérifier** (plusieurs candidats : l'utilisateur choisit dans une liste de cinq), **introuvable**, **déjà présent**.
- *Prudence à l'ajout* : seuls les jeux cochés sont ajoutés (les « trouvés » sont cochés d'office, les « à vérifier » non) ; un jeu déjà dans la bibliothèque n'est jamais modifié ; deux lignes qui désignent le même jeu n'en font qu'un ; le jeu choisi doit faire partie des candidats proposés pour la ligne (le moteur refuse sinon) ; une sauvegarde « avant import » est faite d'abord. La date d'ajout vient du fichier ; sans colonne de date, elle reste vide plutôt que d'afficher « aujourd'hui ».
- *Réseau* : l'attente et la progression sont gérées par lots de cinq lignes ; en cas de coupure, trois essais puis un bouton « Reprendre » qui ne refait pas le travail déjà fait.
- *L'export CSV de l'application se réimporte* (grâce à la colonne « Identifiant IGDB ») : statuts, notes, plateformes, heures, commentaires et dates reviennent.

**Vérifié ici** : 151 tests (lecture du fichier, fichiers inacceptables, rapprochement hors ligne et avec un IGDB simulé, choix refusés, appels hors séquence, coupure réseau, aller-retour export → import de 18 jeux) ; parcours Chromium complet (choix du fichier, colonnes, progression, vérification, filtres, ajout, réimport tout « déjà présent », écran étroit) ; un import de 4000 lignes par le vrai serveur local.
**Non vérifié** : la recherche par titre contre le **vrai** IGDB (jamais atteignable d'ici : qualité du classement des candidats et cadence de 280 ms entre requêtes à confirmer) ; le sélecteur de fichier sous WebView2. **Limite assumée** : sans indication d'échelle dans l'en-tête de la colonne de notes (« Note /10 »), l'échelle est devinée valeur par valeur, donc un 4 dans un fichier sur 10 est lu comme 4 sur 5.

## 5. Limites connues et prochaines itérations
1. **IGDB jamais testé en conditions réelles** : syntaxe de requête et obtention du jeton écrites de mémoire (champs confirmés via les définitions de types publiques). À valider dès que les clés sont saisies : bouton « Tester la connexion ».
2. **Windows non vérifié** : la compilation n'a été faite que sous Linux. Sur ton PC il faut les outils C++ de Visual Studio (erreur `LNK1104` rencontrée).
3. **Aucune mesure de RAM** sur Windows : à relever dans le Gestionnaire des tâches après quelques minutes d'usage.
4. Jaquettes de test **synthétiques** dans les captures : le rendu du halo avec de vraies images reste à juger.
5. Pas encore : import direct depuis Backloggd (le CSV générique couvre un export fait à la main). (Fait depuis : export/import, sauvegardes datées, récupération automatique ; clés dans le coffre Windows ; tri par plateforme ; import CSV.)
6. Pistes visuelles : thème clair optionnel, densité de grille réglable, bascule grille/liste, transition de la jaquette vers le tiroir.
