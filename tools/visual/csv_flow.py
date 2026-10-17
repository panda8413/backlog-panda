"""Parcours de l'import CSV (Playwright), avec captures.

Usage : backlog-panda app --no-open --data-dir /tmp/bp-csv --port 47904 &
        python3 tools/visual/csv_flow.py [dossier_de_sortie]
"""
import os, sys
from playwright.sync_api import sync_playwright
URL = 'http://127.0.0.1:47904/'
OUT = (sys.argv[1] if len(sys.argv) > 1 else '/tmp/bp-shots4').rstrip('/') + '/'
os.makedirs(OUT, exist_ok=True)
CSV = """Titre;Année;Statut;Note /10;Plateforme;Heures;Commentaire;Ajouté le;Terminé le;Notes perso
Hades;2020;Terminé;9;PC;32h;Excellent;12/03/2023;02/05/2023;x
Katana Zero;;Joué;7;Switch;5h30;;;;
Zelda;;En cours;;;;;01/02/2024;;
Un jeu qui n'existe pas du tout;;;;;;;;;
Celeste;;Backlog;8,5;;;;;;
Hades;;Backlog;;;;doublon;;;
Portal;;Abandonné;abc;PC;;;31/02/2020;;
"""
path = OUT + 'liste.csv'
open(path, 'w', encoding='utf-8').write(CSV)
with sync_playwright() as p:
    b = p.chromium.launch()
    pg = b.new_page(viewport={'width': 1440, 'height': 1000})
    logs = []
    pg.on('console', lambda m: logs.append(f'{m.type}: {m.text}'))
    pg.on('pageerror', lambda e: logs.append(f'PAGEERROR {e}'))
    pg.route('https://images.igdb.com/**', lambda r: r.abort())
    pg.goto(URL); pg.wait_for_timeout(800)
    pg.click('nav button:nth-child(4)'); pg.wait_for_selector('#import-csv'); pg.wait_for_timeout(500)
    pg.query_selector('#import-csv').scroll_into_view_if_needed()
    pg.screenshot(path=OUT + 'a-settings.png')
    pg.set_input_files('#import-csv input[type=file]', path)
    pg.wait_for_selector('.csv-cols'); pg.wait_for_timeout(400)
    pg.query_selector('#import-csv').scroll_into_view_if_needed()
    pg.screenshot(path=OUT + 'b-start.png')
    print('colonnes:', pg.inner_text('.csv-cols').replace('\n', ' | '))
    pg.click('text=Rechercher les jeux'); pg.wait_for_selector('.csv-list', timeout=20000); pg.wait_for_timeout(500)
    pg.query_selector('#import-csv').scroll_into_view_if_needed()
    pg.screenshot(path=OUT + 'c-review.png')
    for r in pg.query_selector_all('.csv-row'):
        print(r.get_attribute('data-kind'), '|', r.inner_text().replace('\n', ' / '))
    print('bouton:', pg.inner_text('.csv-actions .btn.primary'))
    pg.click('text=Cocher le premier choix')
    print('après cocher:', pg.inner_text('.csv-actions .btn.primary'))
    pg.click('.csv-tabs >> text=Introuvables'); pg.wait_for_timeout(300)
    pg.screenshot(path=OUT + 'd-notfound.png')
    pg.click('.csv-tabs >> text=Tous')
    pg.click('.csv-actions .btn.primary'); pg.wait_for_selector('.csv .status-msg.ok'); pg.wait_for_timeout(600)
    print('résultat:', pg.inner_text('.csv .status-msg.ok'))
    pg.screenshot(path=OUT + 'e-done.png')
    pg.click('nav button:nth-child(1)'); pg.wait_for_selector('.card'); pg.wait_for_timeout(600)
    print('cartes:', [c.inner_text().split('\n')[0] for c in pg.query_selector_all('.grid .card')])
    pg.screenshot(path=OUT + 'f-library.png')
    # réimport du même fichier : tout est « déjà présent »
    pg.click('nav button:nth-child(4)'); pg.wait_for_selector('#import-csv'); pg.wait_for_timeout(400)
    pg.set_input_files('#import-csv input[type=file]', path); pg.wait_for_selector('.csv-cols')
    pg.click('text=Rechercher les jeux'); pg.wait_for_selector('.csv-list', timeout=20000)
    print('2e import:', pg.inner_text('.csv > p').replace('\n', ' '))
    # étroit
    pg.set_viewport_size({'width': 420, 'height': 900}); pg.wait_for_timeout(400)
    pg.query_selector('#import-csv').scroll_into_view_if_needed()
    pg.screenshot(path=OUT + 'g-narrow.png')
    print('débordement horizontal:', pg.evaluate('document.documentElement.scrollWidth > document.documentElement.clientWidth'))
    print('\n'.join(logs) or 'no console output')
    b.close()
