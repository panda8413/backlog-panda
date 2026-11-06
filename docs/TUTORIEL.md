# Tutoriel : installer et utiliser Backlog Panda

Backlog Panda est une application Windows pour suivre ton backlog de jeux vidéo. Tout reste **sur ton ordinateur** : pas de compte, pas de serveur.

## 1. Installer

1. Va dans l'onglet **Releases** de ce dépôt et télécharge `Backlog Panda.exe` (environ 5 Mo). Il n'y a rien à installer : tu le ranges où tu veux et tu double-cliques.
2. *(Facultatif)* Vérifie que le fichier est intact. Dans PowerShell :
   ```
   Get-FileHash "$env:USERPROFILE\Downloads\Backlog Panda.exe" -Algorithm SHA256
   ```
   Compare le résultat avec l'empreinte SHA-256 indiquée dans la description de la release.
3. **Windows SmartScreen** peut afficher « Windows a protégé votre ordinateur », car l'application n'est pas signée numériquement (c'est payant). Clique sur **Informations complémentaires**, puis **Exécuter quand même**. Le code source est dans ce dépôt, tu peux le lire ou compiler toi-même (voir plus bas).
4. Prérequis : WebView2, déjà présent sur Windows 11 et les Windows 10 à jour. Sinon : <https://developer.microsoft.com/microsoft-edge/webview2/> (« Evergreen Bootstrapper »).

## 2. Premier lancement

Sans clé, l'application fonctionne en **mode hors ligne** avec un petit catalogue de 162 jeux, sans jaquettes : de quoi essayer. Pour chercher parmi tous les jeux, avec leurs images, il faut une clé IGDB gratuite.

### Obtenir la clé IGDB (gratuite, environ 5 minutes)
IGDB appartient à Twitch, la clé s'obtient donc via un compte Twitch.
1. Active la **double authentification** sur Twitch (Paramètres → Sécurité et confidentialité) : elle est obligatoire pour créer une application.
2. Ouvre <https://dev.twitch.tv/console>, puis **Applications → Enregistrer votre application**.
3. Remplis : un nom au choix ; URL de redirection OAuth `http://localhost` ; catégorie « Application Integration » (ou la plus proche) ; type de client « Confidentiel ».
4. Ouvre l'application créée, copie l'**Identifiant client**, puis clique sur **Nouveau secret** et copie-le.
5. Dans Backlog Panda : **Réglages**, colle les deux valeurs, enregistre, puis utilise **Tester la connexion**.

Le secret est rangé dans le Gestionnaire d'identifiants de Windows (chiffré, lié à ta session). Ne le partage avec personne.

### Traduction des résumés (facultatif)
Une clé **DeepL API Free** (<https://www.deepl.com/pro-api>) permet de traduire les résumés en français. Sans elle, les genres, thèmes et modes de jeu sont tout de même traduits.

## 3. Utiliser l'application

- **Rechercher** (onglet Rechercher ou `Ctrl + K`) : tape un titre, puis ajoute le jeu avec un statut : Backlog, En cours, Terminé, Joué, Abandonné ou Envie.
- **Bibliothèque** : filtre par statut, genre, plateforme, studio, note minimale, durée estimée (moins de 5 h, 5 à 15 h, 15 à 40 h, plus de 40 h) ; trie par dernière modification, note, nom, année, temps de jeu, durée estimée, date d'ajout ou plateforme.
- **Sections de la vue « Tous »** : les jeux sont rangés par statut. Chaque section affiche une rangée ; clique sur **Voir les N** pour tout afficher, sur **Réduire** pour revenir à une rangée, ou sur le chevron pour fermer la section (Abandonné est fermée au départ). Ton choix est retenu.
- **Thème** : **Réglages → Apparence** (Selon Windows, Clair ou Sombre), ou le bouton soleil/lune en bas de la barre de gauche.
- **Fiche d'un jeu** (clic sur la jaquette) : statut, plateforme jouée, note en demi-étoiles, temps de jeu (`12h30`, `12,5`, `45 min`), dates d'ajout et de fin, commentaire (enregistré tout seul). Avec IGDB, la fiche affiche aussi « Temps pour finir » (rapide, normal, complet, d'après les joueurs IGDB ; « peu de votes » veut dire moins de 5 joueurs), le lien « Comparer sur HowLongToBeat », et, si tu as renseigné ton temps de jeu, où tu en es par rapport au temps normal.
- **Statistiques** : temps total, taux de finition, notes, ancienneté du backlog, jeux terminés par année, répartitions.

## 4. Sauvegarder, exporter, importer

Tout est dans **Réglages → Sauvegarde et export**.
- Une copie datée de ta bibliothèque est faite automatiquement une fois par jour ; les 15 dernières sont gardées, avec un bouton **Restaurer**. Si le fichier principal est abîmé, l'application se rétablit seule depuis la dernière bonne copie.
- **Exporter** en JSON (réimportable, contient tout) ou en CSV pour Excel. Les fichiers arrivent dans *Téléchargements*.
- **Importer** un JSON avec aperçu de ce qui va changer (ajouter les manquants, fusionner ou tout remplacer).
- **Importer une liste (CSV)** depuis un tableur ou un autre site : il suffit d'une colonne de titres. Indique l'échelle des notes dans l'en-tête (« Note /10 »). Les jeux sont rapprochés puis proposés à la vérification avant tout ajout.

Tes données sont dans `%APPDATA%\BacklogPanda\`. Pour changer d'ordinateur : exporte en JSON, puis importe sur l'autre machine (les clés ne suivent pas, il faut les ressaisir).

## 5. Problèmes fréquents

| Symptôme | Que faire |
|---|---|
| SmartScreen bloque | Informations complémentaires → Exécuter quand même |
| La fenêtre ne s'ouvre pas / écran blanc | Installer WebView2 (lien ci-dessus). L'application écrit `erreur.txt` dans `%APPDATA%\BacklogPanda\` en cas d'échec au démarrage |
| « Mode hors ligne » en bas à gauche | Aucune clé IGDB enregistrée : voir la section 2 |
| La recherche ne trouve rien | Utilise « Tester la connexion » dans Réglages ; vérifie l'identifiant et le secret |
| Un jeu du CSV est « introuvable » | Ajoute-le à la main depuis Rechercher |

## 6. Compiler toi-même

Prérequis : Rust (`winget install Rustlang.Rustup`) et les « Outils de build Visual Studio » avec la charge de travail *Développement Desktop en C++*. Puis double-clique sur `build-windows.bat` : l'exécutable est créé dans `dist\`. La première compilation prend 5 à 15 minutes.

Pour les tests : `cargo test`.
