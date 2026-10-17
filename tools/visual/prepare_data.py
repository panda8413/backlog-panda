"""Prépare un dossier de données avec des jaquettes SYNTHÉTIQUES pour juger le rendu visuel.

Usage : backlog-panda app --demo --data-dir /tmp/bp-demo --no-open  (une fois, puis Ctrl+C)
        python3 tools/visual/prepare_data.py /tmp/bp-demo /tmp/bp-vis
Les fiches reçoivent des identifiants d'image fictifs (co00, co01...) ; screenshots.py intercepte
les requêtes vers images.igdb.com et sert les images générées ici.
"""
import sys
import json, os, random, colorsys, shutil
from PIL import Image, ImageDraw, ImageFilter
src=(sys.argv[1] if len(sys.argv)>1 else '/tmp/bp-demo')+'/bibliotheque.json'; dst=sys.argv[2] if len(sys.argv)>2 else '/tmp/bp-vis'
shutil.rmtree(dst, ignore_errors=True); os.makedirs(dst+'/covers')
d=json.load(open(src))
random.seed(4)
devs=['Studio Aube','Atelier Nord','Fumée Games','Orbite Interactive']; pubs=['Éditions Lune','Grand Phare']
comments=['Superbe ambiance, à finir avant décembre.','Trop de va-et-vient pour moi.']
plats=[['Switch'],['PC','PS5'],['PC','Switch','Xbox Series X|S'],['PS4'],['GameCube'],['Wii U','Switch']]
gen=[['Aventure','RPG'],['Action','Roguelike'],['Plateforme'],['Course'],['Réflexion'],['Tir']]
summ="Un monde vaste et mystérieux t'attend. Explore, combats et découvre une histoire qui se dévoile à ton rythme, entre moments calmes et affrontements intenses, avec une direction artistique remarquable."
for i,(k,g) in enumerate(d['games'].items()):
    g['cover']=f'co{i:02d}'
    g['platforms']=plats[i%len(plats)]; g['genres']=gen[i%len(gen)]; g['summary']=summ if i%3 else None
    g['rating']=70+random.random()*25
    g['developers']=[devs[i%len(devs)]]; g['publishers']=[pubs[i%len(pubs)]]
    g['modes']=['Un joueur'] + (['Coopération'] if i%2 else []); g['themes']=['Fantasy'] if i%2 else ['Science-fiction']
    g['perspectives']=['À la troisième personne']; g['franchise']='Série fictive %d' % (i%3); g['released']=1000000000+i*60000000
    g['rev']=2
    if i%4==0: g['summary_fr']="Un vaste monde mystérieux t'attend. Explore, combats et découvre une histoire qui se dévoile à ton rythme."
    if g['summary'] is None and i%4==0: g['summary']=summ
    # image
    h=random.random(); W,H=264,374
    im=Image.new('RGB',(W,H))
    px=im.load()
    c1=colorsys.hsv_to_rgb(h,.7,.55); c2=colorsys.hsv_to_rgb((h+.12)%1,.8,.15)
    for y in range(H):
        t=y/H
        for x in range(W):
            px[x,y]=tuple(int(255*(c1[j]*(1-t)+c2[j]*t)) for j in range(3))
    dr=ImageDraw.Draw(im,'RGBA')
    cx,cy=random.randint(60,200),random.randint(90,200)
    for r in range(120,0,-12):
        a=colorsys.hsv_to_rgb((h+.5)%1,.5,1)
        dr.ellipse([cx-r,cy-r,cx+r,cy+r],fill=tuple(int(255*v) for v in a)+(18,))
    dr.polygon([(0,H),(W,H-random.randint(60,140)),(W,H)],fill=(0,0,0,110))
    dr.rectangle([14,14,W-14,60],fill=(0,0,0,60))
    im.save(f'{dst}/covers/co{i:02d}.jpg',quality=85)
for j,(k,e) in enumerate(d['library']['entries'].items()):
    if j%3==0: e['comment']=comments[j%2]
    if j%2==0: e['platform']=['PC','Switch','PS5'][j%3]
json.dump(d,open(dst+'/bibliotheque.json','w'),ensure_ascii=False,indent=1)
print(len(d['games']))
