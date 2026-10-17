"""Captures d'écran de l'interface (Playwright + Chromium) pour itérer sur le design.

Usage : backlog-panda app --no-open --data-dir /tmp/bp-vis --port 47901 &
        python3 tools/visual/screenshots.py [dossier_de_sortie]
"""
import sys, re
from playwright.sync_api import sync_playwright
URL='http://127.0.0.1:47901/'
OUT=(sys.argv[1] if len(sys.argv)>1 else '/tmp/bp-shots').rstrip('/')+'/'
import os; os.makedirs(OUT, exist_ok=True)
def cover(route):
    m=re.search(r'/(co\d+)\.jpg',route.request.url)
    route.fulfill(path=f'/tmp/bp-vis/covers/{m.group(1)}.jpg',content_type='image/jpeg') if m else route.abort()
with sync_playwright() as p:
    b=p.chromium.launch()
    pg=b.new_page(viewport={'width':1440,'height':900})
    logs=[]
    pg.on('console',lambda m: logs.append(f'{m.type}: {m.text}'))
    pg.on('pageerror',lambda e: logs.append(f'PAGEERROR {e}'))
    pg.route('https://images.igdb.com/**',cover)
    pg.goto(URL); pg.wait_for_selector('.card'); pg.wait_for_timeout(700)
    pg.screenshot(path=OUT+'1-library.png')
    # hover glow
    cards=pg.query_selector_all('.grid .card')
    cards[2].hover(); pg.wait_for_timeout(700)
    pg.screenshot(path=OUT+'2-hover.png')
    # filtres
    pg.select_option('.filters .flt:nth-child(1) select', index=1); pg.wait_for_timeout(500)
    pg.screenshot(path=OUT+'2b-filtered.png')
    pg.click('.filters .btn'); pg.wait_for_timeout(400)
    # fiche
    cards=pg.query_selector_all('.grid .card')
    cards[2].click(); pg.wait_for_timeout(900)
    pg.screenshot(path=OUT+'3-sheet.png')
    st=pg.query_selector('#drawer .stars'); box=st.bounding_box()
    pg.mouse.click(box['x']+box['width']*0.7, box['y']+box['height']/2); pg.wait_for_timeout(700)
    pg.fill('#f-comment','Excellent jeu, la fin m\'a surpris.'); pg.wait_for_timeout(1200)
    pg.select_option('#f-platform', label='Switch 2'); pg.fill('#f-hours','12h30'); pg.keyboard.press('Enter'); pg.wait_for_timeout(500)
    pg.screenshot(path=OUT+'3b-sheet-edited.png')
    pg.keyboard.press('Escape'); pg.wait_for_timeout(700)
    pg.screenshot(path=OUT+'3c-after-close.png')
    pg.set_viewport_size({'width':700,'height':800}); pg.query_selector_all('.grid .card')[0].click(); pg.wait_for_timeout(900)
    pg.screenshot(path=OUT+'3d-sheet-narrow.png'); pg.keyboard.press('Escape'); pg.wait_for_timeout(500)
    pg.set_viewport_size({'width':1440,'height':900})
    # search
    pg.keyboard.press('Control+k'); pg.keyboard.type('zelda'); pg.wait_for_timeout(1200)
    pg.screenshot(path=OUT+'4-search.png')
    pg.click('nav button:nth-child(3)'); pg.wait_for_timeout(1200)
    pg.screenshot(path=OUT+'5-stats.png'); pg.evaluate("document.querySelector('#view').scrollTop=700"); pg.wait_for_timeout(500); pg.screenshot(path=OUT+'5b-stats-bottom.png'); pg.evaluate("document.querySelector('#view').scrollTop=0")
    pg.click('nav button:nth-child(4)'); pg.wait_for_timeout(800)
    pg.screenshot(path=OUT+'6-settings.png')
    pg.screenshot(path=OUT+'6b-settings-full.png', full_page=False)
    pg.set_viewport_size({'width':420,'height':860}); pg.click('nav button:nth-child(1)'); pg.wait_for_timeout(600)
    pg.screenshot(path=OUT+'7-narrow.png')
    print('\n'.join(logs) or 'no console output')
    b.close()
