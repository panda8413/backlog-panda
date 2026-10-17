"""Parcours export / import / sauvegardes dans le navigateur (Playwright), avec captures.

Usage : BACKLOG_PANDA_EXPORT_DIR=/tmp/bp-exp backlog-panda app --no-open --demo --data-dir /tmp/bp-bk --port 47902 &
        python3 tools/visual/backup_flow.py [dossier_de_sortie]
"""
import glob, os, sys
from playwright.sync_api import sync_playwright
URL = 'http://127.0.0.1:47902/'
OUT = (sys.argv[1] if len(sys.argv) > 1 else '/tmp/bp-shots2').rstrip('/') + '/'
os.makedirs(OUT, exist_ok=True)
with sync_playwright() as p:
    b = p.chromium.launch()
    pg = b.new_page(viewport={'width': 1440, 'height': 1000})
    logs = []
    pg.on('console', lambda m: logs.append(f'{m.type}: {m.text}'))
    pg.on('pageerror', lambda e: logs.append(f'PAGEERROR {e}'))
    pg.route('https://images.igdb.com/**', lambda r: r.abort())
    pg.goto(URL); pg.wait_for_selector('.card')
    pg.click('nav button:nth-child(4)'); pg.wait_for_selector('#sauvegarde .bk-block')
    pg.locator('#sauvegarde').scroll_into_view_if_needed(); pg.wait_for_timeout(600)
    pg.screenshot(path=OUT + 'a-panel.png')
    pg.click('text=Exporter en JSON'); pg.wait_for_selector('.bk-result .path'); pg.click('text=Exporter pour Excel (CSV)'); pg.wait_for_timeout(500)
    print('exports :', sorted(os.path.basename(f) for f in glob.glob('/tmp/bp-exp/*')))
    pg.screenshot(path=OUT + 'b-exported.png')
    # on modifie la bibliothèque (retrait d'un jeu) puis on importe l'export (mode ajouter)
    pg.click('nav button:nth-child(1)'); pg.wait_for_selector('.card'); n0 = len(pg.query_selector_all('.grid .card'))
    pg.query_selector_all('.grid .card')[0].click(); pg.wait_for_timeout(700)
    pg.click('#drawer >> text=Retirer'); pg.wait_for_timeout(700)
    pg.keyboard.press('Escape'); pg.wait_for_timeout(500)
    n1 = len(pg.query_selector_all('.grid .card')); print('cartes :', n0, '->', n1)
    pg.click('nav button:nth-child(4)'); pg.wait_for_selector('#sauvegarde .bk-block')
    jf = sorted(glob.glob('/tmp/bp-exp/*.json'))[0]
    pg.set_input_files('#sauvegarde input[type=file]', jf); pg.wait_for_selector('.bk-modes'); pg.wait_for_timeout(400)
    pg.locator('.bk-import').scroll_into_view_if_needed()
    pg.screenshot(path=OUT + 'c-preview.png')
    pg.click('.bk-import >> text=Importer'); pg.wait_for_timeout(900)
    pg.screenshot(path=OUT + 'd-imported.png')
    pg.click('nav button:nth-child(1)'); pg.wait_for_selector('.card'); print('cartes après import :', len(pg.query_selector_all('.grid .card')))
    # fichier invalide
    open('/tmp/bp-exp/mauvais.json', 'w').write('{"x": 1}')
    pg.click('nav button:nth-child(4)'); pg.wait_for_selector('#sauvegarde .bk-block')
    pg.set_input_files('#sauvegarde input[type=file]', '/tmp/bp-exp/mauvais.json'); pg.wait_for_selector('.bk-import .status-msg'); pg.wait_for_timeout(300)
    pg.locator('.bk-import').scroll_into_view_if_needed(); pg.screenshot(path=OUT + 'e-invalid.png')
    # restauration d'une sauvegarde
    pg.click('text=Sauvegarder maintenant'); pg.wait_for_timeout(500)
    pg.locator('.bk-list').scroll_into_view_if_needed(); pg.click('.bk li >> nth=0 >> text=Restaurer'); pg.wait_for_timeout(400)
    pg.screenshot(path=OUT + 'f-restore-ask.png')
    pg.click('text=Oui, restaurer'); pg.wait_for_timeout(700)
    pg.screenshot(path=OUT + 'g-restored.png')
    pg.set_viewport_size({'width': 420, 'height': 900}); pg.wait_for_timeout(400)
    pg.locator('#sauvegarde').scroll_into_view_if_needed(); pg.screenshot(path=OUT + 'h-narrow.png')
    print('\n'.join(logs) or 'no console output')
    b.close()
