"""Parcours des dates d'ajout et de fin (Playwright), avec captures.

Usage : backlog-panda app --no-open --demo --data-dir /tmp/bp-d --port 47903 &
        python3 tools/visual/dates_flow.py [dossier_de_sortie]
"""
import os, sys
from playwright.sync_api import sync_playwright
URL = 'http://127.0.0.1:47903/'
OUT = (sys.argv[1] if len(sys.argv) > 1 else '/tmp/bp-shots3').rstrip('/') + '/'
os.makedirs(OUT, exist_ok=True)
with sync_playwright() as p:
    b = p.chromium.launch()
    pg = b.new_page(viewport={'width': 1440, 'height': 1000})
    logs = []
    pg.on('console', lambda m: logs.append(f'{m.type}: {m.text}'))
    pg.on('pageerror', lambda e: logs.append(f'PAGEERROR {e}'))
    pg.route('https://images.igdb.com/**', lambda r: r.abort())
    pg.goto(URL); pg.wait_for_selector('.card')
    # tri par date d'ajout
    pg.select_option('.sort select', value='added'); pg.wait_for_timeout(500)
    pg.screenshot(path=OUT + 'a-sort-added.png')
    # fiche d'un jeu terminé
    pg.click('.tabs >> text=Terminé'); pg.wait_for_timeout(400)
    pg.query_selector_all('.grid .card')[0].click(); pg.wait_for_selector('#f-finished'); pg.wait_for_timeout(700)
    pg.screenshot(path=OUT + 'b-sheet-dates.png')
    print('ajouté:', pg.input_value('#f-added'), 'fini:', pg.input_value('#f-finished'), '|', pg.inner_text('.dates .hint'))
    pg.fill('#f-added', '2024-03-05'); pg.fill('#f-finished', '2024-04-20'); pg.wait_for_timeout(700)
    print('après saisie:', pg.inner_text('.dates .hint'))
    pg.screenshot(path=OUT + 'c-sheet-edited.png')
    # on passe à En cours : le champ de fin disparaît
    pg.click('#drawer .pill >> text=En cours'); pg.wait_for_timeout(700)
    print('champ fin après En cours:', pg.query_selector('#f-finished') is not None, '|', pg.inner_text('.dates .hint'))
    pg.click('#drawer .pill >> text=Terminé'); pg.wait_for_timeout(700)
    print('fin reposée automatiquement:', pg.input_value('#f-finished'))
    pg.keyboard.press('Escape'); pg.wait_for_timeout(500)
    pg.click('nav button:nth-child(3)'); pg.wait_for_selector('.tiles'); pg.wait_for_timeout(1200)
    pg.screenshot(path=OUT + 'd-stats.png')
    pg.evaluate("document.querySelector('#view').scrollTop=600"); pg.wait_for_timeout(500)
    pg.screenshot(path=OUT + 'e-stats-timeline.png')
    pg.set_viewport_size({'width': 420, 'height': 900}); pg.click('nav button:nth-child(1)'); pg.wait_for_selector('.card')
    pg.query_selector_all('.grid .card')[0].click(); pg.wait_for_timeout(900)
    pg.screenshot(path=OUT + 'f-narrow-sheet.png')
    print('\n'.join(logs) or 'no console output')
    b.close()
