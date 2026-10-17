'use strict';
/* Backlog Panda : affichage uniquement. Toute la logique (recherche, statuts, notes, stockage,
   appels réseau) est dans le cœur Rust, atteint par rpc(). Aucune donnée n'est injectée via
   innerHTML : tout passe par textContent / createElement. */
(() => {
  const NS = 'http://www.w3.org/2000/svg';
  const $ = (s, r = document) => r.querySelector(s);
  const IGDB_IMG = 'https://images.igdb.com/';

  const STATUSES = [
    { key: 'backlog', label: 'Backlog' },
    { key: 'playing', label: 'En cours' },
    { key: 'completed', label: 'Terminé' },
    { key: 'played', label: 'Joué' },
    { key: 'abandoned', label: 'Abandonné' },
    { key: 'wishlist', label: 'Envie' },
  ];
  const STATUS = Object.fromEntries(STATUSES.map((s) => [s.key, s]));
  const ENDED = ['completed', 'played', 'abandoned'];
  const END_LABEL = { completed: 'Terminé le', played: 'Fini de jouer le', abandoned: 'Abandonné le' };

  /* Dates : l'API échange des secondes Unix ; les champs de date et les durées sont en heure locale.
     Une date saisie est enregistrée à midi, ce qui évite qu'un changement de fuseau la déplace d'un jour. */
  const pad2 = (n) => String(n).padStart(2, '0');
  function dateToInput(secs) {
    if (secs == null) return '';
    const d = new Date(secs * 1000);
    return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
  }
  function inputToDate(v) {
    if (!v) return null;
    const [y, m, d] = v.split('-').map(Number);
    return Math.round(new Date(y, m - 1, d, 12).getTime() / 1000);
  }
  const daysSince = (secs) => Math.max(0, Math.floor((Date.now() / 1000 - secs) / 86400));
  function ageText(days) {
    if (days < 1) return 'aujourd’hui';
    if (days < 2) return '1 jour';
    if (days < 60) return `${days} jours`;
    const months = Math.floor(days / 30.44);
    if (months < 24) return `${months} mois`;
    const y = Math.floor(months / 12), m = months % 12;
    return m ? `${y} ans ${m} mois` : `${y} ans`;
  }
  const SORTS = [
    ['recent', 'Dernière modification'],
    ['rating', 'Ma note'],
    ['name', 'Nom'],
    ['year', 'Année de sortie'],
    ['hours', 'Temps de jeu'],
    ['added', 'Date d’ajout'],
    ['platform', 'Plateforme'],
  ];

  const ICONS = {
    library: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z',
    search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM20 20l-4-4',
    stats: 'M5 20V10M12 20V4M19 20v-7',
    settings: 'M4 7h10M18 7h2M4 17h2M10 17h10M14 4v6M6 14v6',
    plus: 'M12 5v14M5 12h14',
    close: 'M6 6l12 12M18 6L6 18',
    trash: 'M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13',
    note: 'M5 5h14v10H10l-5 4V5Z',
    star: 'M12 2.8l2.78 5.9 6.42.8-4.7 4.4 1.2 6.4L12 17.2 6.3 20.3l1.2-6.4-4.7-4.4 6.42-.8L12 2.8Z',
  };

  const state = {
    mode: 'offline',
    view: 'library',
    status: 'all',
    sort: 'recent',
    lib: { entries: [], counts: {} },
    query: '',
    found: null, // { source, results, warning, mine }
    stats: null,
    settings: null,
    open: null,
    cards: new Map(),
    filters: { genre: '', platform: '', developer: '', min: 0 },
    facets: { genres: [], platforms: [], developers: [] },
    total: 0,
    dirty: false,
    showOriginal: false,
  };
  let flushComment = null; // enregistre immédiatement le commentaire en cours de saisie
  let enrichingId = null;
  const refreshed = new Set(); // fiches déjà actualisées depuis IGDB pendant cette session
  const COMMON_PLATFORMS = ['PC', 'Mac', 'PS5', 'PS4', 'Xbox Series X|S', 'Xbox One', 'Switch 2', 'Switch', 'Steam Deck', 'Mobile'];

  /* ---------- Outils ---------- */

  function h(tag, props = {}, ...kids) {
    const n = document.createElement(tag);
    for (const [k, v] of Object.entries(props)) {
      if (v == null || v === false) continue;
      if (k === 'class') n.className = v;
      else if (k === 'text') n.textContent = v;
      else if (k === 'data') Object.assign(n.dataset, v);
      else if (k === 'vars') for (const [vk, vv] of Object.entries(v)) n.style.setProperty(vk, vv);
      else if (k.startsWith('on')) n.addEventListener(k.slice(2), v);
      else n.setAttribute(k, v === true ? '' : String(v));
    }
    for (const kid of kids.flat()) if (kid != null && kid !== false) n.append(kid);
    return n;
  }

  function icon(name, size = 20, solid = false) {
    const s = document.createElementNS(NS, 'svg');
    s.setAttribute('viewBox', '0 0 24 24');
    s.setAttribute('width', size);
    s.setAttribute('height', size);
    s.setAttribute('aria-hidden', 'true');
    s.setAttribute('class', solid ? 'icon solid' : 'icon');
    const p = document.createElementNS(NS, 'path');
    p.setAttribute('d', ICONS[name]);
    s.append(p);
    return s;
  }

  const fmt = (n) => new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 1 }).format(n);
  const tauri = window.__TAURI__ && window.__TAURI__.core;

  async function rpc(method, args = {}) {
    let res;
    if (tauri) {
      res = await tauri.invoke('rpc', { method, args });
    } else {
      const r = await fetch('rpc/' + method, {
        method: 'POST',
        headers: { 'X-Panda': '1', 'Content-Type': 'application/json' },
        body: JSON.stringify(args),
      });
      res = await r.json();
    }
    if (!res.ok) throw new Error((res.error || 'Erreur inconnue').replace(/^argument invalide : /, ''));
    return res.result;
  }

  function toast(text, action) {
    const box = $('#toasts');
    const t = h('div', { class: 'toast' }, h('span', { text }));
    let timer;
    const close = () => {
      clearTimeout(timer);
      t.classList.add('out');
      setTimeout(() => t.remove(), 280);
    };
    if (action) {
      t.append(h('button', { type: 'button', text: action.label, onclick: () => { close(); action.run(); } }));
    }
    box.append(t);
    while (box.children.length > 3) box.firstChild.remove();
    timer = setTimeout(close, action ? 7000 : 3800);
  }

  function remember(list) {
    for (const c of list) state.cards.set(c.id, c);
  }

  /* ---------- Jaquettes ---------- */

  function hueOf(name) {
    let x = 7;
    for (const ch of name) x = (x * 31 + ch.codePointAt(0)) >>> 0;
    return x % 360;
  }
  const SKIP = /^(the|of|a|an|le|la|les|de|du|des|and|et|hd|dx|remastered|edition)$/i;
  function initials(name) {
    // « Série : Sous-titre » : le sous-titre distingue les jeux d'une même série.
    const part = name.includes(':') ? name.slice(name.lastIndexOf(':') + 1) : name;
    const words = part.replace(/[^\p{L}\p{N} ]/gu, ' ').split(/\s+/).filter((w) => w && !SKIP.test(w));
    const pick = words.length ? words : [name];
    return pick.slice(0, 2).map((w) => Array.from(w)[0]).join('').toUpperCase() || '?';
  }
  function fallbackEl(name) {
    const hue = hueOf(name);
    return h('div', { class: 'fallback', vars: { '--h': hue, '--a': (hue % 4) * 45 + 'deg' } }, h('span', { text: initials(name) }));
  }
  function validCover(c) {
    return typeof c.cover_url === 'string' && c.cover_url.startsWith(IGDB_IMG);
  }
  function coverEl(c) {
    const wrap = h('div', { class: 'cover' });
    if (validCover(c)) {
      const img = h('img', { src: c.cover_url, alt: '', loading: 'lazy', decoding: 'async', draggable: 'false' });
      img.addEventListener('load', () => img.classList.add('in'));
      img.addEventListener('error', () => { img.remove(); wrap.prepend(fallbackEl(c.name)); });
      wrap.append(img);
    } else {
      wrap.append(fallbackEl(c.name));
    }
    return wrap;
  }

  /* ---------- Étoiles ---------- */

  function starsWidget(initial, onChange, opts = {}) {
    let value = initial || 0;
    const root = h('div', {
      class: 'stars' + (opts.readonly ? ' readonly' : '') + (opts.small ? ' sm' : ''),
      role: opts.readonly ? 'img' : 'slider',
      tabindex: opts.readonly ? null : 0,
      'aria-label': 'Ma note',
      'aria-valuemin': opts.readonly ? null : 0,
      'aria-valuemax': opts.readonly ? null : 5,
      'aria-valuenow': opts.readonly ? null : value || 0,
      'aria-valuetext': value ? `${fmt(value)} sur 5` : 'Pas de note',
    });
    const stars = [];
    for (let i = 0; i < 5; i++) {
      const mk = (cls) => {
        const s = document.createElementNS(NS, 'svg');
        s.setAttribute('viewBox', '0 0 24 24');
        s.setAttribute('class', cls);
        const p = document.createElementNS(NS, 'path');
        p.setAttribute('d', ICONS.star);
        s.append(p);
        return s;
      };
      const el = h('span', { class: 'star' }, mk('base'), mk('fill'));
      stars.push(el);
      root.append(el);
    }
    const paint = (v) => stars.forEach((s, i) => s.querySelector('.fill').style.setProperty('--w', Math.max(0, Math.min(1, v - i)) * 100 + '%'));
    paint(value || 0);
    if (opts.readonly) return root;

    const halfAt = (ev) => {
      const r = root.getBoundingClientRect();
      const pos = ((ev.clientX - r.left) / r.width) * 5;
      return Math.min(5, Math.max(0.5, Math.ceil(pos * 2) / 2));
    };
    const set = (v) => {
      value = v || 0;
      root.setAttribute('aria-valuenow', value);
      root.setAttribute('aria-valuetext', value ? `${fmt(value)} sur 5` : 'Pas de note');
      stars.forEach((s, i) => {
        if (v && i < Math.ceil(v)) {
          s.classList.remove('pop');
          void s.getBoundingClientRect();
          s.style.animationDelay = i * 40 + 'ms';
          s.classList.add('pop');
        }
      });
      onChange(v);
    };
    root.addEventListener('pointermove', (e) => paint(halfAt(e)));
    root.addEventListener('pointerleave', () => paint(value));
    root.addEventListener('click', (e) => { const v = halfAt(e); set(v === value ? null : v); });
    root.addEventListener('keydown', (e) => {
      const cur = value;
      const keys = { ArrowRight: cur + 0.5, ArrowUp: cur + 0.5, ArrowLeft: cur - 0.5, ArrowDown: cur - 0.5, Home: 0.5, End: 5 };
      if (e.key in keys) { e.preventDefault(); const v = Math.min(5, keys[e.key]); set(v <= 0 ? null : v); }
      else if (e.key === 'Delete' || e.key === 'Backspace') { e.preventDefault(); set(null); }
    });
    return root;
  }

  /* ---------- Cartes ---------- */

  const canTilt = matchMedia('(hover: hover) and (prefers-reduced-motion: no-preference)').matches;

  function statusBadge(st) {
    return h('span', { class: 'badge', vars: { '--c': `var(--st-${st})` } }, h('span', { class: 'd' }), STATUS[st].label);
  }

  function cardEl(c) {
    const st = c.entry && c.entry.status;
    const cover = coverEl(c);
    if (st) cover.append(statusBadge(st));
    if (c.entry && c.entry.stars) cover.append(h('span', { class: 'score' }, icon('star', 13, true), fmt(c.entry.stars)));
    if (c.entry && c.entry.comment) cover.append(h('span', { class: 'has-note', title: 'Commentaire écrit' }, icon('note', 14, true)));
    const tilt = h('div', { class: 'tilt' }, cover);
    const where = (c.entry && c.entry.platform) || (c.platforms && c.platforms[0]);
    const sub = [c.year, where, c.entry && c.entry.hours_text].filter(Boolean).join('  ');
    const hit = h('button', { class: 'hit', type: 'button', 'aria-label': `Ouvrir ${c.name}`, onclick: () => openGame(c.id) },
      tilt,
      h('div', { class: 'meta' }, h('span', { class: 'name', text: c.name }), sub ? h('span', { class: 'sub', text: sub }) : null));
    const art = h('article', { class: 'card', data: { id: c.id } }, hit);
    if (!st) {
      art.append(h('button', { class: 'quick', type: 'button', 'aria-label': `Ajouter ${c.name} au backlog`, onclick: () => add(c, 'backlog') }, icon('plus', 18)));
    }

    let glow = null;
    art.addEventListener('pointerenter', () => {
      if (glow && glow.isConnected) { glow.classList.add('on'); return; }
      glow = h('div', { class: 'glow' }, validCover(c) ? h('img', { src: c.cover_url, alt: '', class: 'in' }) : fallbackEl(c.name));
      tilt.prepend(glow);
      requestAnimationFrame(() => glow && glow.classList.add('on'));
    });
    art.addEventListener('pointerleave', () => {
      tilt.style.setProperty('--rx', '0deg');
      tilt.style.setProperty('--ry', '0deg');
      if (!glow) return;
      const g = glow;
      g.classList.remove('on');
      setTimeout(() => { if (!g.classList.contains('on')) { g.remove(); if (glow === g) glow = null; } }, 520);
    });
    if (canTilt) {
      let raf = 0;
      hit.addEventListener('pointermove', (e) => {
        if (raf) return;
        raf = requestAnimationFrame(() => {
          raf = 0;
          const r = hit.getBoundingClientRect();
          const x = (e.clientX - r.left) / r.width;
          const y = (e.clientY - r.top) / r.height;
          tilt.style.setProperty('--ry', ((x - 0.5) * 9).toFixed(2) + 'deg');
          tilt.style.setProperty('--rx', ((0.5 - y) * 9).toFixed(2) + 'deg');
          cover.style.setProperty('--mx', (x * 100).toFixed(1) + '%');
          cover.style.setProperty('--my', (y * 100).toFixed(1) + '%');
        });
      });
    }
    return art;
  }

  /* ---------- Actions métier (toutes déléguées au cœur Rust) ---------- */

  function apply(c) {
    state.cards.set(c.id, c);
    if (state.found) {
      state.found.results = state.found.results.map((r) => (r.id === c.id ? c : r));
      state.found.mine = state.found.mine.map((r) => (r.id === c.id ? c : r));
    }
    if (state.open && state.open.id === c.id) state.open = c;
  }

  async function guarded(fn) {
    try { return await fn(); } catch (e) { toast(e.message); return null; }
  }

  async function add(c, status) {
    const r = await guarded(() => rpc('add', { id: c.id, status }));
    if (!r) return;
    apply(r);
    toast(`${r.name} ajouté (${STATUS[status].label.toLowerCase()})`);
    await afterChange();
  }
  async function setStatus(c, status) {
    if (flushComment) await flushComment();
    const r = await guarded(() => (c.entry ? rpc('set_status', { id: c.id, status }) : rpc('add', { id: c.id, status })));
    if (!r) return;
    apply(r);
    await afterChange();
  }
  async function removeGame(c) {
    if (flushComment) await flushComment();
    const prev = c.entry;
    const ok = await guarded(() => rpc('remove', { id: c.id }));
    if (!ok) return;
    apply({ ...c, entry: null });
    flushComment = null;
    closeDrawer(true);
    await afterChange({ sheet: false });
    toast(`${c.name} retiré`, {
      label: 'Annuler',
      run: async () => {
        const r = await guarded(() => rpc('add', { id: c.id, status: prev.status, rating: prev.stars, platform: prev.platform, comment: prev.comment, minutes: prev.minutes, added: prev.added, finished: prev.finished }));
        if (r) { apply(r); await afterChange({ sheet: false }); }
      },
    });
  }

  async function setRating(c, v) {
    const r = await guarded(() => (c.entry ? rpc('set_rating', { id: c.id, rating: v }) : null));
    if (!r) return;
    apply(r);
    const lab = $('#drawer .rating-val');
    if (lab) { lab.textContent = r.entry.stars ? fmt(r.entry.stars) + ' / 5' : 'Pas encore noté'; lab.classList.toggle('none', !r.entry.stars); }
    state.dirty = true;
  }
  async function setPlatform(c, platform) {
    const r = await guarded(() => rpc('set_platform', { id: c.id, platform: platform || null }));
    if (!r) return;
    apply(r);
    state.dirty = true;
  }
  /** Saisie libre (« 12h30 », « 12,5 », « 45 min ») : le moteur l'interprète et renvoie le texte normalisé. */
  async function setHours(c, input) {
    const r = await guarded(() => rpc('set_hours', { id: c.id, hours: input.value }));
    if (!r) { input.value = (c.entry && c.entry.hours_text) || ''; return; }
    apply(r);
    c.entry = r.entry;
    input.value = r.entry.hours_text || '';
    state.dirty = true;
  }
  /** `patch` : { added } et/ou { finished }, en secondes Unix ou null pour effacer. */
  async function setDates(c, patch) {
    const r = await guarded(() => rpc('set_dates', { id: c.id, ...patch }));
    if (!r) return false;
    apply(r);
    c.entry = r.entry;
    state.dirty = true;
    return true;
  }
  async function translate(c) {
    if (flushComment) await flushComment();
    toast('Traduction en cours…');
    const r = await guarded(() => rpc('translate', { id: c.id }));
    if (!r) return;
    apply(r);
    state.showOriginal = false;
    sheet();
  }
  /** Complète une fiche ancienne (sans studio, éditeur…) depuis IGDB, une seule fois par session. */
  async function enrich(c) {
    if (state.mode !== 'igdb' || c.rev >= 2 || refreshed.has(c.id)) return;
    refreshed.add(c.id);
    enrichingId = c.id;
    if (state.open && state.open.id === c.id) sheet();
    try {
      const r = await rpc('refresh', { id: c.id });
      apply(r);
    } catch (e) { /* IGDB indisponible : la fiche reste telle quelle */ }
    enrichingId = null;
    if (state.open && state.open.id === c.id) sheet();
  }

  /** `sheet: false` : ne pas redessiner la fiche ouverte (on y est en train de saisir). */
  async function afterChange(opts = {}) {
    await loadLibrary();
    if (state.open && opts.sheet !== false) sheet();
    render(true);
  }

  function activeFilters() {
    const f = state.filters;
    return Boolean(f.genre || f.platform || f.developer || f.min);
  }

  async function loadLibrary() {
    const f = state.filters;
    const args = { sort: state.sort };
    if (state.status !== 'all') args.status = state.status;
    if (f.genre) args.genre = f.genre;
    if (f.platform) args.platform = f.platform;
    if (f.developer) args.developer = f.developer;
    if (f.min) args.min_rating = f.min;
    state.lib = await rpc('library', args);
    state.facets = state.lib.facets;
    state.total = state.lib.total;
    remember(state.lib.entries);
    renderNav();
  }

  /* ---------- Navigation ---------- */

  const NAV = [
    ['library', 'Bibliothèque', 'library'],
    ['search', 'Rechercher', 'search'],
    ['stats', 'Statistiques', 'stats'],
    ['settings', 'Réglages', 'settings'],
  ];

  function renderNav() {
    const total = state.total;
    $('#nav').replaceChildren(...NAV.map(([key, label, ic]) =>
      h('button', { type: 'button', 'aria-current': state.view === key ? 'page' : null, onclick: () => go(key) },
        icon(ic), h('span', { class: 't', text: label }),
        key === 'library' && total ? h('span', { class: 'count', text: total }) : null)));
    const conn = $('#conn');
    conn.dataset.mode = state.mode;
    conn.replaceChildren(h('span', { class: 'dot' }), h('span', { class: 't', text: state.mode === 'igdb' ? 'IGDB connecté' : 'Mode hors ligne' }));
    conn.title = 'Ouvrir les réglages';
  }

  function go(view) {
    state.view = view;
    if (view !== 'search') { $('#q').value = ''; state.query = ''; }
    renderNav();
    render();
    if (view === 'stats') loadStats();
    if (view === 'settings') loadSettings();
  }

  async function loadStats() {
    state.stats = await guarded(() => rpc('stats'));
    if (state.stats) { remember(state.stats.top_rated); remember(state.stats.hours.longest); }
    if (state.view === 'stats') render(true);
  }
  async function loadSettings() {
    state.settings = await guarded(() => rpc('get_settings'));
    if (state.view === 'settings') render(true);
  }

  /* ---------- Vues ---------- */

  function render(keepScroll) {
    const view = $('#view');
    const top = keepScroll ? view.scrollTop : 0;
    const fn = { library: viewLibrary, search: viewSearch, stats: viewStats, settings: viewSettings }[state.view];
    const nodes = fn();
    if (state.notice) nodes.unshift(noticeBanner());
    view.replaceChildren(...nodes);
    view.scrollTop = top;
  }

  /** Message important venu du moteur au lancement (ex. bibliothèque restaurée) : reste affiché jusqu'à « Compris ». */
  function noticeBanner() {
    return h('div', { class: 'alertbox', role: 'alert' },
      h('p', { text: state.notice }),
      h('button', { class: 'btn small', type: 'button', text: 'Compris', onclick: () => { state.notice = null; render(true); } }));
  }

  function emptyLibrary() {
    return h('div', { class: 'empty' },
      h('h2', { text: 'Ta bibliothèque est vide' }),
      h('p', { text: 'Cherche un jeu, ajoute-le en un clic, puis suis ce que tu joues, finis ou abandonnes.' }),
      h('button', { class: 'btn primary', type: 'button', onclick: () => $('#q').focus() }, icon('search', 18), 'Chercher un jeu'));
  }

  function filterSelect(label, all, key, options) {
    const cur = state.filters[key];
    const known = options.some((o) => o.name === cur);
    const list = known || !cur ? options : [{ name: cur, count: 0 }, ...options];
    return h('label', { class: 'flt' + (cur ? ' on' : '') },
      h('span', { class: 'sr', text: label }),
      h('select', {
        'aria-label': label,
        onchange: async (e) => { state.filters[key] = e.target.value; await loadLibrary(); render(true); },
      }, h('option', { value: '', text: all }),
        list.map((o) => h('option', { value: o.name, selected: o.name === cur, text: `${o.name} (${o.count})` }))));
  }

  function filterRow() {
    const f = state.filters;
    const fc = state.facets;
    const minSel = h('label', { class: 'flt' + (f.min ? ' on' : '') },
      h('span', { class: 'sr', text: 'Note minimale' }),
      h('select', {
        'aria-label': 'Note minimale',
        onchange: async (e) => { state.filters.min = Number(e.target.value); await loadLibrary(); render(true); },
      }, h('option', { value: '0', text: 'Toute note' }),
        [4.5, 4, 3.5, 3, 2].map((v) => h('option', { value: String(v), selected: f.min === v, text: `${fmt(v)} étoiles et plus` }))));
    return h('div', { class: 'filters', role: 'group', 'aria-label': 'Filtres' },
      filterSelect('Genre', 'Tous les genres', 'genre', fc.genres),
      filterSelect('Plateforme', 'Toutes les plateformes', 'platform', fc.platforms),
      filterSelect('Studio', 'Tous les studios', 'developer', fc.developers),
      minSel,
      activeFilters() ? h('button', {
        class: 'btn quiet small', type: 'button',
        onclick: async () => { state.filters = { genre: '', platform: '', developer: '', min: 0 }; await loadLibrary(); render(true); },
      }, 'Réinitialiser') : null);
  }

  function viewLibrary() {
    const { entries, counts } = state.lib;
    const total = state.total || 0;
    const shown = entries.length;
    const sub = !total ? '' : activeFilters() || state.status !== 'all'
      ? `${shown} sur ${total} jeu${total > 1 ? 'x' : ''}` : `${total} jeu${total > 1 ? 'x' : ''}`;
    const out = [h('div', { class: 'page-head' }, h('h1', { text: 'Bibliothèque' }), h('span', { class: 'sub', text: sub }))];
    if (!total) { out.push(emptyLibrary()); return out; }

    const tabs = [{ key: 'all', label: 'Tous' }, ...STATUSES].map((s) =>
      h('button', {
        class: 'tab', type: 'button', 'aria-pressed': state.status === s.key, vars: { '--c': s.key === 'all' ? 'var(--dim)' : `var(--st-${s.key})` },
        onclick: async () => { state.status = s.key; await loadLibrary(); render(); },
      }, s.key === 'all' ? null : h('span', { class: 'd' }), s.label, h('span', { class: 'n', text: counts[s.key] || 0 })));
    const select = h('select', { 'aria-label': 'Trier par', onchange: async (e) => { state.sort = e.target.value; await loadLibrary(); render(true); } },
      SORTS.map(([v, l]) => h('option', { value: v, text: l, selected: state.sort === v })));
    out.push(h('div', { class: 'controls' }, h('div', { class: 'tabs', role: 'group', 'aria-label': 'Filtrer par statut' }, tabs), h('label', { class: 'sort' }, 'Trier par', select)));
    out.push(filterRow());

    if (state.mode === 'offline') {
      out.push(h('div', { class: 'banner' }, h('span', { text: 'Mode hors ligne : catalogue intégré, sans jaquettes. Ajoute tes clés IGDB pour chercher parmi tous les jeux avec leurs images.' }),
        h('button', { class: 'btn small', type: 'button', onclick: () => go('settings'), text: 'Réglages' })));
    }

    const playing = state.status === 'all' && !activeFilters() ? entries.filter((e) => e.entry.status === 'playing') : [];
    if (playing.length) {
      out.push(h('h2', { class: 'section' }, 'Reprendre', h('span', { class: 'n', text: playing.length })));
      out.push(h('div', { class: 'shelf' }, playing.map(cardEl)));
    }
    const rest = playing.length ? entries.filter((e) => e.entry.status !== 'playing') : entries;
    if (playing.length && rest.length) out.push(h('h2', { class: 'section' }, 'Autres jeux', h('span', { class: 'n', text: rest.length })));
    if (rest.length) out.push(h('div', { class: 'grid' }, rest.map(cardEl)));
    else if (!playing.length) {
      out.push(h('p', { class: 'note', text: activeFilters() ? 'Aucun jeu ne correspond à ces filtres.' : 'Aucun jeu avec ce statut.' }));
    }
    return out;
  }

  function viewSearch() {
    const f = state.found;
    const out = [h('div', { class: 'page-head' }, h('h1', { text: state.query ? `Résultats pour « ${state.query} »` : 'Rechercher' }))];
    if (!state.query) {
      out.push(h('div', { class: 'empty' },
        h('h2', { text: 'Quel jeu veux-tu suivre ?' }),
        h('p', { text: 'Tape au moins deux lettres. La recherche tolère les fautes de frappe : « zleda » trouve Zelda.' })));
      return out;
    }
    if (!f) { out.push(h('p', { class: 'note', text: 'Recherche en cours…' })); return out; }
    if (f.warning) out.push(h('div', { class: 'banner' }, h('span', { text: f.warning })));
    else if (f.source === 'offline') out.push(h('div', { class: 'banner' }, h('span', { text: 'Catalogue hors ligne : sans jaquettes et limité aux jeux intégrés.' }),
      h('button', { class: 'btn small', type: 'button', onclick: () => go('settings'), text: 'Ajouter mes clés IGDB' })));
    if (f.mine.length) {
      out.push(h('h2', { class: 'section' }, 'Dans ta bibliothèque', h('span', { class: 'n', text: f.mine.length })));
      out.push(h('div', { class: 'grid' }, f.mine.map(cardEl)));
    }
    out.push(h('h2', { class: 'section' }, f.source === 'igdb' ? 'Sur IGDB' : 'Catalogue', h('span', { class: 'n', text: f.results.length })));
    out.push(f.results.length ? h('div', { class: 'grid' }, f.results.map(cardEl)) : h('p', { class: 'note', text: 'Aucun jeu trouvé. Essaie un autre titre ou une orthographe proche.' }));
    return out;
  }

  function tile(label, value, note, extra) {
    return h('div', { class: 'tile' }, h('span', { class: 'k', text: label }), h('span', { class: 'v', text: value }), note ? h('span', { class: 'n', text: note }) : null, extra || null);
  }

  /** Liste de barres horizontales, une seule teinte (la longueur porte la valeur, le texte les détails). */
  function barList(items, label) {
    const max = Math.max(1, ...items.map((i) => i.count));
    return h('ul', { class: 'blist', 'aria-label': label },
      items.map((i) => {
        const meta = [i.average ? `${fmt(i.average)} ★` : null, i.hours_text].filter(Boolean).join('  ');
        return h('li', { title: `${i.name} : ${i.count} jeu${i.count > 1 ? 'x' : ''}${meta ? ' (' + meta + ')' : ''}` },
          h('span', { class: 'nm', text: i.name }),
          h('span', { class: 'track' }, h('span', { class: 'fill', vars: { '--w': (i.count / max) * 100 + '%' } })),
          h('span', { class: 'ct', text: i.count }),
          h('span', { class: 'mt', text: meta }));
      }));
  }

  function gameRows(cards, valueOf, maxValue) {
    return h('ul', { class: 'glist' }, cards.map((c) => h('li', {},
      h('button', { type: 'button', class: 'grow', onclick: () => openGame(c.id) },
        h('span', { class: 'mini' }, coverEl(c)),
        h('span', { class: 'gn' }, h('b', { text: c.name }), h('i', { text: [c.year, c.entry && c.entry.platform].filter(Boolean).join('  ') })),
        maxValue ? h('span', { class: 'gtrack' }, h('span', { class: 'fill', vars: { '--w': (valueOf(c) / maxValue) * 100 + '%' } })) : null,
        h('span', { class: 'gv' }, valueOf.label(c))))));
  }

  function viewStats() {
    const s = state.stats;
    const out = [h('div', { class: 'page-head' }, h('h1', { text: 'Statistiques' }))];
    if (!s) return out;
    if (!s.total) {
      out.push(h('div', { class: 'empty' }, h('h2', { text: 'Rien à compter pour l’instant' }), h('p', { text: 'Ajoute quelques jeux, note-les et renseigne ton temps de jeu : tes statistiques apparaîtront ici.' })));
      return out;
    }
    const plural = (n, one, many) => `${n} ${n > 1 ? many : one}`;
    const tl = s.timeline;
    const tiles = h('div', { class: 'tiles' },
      tile('Jeux suivis', String(s.total), s.backlog_pending ? `dont ${plural(s.backlog_pending, 'à jouer ou en envie', 'à jouer ou en envie')}` : 'rien en attente'),
      tile('Temps de jeu', s.hours.games_timed ? s.hours.total_text : '–', s.hours.games_timed ? `sur ${plural(s.hours.games_timed, 'jeu renseigné', 'jeux renseignés')}` : 'à saisir dans chaque fiche'),
      tile('Terminés', String(s.counts.completed || 0), s.finish_rate != null ? `${fmt(s.finish_rate)} % des jeux commencés` : 'aucun jeu commencé'),
      tile('Note moyenne', s.rated ? fmt(s.average) : '–', s.rated ? `médiane ${fmt(s.median)}, ${plural(s.loved, 'coup de cœur', 'coups de cœur')} (4,5 et +)` : 'aucun jeu noté'),
      tile('Commentaires', String(s.commented), s.commented ? `sur ${plural(s.total, 'jeu', 'jeux')}` : 'aucun pour l’instant'),
      tile('Ancienneté du backlog', tl.backlog_average_days != null ? ageText(tl.backlog_average_days) : '–',
        tl.backlog_dated ? `en moyenne, sur ${plural(tl.backlog_dated, 'jeu daté', 'jeux datés')}` : tl.backlog_total ? 'dates d’ajout à renseigner' : 'aucun jeu en backlog'),
      tile(`Terminés en ${tl.year}`, String(tl.finished_this_year), tl.finished_this_year ? 'jeux finis cette année' : 'aucun terminé cette année'));
    out.push(tiles);

    // notes
    const max = Math.max(1, ...s.bars.map((b) => b.count));
    const bars = h('div', { class: 'bars', role: 'img', 'aria-label': 'Répartition de mes notes' },
      s.bars.map((b) => h('div', { class: 'bar', title: `${fmt(b.stars)} étoile${b.stars > 1 ? 's' : ''} : ${b.count} jeu${b.count > 1 ? 'x' : ''}` },
        h('span', { class: 'fill', vars: { '--h': (b.count / max) * 100 + '%' } }),
        h('span', { class: 'tick', text: Number.isInteger(b.stars) ? String(b.stars) : '' }))));
    const notes = h('section', { class: 'panel' },
      h('h3', { text: 'Mes notes' }),
      h('div', { class: 'avg' }, h('span', { class: 'big', text: s.rated ? fmt(s.average) : '–' }), starsWidget(s.rated ? s.average : 0, null, { readonly: true }),
        h('span', { class: 'note', text: s.rated ? `moyenne sur ${plural(s.rated, 'jeu noté', 'jeux notés')}` : 'aucun jeu noté' })),
      bars, h('div', { class: 'axis' }, h('span', { text: '½ étoile' }), h('span', { text: '5 étoiles' })));

    const present = STATUSES.filter((st) => s.counts[st.key] > 0);
    const status = h('section', { class: 'panel' },
      h('h3', { text: 'Où en sont mes jeux' }),
      h('div', { class: 'split' }, present.map((st) => h('i', { title: st.label, vars: { '--c': `var(--st-${st.key})`, flex: s.counts[st.key] } }))),
      h('ul', { class: 'legend' }, STATUSES.map((st) => h('li', { vars: { '--c': `var(--st-${st.key})` } }, h('span', { class: 'd' }), st.label, h('b', { text: s.counts[st.key] })))));

    const panels = [notes, status];
    if (s.top_rated.length) {
      panels.push(h('section', { class: 'panel' }, h('h3', { text: 'Mes mieux notés' }),
        gameRows(s.top_rated, Object.assign((c) => c.entry.stars, { label: (c) => `${fmt(c.entry.stars)} ★` }), 5)));
    }
    if (s.hours.longest.length) {
      const top = s.hours.longest[0].entry.minutes;
      panels.push(h('section', { class: 'panel' }, h('h3', { text: 'Mes plus longues parties' }),
        gameRows(s.hours.longest, Object.assign((c) => c.entry.minutes, { label: (c) => c.entry.hours_text }), top),
        s.hours.average_finish_text ? h('p', { class: 'note', text: `Durée moyenne d’un jeu terminé : ${s.hours.average_finish_text}.` }) : null));
    } else {
      panels.push(h('section', { class: 'panel' }, h('h3', { text: 'Temps de jeu' }),
        h('p', { class: 'note', text: 'Aucune durée renseignée. Ouvre une fiche et saisis le temps passé sur le jeu (par exemple 12h30) : le total et le classement des plus longues parties apparaîtront ici.' })));
    }
    if (tl.oldest.length) {
      const oldest = daysSince(tl.oldest[0].entry.added);
      panels.push(h('section', { class: 'panel' }, h('h3', { text: 'Les plus anciens du backlog' }),
        gameRows(tl.oldest, Object.assign((c) => daysSince(c.entry.added), { label: (c) => ageText(daysSince(c.entry.added)) }), Math.max(1, oldest)),
        tl.backlog_dated < tl.backlog_total ? h('p', { class: 'note', text: `${plural(tl.backlog_total - tl.backlog_dated, 'jeu du backlog n’a', 'jeux du backlog n’ont')} pas de date d’ajout et n’${tl.backlog_total - tl.backlog_dated > 1 ? 'entrent' : 'entre'} pas dans ce classement.` }) : null));
    }
    if (tl.finished_by_year.length) {
      panels.push(h('section', { class: 'panel' }, h('h3', { text: 'Terminés par année' }),
        barList(tl.finished_by_year, 'Jeux terminés par année'),
        tl.average_days_to_finish != null ? h('p', { class: 'note', text: `Entre l’ajout et la fin d’un jeu : ${ageText(tl.average_days_to_finish)} en moyenne (sur ${plural(tl.to_finish_sample, 'jeu', 'jeux')}).` }) : null));
    } else if (!tl.oldest.length) {
      panels.push(h('section', { class: 'panel' }, h('h3', { text: 'Ancienneté et rythme' }),
        h('p', { class: 'note', text: 'Les dates d’ajout et de fin se remplissent toutes seules pour les nouveaux jeux. Pour les jeux déjà présents, ouvre une fiche et renseigne les dates : tu verras ici l’ancienneté de ton backlog et les jeux terminés par année.' })));
    }
    const listPanel = (title, items, empty) => h('section', { class: 'panel' }, h('h3', { text: title }), items.length ? barList(items, title) : h('p', { class: 'note', text: empty }));
    const needIgdb = state.mode === 'igdb' ? 'Les fiches se complètent en arrière-plan : reviens dans un instant.' : 'Disponible avec IGDB (voir Réglages) : genres, studios et éditeurs viennent de leurs fiches.';
    panels.push(listPanel('Genres', s.genres, needIgdb));
    panels.push(listPanel('Plateformes', s.platforms, 'Choisis la plateforme dans chaque fiche pour alimenter cette liste.'));
    panels.push(listPanel('Studios', s.developers, needIgdb));
    panels.push(listPanel('Éditeurs', s.publishers, needIgdb));
    if (s.decades.length) {
      const dmax = Math.max(1, ...s.decades.map((d) => d.count));
      panels.push(h('section', { class: 'panel wide' }, h('h3', { text: 'Époque des jeux' }),
        h('div', { class: 'decades', role: 'img', 'aria-label': 'Nombre de jeux par décennie de sortie' },
          s.decades.map((d) => h('div', { class: 'dec', title: `${d.name} : ${d.count} jeu${d.count > 1 ? 'x' : ''}` },
            h('span', { class: 'ct', text: d.count }), h('span', { class: 'col' }, h('span', { class: 'fill', vars: { '--h': (d.count / dmax) * 100 + '%' } })), h('span', { class: 'nm', text: d.name }))))));
    }
    out.push(h('div', { class: 'stats' }, panels));
    return out;
  }

  /** Adresse à recopier : les liens externes ne s'ouvrent pas dans la fenêtre de l'application. */
  function linkBox(url) {
    const copy = async () => {
      try { await navigator.clipboard.writeText(url); toast('Adresse copiée : colle-la dans ton navigateur'); }
      catch (e) { toast('Sélectionne l’adresse et copie-la avec Ctrl C'); }
    };
    return h('span', { class: 'linkbox' }, h('span', { class: 'path', text: url }), h('button', { class: 'btn quiet small', type: 'button', onclick: copy, text: 'Copier' }));
  }

  function guideStep(title, ...body) {
    return h('li', {}, h('div', { class: 'st' }, h('b', { text: title }), ...body));
  }

  function guideIgdb(open) {
    return h('details', { class: 'guide', open },
      h('summary', {}, 'Comment obtenir mes clés IGDB ? (environ 5 minutes, gratuit)'),
      h('p', { class: 'lead', text: 'IGDB est la grande base de données publique des jeux vidéo : elle fournit les jaquettes, les résumés, les studios et les éditeurs. Elle appartient à Twitch, donc les clés se créent depuis un compte Twitch.' }),
      h('ol', { class: 'steps' },
        guideStep('Crée un compte Twitch, ou connecte-toi au tien', h('p', { text: 'Un compte gratuit suffit. Tu n’as jamais besoin de diffuser quoi que ce soit.' }), linkBox('https://www.twitch.tv')),
        guideStep('Active la double authentification', h('p', { text: 'Twitch l’exige avant de créer une application. Dans Twitch : ton profil, Paramètres, onglet « Sécurité et confidentialité », puis active l’authentification à deux facteurs avec ton téléphone.' })),
        guideStep('Ouvre la console développeur de Twitch', h('p', { text: 'Va à l’adresse ci-dessous, puis choisis « Applications » et « Enregistrer votre application ».' }), linkBox('https://dev.twitch.tv/console')),
        guideStep('Remplis le formulaire', h('ul', {},
          h('li', { text: 'Nom : un nom unique, par exemple « Backlog Panda de » suivi de ton pseudo.' }),
          h('li', { text: 'URL de redirection OAuth : http://localhost' }),
          h('li', { text: 'Catégorie : « Application Integration » (ou la plus proche).' }),
          h('li', { text: 'Type de client : « Confidentiel ».' })),
          h('p', { class: 'fine', text: 'Les intitulés exacts peuvent varier selon la langue de Twitch. Valide ensuite avec « Créer ».' })),
        guideStep('Récupère l’identifiant et le secret', h('p', { text: 'Dans la liste de tes applications, clique sur « Gérer » pour la tienne. Copie l’« Identifiant client » (Client ID). Clique sur « Nouveau secret » : copie le Client Secret tout de suite, Twitch ne le réaffiche plus. Tu as perdu le secret ? Génère-en simplement un nouveau.' })),
        guideStep('Colle les deux valeurs ici, enregistre, puis teste', h('p', { text: 'Utilise les deux champs ci-dessous, puis « Tester la connexion ». Quand le test réussit, le bandeau « Mode hors ligne » disparaît et la recherche couvre tous les jeux.' }))),
      h('p', { class: 'fine', text: 'Ces clés restent sur cet ordinateur et ne servent qu’à interroger IGDB. Ne les partage avec personne : quiconque les possède peut utiliser ton application Twitch.' }));
  }

  function guideDeepl(open) {
    return h('details', { class: 'guide', open },
      h('summary', {}, 'Comment obtenir une clé DeepL ? (facultatif, gratuit)'),
      h('p', { class: 'lead', text: 'DeepL est un service de traduction. IGDB ne fournit les résumés qu’en anglais : avec une clé, un bouton les traduit en français dans chaque fiche. Sans clé, tout le reste fonctionne.' }),
      h('ol', { class: 'steps' },
        guideStep('Crée un compte DeepL API Free', h('p', { text: 'Choisis l’offre gratuite de l’API (et non l’application de traduction). DeepL peut demander une carte bancaire pour vérifier le compte.' }), linkBox('https://www.deepl.com/pro-api')),
        guideStep('Trouve ta clé d’authentification', h('p', { text: 'Une fois connecté, ouvre la page « Compte », section « Clé d’authentification pour DeepL API », et copie la clé. Avec l’offre gratuite, elle se termine par « :fx ».' })),
        guideStep('Colle-la dans le champ ci-dessous et enregistre', h('p', { text: 'Ouvre ensuite une fiche de jeu : le bouton « Traduire en français » apparaît sous le résumé. La traduction est gardée, elle ne consomme ton quota qu’une fois par jeu.' }))),
      h('p', { class: 'fine', text: 'L’offre gratuite impose un quota mensuel de caractères traduits, largement suffisant pour des résumés de jeux.' }));
  }

  function viewSettings() {
    const s = state.settings;
    const out = [h('div', { class: 'page-head' }, h('h1', { text: 'Réglages' }))];
    if (!s) return out;
    const msg = h('p', { class: 'status-msg', role: 'status' });
    const say = (text, ok) => { msg.textContent = text; msg.className = 'status-msg ' + (ok ? 'ok' : 'ko'); };
    const id = h('input', { id: 'cid', type: 'text', autocomplete: 'off', spellcheck: 'false', value: s.igdb_client_id || '', disabled: s.from_env });
    const secret = h('input', { id: 'csec', type: 'password', autocomplete: 'off', placeholder: s.has_secret ? 'Enregistré (laisse vide pour le garder)' : '', disabled: s.from_env });
    const save = async (e) => {
      e.preventDefault();
      const r = await guarded(() => rpc('save_settings', { igdb_client_id: id.value, igdb_client_secret: secret.value }));
      if (!r) return;
      state.settings = r;
      state.mode = r.configured ? 'igdb' : 'offline';
      renderNav();
      say(r.configured ? 'Clés enregistrées.' : 'Clés incomplètes : le mode hors ligne reste actif.', r.configured);
      secret.value = '';
    };
    const test = async () => {
      say('Test en cours…', true);
      try { await rpc('test_igdb'); say('Connexion à IGDB réussie.', true); } catch (e) { say(e.message, false); }
    };
    const deepl = h('input', { id: 'dkey', type: 'password', autocomplete: 'off', spellcheck: 'false', placeholder: s.has_deepl ? 'Enregistrée (laisse vide pour la garder)' : 'Clé d’API DeepL' });
    const saveDeepl = async (e) => {
      e.preventDefault();
      if (!deepl.value.trim()) { say2('Colle ta clé DeepL pour l’enregistrer.', false); return; }
      const r = await guarded(() => rpc('save_settings', { deepl_key: deepl.value }));
      if (!r) return;
      state.settings = r;
      deepl.value = '';
      render(true);
    };
    const clearDeepl = async () => {
      const r = await guarded(() => rpc('save_settings', { clear_deepl: true }));
      if (r) { state.settings = r; render(true); toast('Clé DeepL effacée'); }
    };
    const msg2 = h('p', { class: 'status-msg', role: 'status' });
    const say2 = (text, ok) => { msg2.textContent = text; msg2.className = 'status-msg ' + (ok ? 'ok' : 'ko'); };
    const clear = async () => {
      const r = await guarded(() => rpc('save_settings', { clear: true }));
      if (r) { state.settings = r; state.mode = 'offline'; renderNav(); render(true); toast('Clés effacées'); }
    };
    const fill = async () => {
      say('Mise à jour des fiches…', true);
      try {
        const r = await rpc('sync_metadata');
        say(r.mode === 'offline' ? 'Configure IGDB d’abord.' : `${r.updated} fiche${r.updated > 1 ? 's' : ''} mise${r.updated > 1 ? 's' : ''} à jour.`, r.mode !== 'offline');
        await loadLibrary();
      } catch (e) { say(e.message, false); }
    };
    const anyKey = s.has_secret || s.has_deepl;
    const pill = s.secret_storage === 'os' ? (anyKey ? ['ok', 'Clés chiffrées'] : ['ok', 'Coffre prêt']) : anyKey ? ['todo', 'Clés en clair'] : ['opt', 'Aucune clé enregistrée'];
    out.push(h('div', { class: 'settings' },
      h('section', { class: 'panel' },
        h('h3', {}, 'Connexion à IGDB', h('span', { class: 'pillstate ' + (s.configured ? 'ok' : 'todo'), text: s.configured ? 'Configurée' : 'Pas encore configurée' })),
        h('p', { class: 'note', text: 'Sans ces clés, l’application fonctionne avec un petit catalogue hors ligne, sans jaquettes.' }),
        guideIgdb(!s.configured),
        s.from_env ? h('p', { class: 'note', text: 'Les clés viennent des variables d’environnement IGDB_CLIENT_ID et IGDB_CLIENT_SECRET.' }) : null,
        h('form', { class: 'form', onsubmit: save },
          h('label', { for: 'cid' }, 'Client ID', id),
          h('label', { for: 'csec' }, 'Client Secret', secret),
          h('div', { class: 'row' },
            h('button', { class: 'btn primary', type: 'submit', disabled: s.from_env, text: 'Enregistrer' }),
            h('button', { class: 'btn', type: 'button', onclick: test, disabled: !s.configured, text: 'Tester la connexion' }),
            h('button', { class: 'btn quiet', type: 'button', onclick: fill, disabled: !s.configured, text: 'Mettre à jour les fiches' }),
            s.configured && !s.from_env ? h('button', { class: 'btn danger small', type: 'button', onclick: clear, text: 'Effacer les clés' }) : null),
          msg)),
      h('section', { class: 'panel' },
        h('h3', {}, 'Traduction des résumés (DeepL)', h('span', { class: 'pillstate ' + (s.has_deepl ? 'ok' : 'opt'), text: s.has_deepl ? 'Activée' : 'Facultatif' })),
        guideDeepl(false),
        h('p', { class: 'note', text: s.has_deepl ? 'Clé DeepL enregistrée : le bouton « Traduire en français » est actif dans chaque fiche.' : 'Facultatif. IGDB fournit les résumés en anglais ; avec une clé DeepL (l’offre gratuite suffit), un bouton les traduit en français et garde la traduction.' }),
        h('form', { class: 'form', onsubmit: saveDeepl },
          h('label', { for: 'dkey' }, 'Clé d’API DeepL', deepl),
          h('div', { class: 'row' },
            h('button', { class: 'btn primary', type: 'submit', text: 'Enregistrer' }),
            s.has_deepl ? h('button', { class: 'btn danger small', type: 'button', onclick: clearDeepl, text: 'Effacer la clé' }) : null),
          msg2),
        h('p', { class: 'note', text: 'Les clés de l’offre gratuite se terminent par « :fx ». Les noms de genres, thèmes et modes de jeu sont déjà traduits sans clé.' })),
      backupPanel(),
      h('section', { class: 'panel help' },
        h('h3', {}, 'Tes données', h('span', { class: 'pillstate ' + pill[0], text: pill[1] })),
        h('p', {}, 'Ta bibliothèque est enregistrée sur cet ordinateur, dans ', h('span', { class: 'path', text: s.data_dir }), '.'),
        s.secret_storage === 'os'
          ? h('p', {}, `Tes clés (secret IGDB, clé DeepL) sont chiffrées par ${s.secret_storage_name} et liées à ta session : le fichier de réglages ne contient que ton identifiant client, qui n’est pas confidentiel. Elles ne suivent pas une copie du dossier sur un autre ordinateur : il faudra les ressaisir.`)
          : s.secret_storage_name
            ? h('p', {}, `${s.secret_storage_name[0].toUpperCase()}${s.secret_storage_name.slice(1)} n’a pas répondu : tes clés restent pour l’instant dans le fichier de réglages, en clair. Elles seront déplacées dans le coffre au prochain lancement.`)
            : h('p', {}, 'Ce système n’a pas de coffre pris en charge : tes clés sont enregistrées en clair dans le fichier de réglages de ce dossier. Ne partage pas ce dossier.'))));
    return out;
  }


  /* ---------- Sauvegarde, export, import ---------- */

  const BACKUP_KINDS = { auto: 'Automatique', manuelle: 'Manuelle', 'avant-import': 'Avant un import', 'avant-restauration': 'Avant une restauration' };
  const plural = (n, one, many) => `${n} ${n > 1 ? many : one}`;
  const when = (ts) => new Date(ts * 1000).toLocaleString('fr-FR', { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' });
  const size = (b) => (b < 1048576 ? `${Math.max(1, Math.round(b / 1024))} Ko` : `${(b / 1048576).toFixed(1).replace('.', ',')} Mo`);
  const MAX_IMPORT = 30 * 1024 * 1024;

  /** « +3 ajoutés, 1 mis à jour » — uniquement ce qui change. */
  function effectText(fx) {
    const parts = [];
    if (fx.added) parts.push(plural(fx.added, 'jeu ajouté', 'jeux ajoutés'));
    if (fx.updated) parts.push(plural(fx.updated, 'jeu modifié', 'jeux modifiés'));
    if (fx.removed) parts.push(plural(fx.removed, 'jeu retiré', 'jeux retirés'));
    return parts.length ? parts.join(', ') : 'aucun changement';
  }
  const samples = (fx) => {
    const one = (label, names, total) => names.length ? `${label} : ${names.join(', ')}${total > names.length ? '…' : ''}` : null;
    return [one('Ajoutés', fx.added_names, fx.added), one('Modifiés', fx.updated_names, fx.updated), one('Retirés', fx.removed_names, fx.removed)].filter(Boolean).join(' — ');
  };

  function backupPanel() {
    const list = h('div', { class: 'bk-list' });
    const pill = h('span', { class: 'pillstate opt', text: '…' });
    const exportMsg = h('div', { class: 'bk-result', role: 'status' });
    const importBox = h('div', { class: 'bk-import', hidden: true });
    const picker = h('input', { type: 'file', accept: '.json,application/json', class: 'sr', tabindex: '-1', 'aria-hidden': 'true' });
    const where = h('span', { text: 'ton dossier Téléchargements' });

    const refresh = async () => {
      const r = await guarded(() => rpc('backups'));
      if (!r) return;
      where.replaceChildren(h('span', { class: 'path', text: r.export_folder }));
      paintList(r);
    };

    const paintList = (r) => {
      const last = r.items[0];
      pill.className = 'pillstate ' + (last ? 'ok' : 'todo');
      pill.textContent = last ? `Dernière : ${when(last.ts)}` : 'Aucune sauvegarde';
      if (!r.items.length) {
        list.replaceChildren(h('p', { class: 'note', text: 'Pas encore de sauvegarde : la première est faite automatiquement au premier changement de la journée.' }));
        return;
      }
      list.replaceChildren(h('ul', { class: 'bk' }, r.items.map((b) => backupRow(b))));
    };

    const backupRow = (b) => {
      const li = h('li', {});
      const idle = () => li.replaceChildren(
        h('div', { class: 'bk-main' },
          h('b', { text: when(b.ts) }),
          h('span', { class: 'bk-meta', text: `${BACKUP_KINDS[b.kind] || b.kind} · ${b.games == null ? 'illisible' : plural(b.games, 'jeu', 'jeux')} · ${size(b.bytes)}` })),
        h('button', { class: 'btn quiet small', type: 'button', disabled: b.games == null, text: 'Restaurer', onclick: confirming }));
      const confirming = () => li.replaceChildren(
        h('div', { class: 'bk-main' }, h('span', { class: 'bk-ask', text: `Remplacer ta bibliothèque par cette sauvegarde (${plural(b.games, 'jeu', 'jeux')}) ? L’état actuel est sauvegardé juste avant.` })),
        h('button', { class: 'btn danger small', type: 'button', text: 'Oui, restaurer', onclick: restore }),
        h('button', { class: 'btn quiet small', type: 'button', text: 'Annuler', onclick: idle }));
      const restore = async () => {
        const r = await guarded(() => rpc('restore_backup', { file: b.file }));
        if (!r) return;
        await loadLibrary();
        paintList(r.backups);
        toast(`Bibliothèque restaurée (${plural(r.games, 'jeu', 'jeux')})`);
      };
      idle();
      return li;
    };

    const doExport = async (format) => {
      exportMsg.replaceChildren(h('span', { class: 'status-msg ok', text: 'Export en cours…' }));
      try {
        const r = await rpc('export_file', { format });
        exportMsg.replaceChildren(
          h('p', { class: 'status-msg ok', text: `${plural(r.games, 'jeu exporté', 'jeux exportés')} dans :` }),
          h('p', {}, h('span', { class: 'path', text: r.path })),
          h('button', { class: 'btn small', type: 'button', text: 'Afficher dans le dossier', onclick: () => guarded(() => rpc('reveal', { target: 'exports', file: r.file })) }));
      } catch (e) {
        exportMsg.replaceChildren(h('p', { class: 'status-msg ko', text: e.message }));
      }
    };

    const backupNow = async () => {
      const r = await guarded(() => rpc('backup_now'));
      if (!r) return;
      paintList(r);
      toast('Sauvegarde créée');
    };

    const closeImport = () => { importBox.hidden = true; importBox.replaceChildren(); picker.value = ''; };

    const showPreview = (name, content, plan) => {
      let mode = 'add';
      const apply = h('button', { class: 'btn primary', type: 'button', text: 'Importer' });
      const modes = [
        ['add', 'Ajouter les jeux qui me manquent', 'Rien de ce que tu as déjà n’est modifié. Le plus sûr.', plan.add],
        ['merge', 'Fusionner', 'Pour chaque jeu présent des deux côtés, la modification la plus récente gagne. Pense à une copie de ta propre bibliothèque ; entre deux ordinateurs, la comparaison n’est pas fiable.', plan.merge],
        ['replace', 'Tout remplacer', 'Ta bibliothèque devient celle du fichier.', plan.replace],
      ];
      const radios = modes.map(([key, title, hint, fx]) => {
        const input = h('input', { type: 'radio', name: 'import-mode', value: key, checked: key === mode, onchange: () => { mode = key; } });
        const extra = key === 'replace' && fx.removed ? ` Les ${plural(fx.removed, 'jeu actuel', 'jeux actuels')} absents du fichier disparaissent.` : '';
        return h('label', { class: 'bk-mode' }, input,
          h('span', {},
            h('b', { text: title }), h('em', { text: effectText(fx) }),
            h('span', { class: 'bk-hint', text: hint + extra }),
            samples(fx) ? h('span', { class: 'bk-hint', text: samples(fx) }) : null));
      });
      apply.addEventListener('click', async () => {
        apply.disabled = true;
        const r = await guarded(() => rpc('import_apply', { content, mode }));
        if (!r) { apply.disabled = false; return; }
        await loadLibrary();
        closeImport();
        toast(`Import terminé : ${effectText(r.effect)}`);
        refresh();
      });
      importBox.hidden = false;
      importBox.replaceChildren(...[
        h('p', {}, h('b', { text: name }), ` contient ${plural(plan.file_games, 'jeu', 'jeux')} ; ta bibliothèque en compte ${plan.local_games}.`),
        plan.skipped ? h('p', { class: 'fine', text: `${plural(plan.skipped, 'entrée ignorée', 'entrées ignorées')} : la fiche du jeu manque dans le fichier.` }) : null,
        h('fieldset', { class: 'bk-modes' }, h('legend', { class: 'sr', text: 'Que faire de ce fichier ?' }), radios),
        h('p', { class: 'fine', text: 'Une sauvegarde de ta bibliothèque actuelle est faite juste avant l’import.' }),
        h('div', { class: 'row' }, apply, h('button', { class: 'btn quiet', type: 'button', text: 'Annuler', onclick: closeImport })),
      ].filter(Boolean));
    };

    picker.addEventListener('change', async () => {
      const f = picker.files && picker.files[0];
      if (!f) return;
      if (f.size > MAX_IMPORT) { toast('Ce fichier est trop gros pour être une bibliothèque Backlog Panda.'); picker.value = ''; return; }
      try {
        const content = await f.text();
        const plan = await rpc('import_preview', { content });
        showPreview(f.name, content, plan);
      } catch (e) {
        importBox.hidden = false;
        importBox.replaceChildren(h('p', { class: 'status-msg ko', role: 'alert', text: e.message }));
        picker.value = '';
      }
    });

    refresh();
    return h('section', { class: 'panel', id: 'sauvegarde' },
      h('h3', {}, 'Sauvegarde et export', pill),
      h('p', { class: 'note', text: 'Ta bibliothèque est copiée automatiquement une fois par jour, au premier changement : les 15 dernières copies sont gardées. Tu peux aussi l’exporter ou la réimporter, par exemple pour changer d’ordinateur.' }),
      h('div', { class: 'bk-block' },
        h('h4', { text: 'Exporter' }),
        h('p', { class: 'fine' }, 'Le fichier JSON contient tout (statuts, notes, heures, commentaires, fiches) et peut être réimporté. Le CSV s’ouvre dans Excel ou LibreOffice et se réimporte avec « Importer une liste (CSV) » plus bas. Les fichiers sont déposés dans ', where, '.'),
        h('div', { class: 'row' },
          h('button', { class: 'btn', type: 'button', text: 'Exporter en JSON', onclick: () => doExport('json') }),
          h('button', { class: 'btn', type: 'button', text: 'Exporter pour Excel (CSV)', onclick: () => doExport('csv') })),
        exportMsg),
      h('div', { class: 'bk-block' },
        h('h4', { text: 'Importer' }),
        h('p', { class: 'fine', text: 'Choisis un fichier JSON créé par « Exporter » ou une sauvegarde. Tu verras ce qui va changer avant de confirmer.' }),
        h('div', { class: 'row' }, h('button', { class: 'btn', type: 'button', text: 'Choisir un fichier…', onclick: () => picker.click() }), picker),
        importBox),
      h('div', { class: 'bk-block', id: 'import-csv' },
        h('h4', { text: 'Importer une liste (CSV)' }),
        h('p', { class: 'fine', text: 'Pour reprendre une liste faite dans un tableur ou exportée d’un autre site : il suffit d’une colonne de titres. Les colonnes statut, note, plateforme, heures, commentaire, date d’ajout et de fin sont reconnues si elles existent. Pour les notes, précise l’échelle dans l’en-tête de la colonne (« Note /10 », « Score /100 ») : sinon elle est devinée valeur par valeur, et un 4 sera lu comme 4 sur 5.' }),
        csvBlock(refresh)),
      h('div', { class: 'bk-block' },
        h('h4', { text: 'Sauvegardes' }),
        h('div', { class: 'row' },
          h('button', { class: 'btn', type: 'button', text: 'Sauvegarder maintenant', onclick: backupNow }),
          h('button', { class: 'btn quiet', type: 'button', text: 'Ouvrir le dossier', onclick: () => guarded(() => rpc('reveal', { target: 'backups' })) })),
        list));
  }

  /* ---------- Import d'une liste (CSV) ---------- */

  const MAX_CSV = 8 * 1024 * 1024;
  const CSV_KINDS = {
    sure: 'Trouvé',
    check: 'À vérifier',
    notfound: 'Introuvable',
    present: 'Déjà dans ta bibliothèque',
  };
  const CSV_PAGE = 40;

  function csvBlock(onDone) {
    const box = h('div', { class: 'csv' });
    const picker = h('input', { type: 'file', accept: '.csv,.txt,text/csv,text/plain', class: 'sr', tabindex: '-1', 'aria-hidden': 'true' });
    let token = 0; // invalide une analyse en cours quand on annule ou qu'on recommence

    const reset = () => { token++; picker.value = ''; };
    const cancel = async () => {
      reset();
      await guarded(() => rpc('csv_cancel'));
      idle();
    };
    const fail = (e) => {
      reset();
      box.replaceChildren(h('p', { class: 'status-msg ko', role: 'alert', text: e.message }), h('div', { class: 'row' }, h('button', { class: 'btn', type: 'button', text: 'Choisir un autre fichier', onclick: () => { idle(); picker.click(); } })));
    };

    const idle = () => {
      box.replaceChildren(
        h('div', { class: 'row' }, h('button', { class: 'btn', type: 'button', text: 'Choisir un fichier CSV…', onclick: () => picker.click() }), picker));
    };

    const showStart = (name, s) => {
      const known = s.columns.filter((c) => c.role);
      const ignored = s.columns.filter((c) => !c.role);
      const offline = s.mode === 'offline';
      const run = h('button', { class: 'btn primary', type: 'button', text: 'Rechercher les jeux', onclick: () => matchAll(s) });
      box.replaceChildren(...[
        h('p', {}, h('b', { text: name }), ` : ${plural(s.rows, 'jeu à rapprocher', 'jeux à rapprocher')}${s.blank ? ` (${plural(s.blank, 'ligne vide ignorée', 'lignes vides ignorées')})` : ''}.`),
        h('ul', { class: 'csv-cols' },
          known.map((c) => h('li', { class: 'on' }, h('b', { text: c.role }), h('span', { text: `colonne « ${c.header} »` }))),
          ignored.map((c) => h('li', { class: 'off' }, h('b', { text: 'Ignorée' }), h('span', { text: `colonne « ${c.header || '(sans nom)'} »` })))),
        s.with_issues ? h('p', { class: 'fine', text: `${plural(s.with_issues, 'ligne contient', 'lignes contiennent')} une valeur illisible (note, date…) : elle sera importée sans cette valeur, tu le verras à l’étape suivante.` }) : null,
        offline ? h('p', { class: 'fine warn', text: 'Aucune clé IGDB n’est configurée : seuls les 162 jeux du catalogue intégré peuvent être trouvés. Ajoute ta clé IGDB plus haut pour une vraie recherche.' }) : null,
        h('p', { class: 'fine', text: offline ? 'Rien n’est ajouté avant ta confirmation.' : 'Chaque titre est cherché sur IGDB (environ trois par seconde). Rien n’est ajouté avant ta confirmation.' }),
        h('div', { class: 'row' }, run, h('button', { class: 'btn quiet', type: 'button', text: 'Annuler', onclick: cancel })),
      ].filter(Boolean));
    };

    const matchAll = async (s) => {
      const mine = ++token;
      const bar = h('div', { class: 'csv-bar', role: 'progressbar', 'aria-valuemin': '0', 'aria-valuemax': String(s.rows), 'aria-valuenow': '0', 'aria-label': 'Recherche des jeux' }, h('i', {}));
      const label = h('p', { class: 'fine', role: 'status', text: 'Recherche en cours…' });
      const note = h('p', { class: 'fine' });
      const stop = h('button', { class: 'btn quiet', type: 'button', text: 'Annuler', onclick: cancel });
      box.replaceChildren(bar, label, note, h('div', { class: 'row' }, stop));
      let done = 0;
      let offset = 0;
      let failures = 0;
      while (offset < s.rows) {
        if (mine !== token) return;
        try {
          const r = await rpc('csv_match', { offset, limit: 5 });
          done = r.done;
          offset += 5;
          failures = 0;
          note.textContent = '';
          bar.firstChild.style.width = `${Math.round((Math.min(offset, s.rows) / s.rows) * 100)}%`;
          bar.setAttribute('aria-valuenow', String(Math.min(offset, s.rows)));
          label.textContent = `${Math.min(offset, s.rows)} sur ${s.rows}…`;
        } catch (e) {
          if (mine !== token) return;
          failures++;
          if (failures >= 3) { return retry(s, offset, e); }
          note.textContent = 'Connexion interrompue, nouvel essai…';
          await new Promise((res) => setTimeout(res, 1200 * failures));
        }
      }
      if (mine !== token) return;
      void done;
      review();
    };

    const retry = (s, offset, e) => {
      box.replaceChildren(
        h('p', { class: 'status-msg ko', role: 'alert', text: e.message }),
        h('p', { class: 'fine', text: `Les ${offset} premières lignes sont déjà traitées : reprendre ne refait pas ce travail.` }),
        h('div', { class: 'row' },
          h('button', { class: 'btn primary', type: 'button', text: 'Reprendre', onclick: () => matchAll(s) }),
          h('button', { class: 'btn quiet', type: 'button', text: 'Annuler', onclick: cancel })));
    };

    const review = async () => {
      let data;
      try { data = await rpc('csv_review'); } catch (e) { return fail(e); }
      const items = data.items;
      const pick = new Map(); // index -> { on, id }
      for (const it of items) {
        if (it.kind === 'sure') pick.set(it.index, { on: true, id: it.candidates[0].id });
        else if (it.kind === 'check') pick.set(it.index, { on: false, id: it.candidates[0].id });
      }
      let filter = 'all';
      let shown = CSV_PAGE;
      const list = h('ul', { class: 'csv-list' });
      const tabs = h('div', { class: 'tabs csv-tabs', role: 'tablist' });
      const apply = h('button', { class: 'btn primary', type: 'button' });
      const checkAll = h('button', { class: 'btn quiet small', type: 'button', text: 'Cocher le premier choix des « à vérifier »' });

      const count = () => [...pick.values()].filter((v) => v.on).length;
      const paintApply = () => {
        const n = count();
        apply.textContent = n ? `Ajouter ${plural(n, 'jeu', 'jeux')}` : 'Aucun jeu coché';
        apply.disabled = !n;
      };

      const row = (it) => {
        const kind = it.kind;
        const st = pick.get(it.index);
        const cand = st && it.candidates.find((c) => c.id === st.id);
        const meta = [STATUS[it.status] ? STATUS[it.status].label : null, it.stars != null ? `${String(it.stars).replace('.', ',')} ★` : null, it.platform, it.hours_text, it.year ? `année ${it.year}` : null].filter(Boolean).join(' · ');
        const cover = h('div', { class: 'csv-cover' });
        const paintCover = () => {
          const c = it.candidates.find((x) => x.id === (pick.get(it.index) || {}).id);
          cover.replaceChildren(c ? coverEl(c) : h('div', { class: 'cover' }, fallbackEl(it.title)));
        };
        paintCover();
        let control;
        if (st) {
          const cb = h('input', { type: 'checkbox', checked: st.on, 'aria-label': `Ajouter « ${it.title} »`, onchange: (e) => { st.on = e.target.checked; paintApply(); } });
          control = cb;
        } else {
          control = h('span', { class: 'csv-nocheck', 'aria-hidden': 'true' });
        }
        let pickEl;
        if (st && it.candidates.length > 1) {
          pickEl = h('select', { 'aria-label': `Quel jeu pour « ${it.title} » ?`, onchange: (e) => { st.id = Number(e.target.value); paintCover(); } },
            it.candidates.map((c) => h('option', { value: c.id, selected: c.id === st.id, text: `${c.name}${c.year ? ` (${c.year})` : ''}${c.platforms.length ? ` — ${c.platforms.slice(0, 2).join(', ')}` : ''}` })));
        } else if (cand) {
          pickEl = h('span', { class: 'csv-found', text: `${cand.name}${cand.year ? ` (${cand.year})` : ''}` });
        } else if (kind === 'notfound') {
          pickEl = h('span', { class: 'csv-found none', text: 'Aucun jeu trouvé pour ce titre' });
        } else if (kind === 'present' && it.candidates[0]) {
          pickEl = h('span', { class: 'csv-found', text: `${it.candidates[0].name}${it.candidates[0].year ? ` (${it.candidates[0].year})` : ''} — inchangé` });
        }
        return h('li', { class: `csv-row k-${kind}`, data: { kind } },
          control, cover,
          h('div', { class: 'csv-main' },
            h('div', { class: 'csv-title' }, h('b', { text: it.title }), h('span', { class: `csv-state s-${kind}`, text: CSV_KINDS[kind] })),
            meta ? h('span', { class: 'bk-meta', text: meta }) : null,
            pickEl,
            ...it.issues.map((m) => h('span', { class: 'csv-issue', text: `Ligne ${it.line} : ${m}` }))));
      };

      const paintList = () => {
        const rows = items.filter((it) => filter === 'all' || it.kind === filter);
        list.replaceChildren(...rows.slice(0, shown).map(row));
        if (rows.length > shown) {
          list.append(h('li', { class: 'csv-more' }, h('button', { class: 'btn quiet small', type: 'button', text: `Afficher ${Math.min(CSV_PAGE, rows.length - shown)} de plus (${rows.length - shown} restants)`, onclick: () => { shown += CSV_PAGE; paintList(); } })));
        }
        if (!rows.length) list.append(h('li', { class: 'note', text: 'Rien dans cette catégorie.' }));
      };

      const paintTabs = () => {
        const c = data.counts;
        const defs = [['all', 'Tous', data.total], ['sure', 'Trouvés', c.sure], ['check', 'À vérifier', c.check], ['notfound', 'Introuvables', c.notfound], ['present', 'Déjà présents', c.present]].filter(([k, , n]) => k === 'all' || n > 0);
        tabs.replaceChildren(...defs.map(([k, label, n]) => h('button', {
          type: 'button', role: 'tab', 'aria-selected': String(k === filter), class: k === filter ? 'on' : '',
          onclick: () => { filter = k; shown = CSV_PAGE; paintTabs(); paintList(); },
        }, label, h('span', { class: 'n', text: n }))));
        checkAll.hidden = !c.check;
      };

      checkAll.addEventListener('click', () => {
        for (const it of items) if (it.kind === 'check') pick.get(it.index).on = true;
        paintList(); paintApply();
      });

      apply.addEventListener('click', async () => {
        apply.disabled = true;
        const choices = [...pick.entries()].filter(([, v]) => v.on).map(([index, v]) => ({ index, id: v.id }));
        let r;
        try { r = await rpc('csv_apply', { choices }); } catch (e) { toast(e.message); paintApply(); return; }
        reset();
        await loadLibrary();
        onDone();
        const extra = [r.present ? `${plural(r.present, 'déjà présent', 'déjà présents')}` : null, r.duplicates ? `${plural(r.duplicates, 'doublon écarté', 'doublons écartés')}` : null].filter(Boolean).join(', ');
        box.replaceChildren(h('p', { class: 'status-msg ok', role: 'status', text: `${plural(r.added, 'jeu ajouté', 'jeux ajoutés')}${extra ? ` (${extra})` : ''}.` }), h('div', { class: 'row' }, h('button', { class: 'btn', type: 'button', text: 'Importer un autre fichier', onclick: idle })));
        toast(`Import CSV : ${plural(r.added, 'jeu ajouté', 'jeux ajoutés')}`);
      });

      paintTabs(); paintList(); paintApply();
      box.replaceChildren(
        h('p', {}, `${plural(data.total, 'ligne', 'lignes')} : `, h('b', { text: `${data.counts.sure} trouvé${data.counts.sure > 1 ? 's' : ''}` }), `, ${data.counts.check} à vérifier, ${data.counts.notfound} introuvable${data.counts.notfound > 1 ? 's' : ''}${data.counts.present ? `, ${data.counts.present} déjà dans ta bibliothèque` : ''}.`),
        h('p', { class: 'fine', text: 'Les jeux « trouvés » sont cochés. Pour les autres, choisis le bon jeu dans la liste puis coche-le. Tes jeux déjà présents ne sont jamais modifiés.' }),
        tabs, checkAll, list,
        h('div', { class: 'row csv-actions' }, apply, h('button', { class: 'btn quiet', type: 'button', text: 'Annuler', onclick: cancel })));
    };

    picker.addEventListener('change', async () => {
      const f = picker.files && picker.files[0];
      if (!f) return;
      if (f.size > MAX_CSV) { fail(new Error('Ce fichier est trop gros pour être une liste de jeux (8 Mo au plus).')); return; }
      try {
        const content = await f.text();
        const s = await rpc('csv_start', { content });
        showStart(f.name, s);
      } catch (e) { fail(e); }
    });

    idle();
    return box;
  }

  /* ---------- Fiche du jeu (fenêtre centrée) ---------- */

  let lastFocus = null;

  function openGame(id) {
    const c = state.cards.get(id);
    if (!c) return;
    lastFocus = document.activeElement;
    state.open = c;
    state.showOriginal = false;
    state.dirty = false;
    sheet();
    const d = $('#drawer');
    d.inert = false;
    d.scrollTop = 0;
    requestAnimationFrame(() => d.classList.add('open'));
    $('#scrim').classList.add('on');
    setTimeout(() => $('.sheet-close') && $('.sheet-close').focus({ preventScroll: true }), 80);
    enrich(c);
  }

  async function closeDrawer(skipRefresh) {
    if (document.activeElement && document.activeElement.id === 'f-hours') document.activeElement.blur();
    await new Promise((r) => setTimeout(r, 0));
    if (flushComment) await flushComment();
    flushComment = null;
    state.open = null;
    const d = $('#drawer');
    d.classList.remove('open');
    $('#scrim').classList.remove('on');
    d.inert = true;
    if (lastFocus && lastFocus.isConnected) lastFocus.focus({ preventScroll: true });
    if (state.dirty && skipRefresh !== true) {
      state.dirty = false;
      await loadLibrary();
      render(true);
    }
  }

  function fact(label, value) {
    const text = Array.isArray(value) ? value.join(', ') : value;
    if (!text) return null;
    return [h('dt', { text: label }), h('dd', { text })];
  }

  function platformField(c) {
    const own = (c.platforms || []);
    const cur = (c.entry && c.entry.platform) || '';
    const others = COMMON_PLATFORMS.filter((p) => !own.includes(p));
    const opt = (p) => h('option', { value: p, text: p, selected: p === cur });
    const known = own.includes(cur) || others.includes(cur);
    const sel = h('select', { id: 'f-platform', onchange: (e) => setPlatform(c, e.target.value) },
      h('option', { value: '', text: 'Non précisée', selected: !cur }),
      !known && cur ? opt(cur) : null,
      own.length ? h('optgroup', { label: 'Sorti sur' }, own.map(opt)) : null,
      h('optgroup', { label: own.length ? 'Autres plateformes' : 'Plateformes' }, others.map(opt)));
    return h('div', { class: 'field' }, h('label', { class: 'label', for: 'f-platform', text: 'Je joue sur' }), sel);
  }

  function hoursField(c) {
    const input = h('input', {
      id: 'f-hours', type: 'text', inputmode: 'decimal', autocomplete: 'off', spellcheck: 'false',
      placeholder: 'ex. 12h30', 'aria-describedby': 'f-hours-hint', value: (c.entry && c.entry.hours_text) || '',
    });
    let last = input.value;
    const commit = () => { if (input.value !== last) { last = input.value; setHours(c, input).then(() => { last = input.value; }); } };
    input.addEventListener('change', commit);
    input.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); commit(); input.blur(); } });
    return h('div', { class: 'field' },
      h('label', { class: 'label', for: 'f-hours', text: 'Temps de jeu' }), input,
      h('span', { class: 'hint', id: 'f-hours-hint', text: '12h30, 12,5 ou 45 min' }));
  }

  function datesField(c) {
    const e = c.entry;
    const today = dateToInput(Date.now() / 1000);
    const note = h('span', { class: 'hint', 'aria-live': 'polite' });
    const paint = () => {
      const { added, finished } = c.entry;
      if (added == null) note.textContent = 'Ce jeu a été ajouté avant l’existence des dates : renseigne-la si tu la connais.';
      else if (finished != null) {
        const d = Math.floor((finished - added) / 86400);
        note.textContent = finished < added ? 'La date de fin précède la date d’ajout.' : d === 0 ? 'Ajouté et fini le même jour.' : `${ageText(d)} entre l’ajout et la fin.`;
      } else note.textContent = ENDED.includes(c.entry.status) ? '' : `Dans ta bibliothèque depuis ${ageText(daysSince(added))}.`;
    };
    const field = (id, label, key, value) => {
      const input = h('input', { id, type: 'date', max: today, min: '1980-01-01', value: dateToInput(value) });
      input.addEventListener('change', async () => {
        const ok = await setDates(c, { [key]: inputToDate(input.value) });
        if (!ok) input.value = dateToInput(c.entry[key]);
        paint();
      });
      return h('div', { class: 'field' }, h('label', { class: 'label', for: id, text: label }), input);
    };
    paint();
    return h('div', { class: 'dates' },
      field('f-added', 'Ajouté le', 'added', e.added),
      ENDED.includes(e.status) ? field('f-finished', END_LABEL[e.status], 'finished', e.finished) : null,
      note);
  }

  function commentField(c) {
    const area = h('textarea', { id: 'f-comment', rows: 5, maxlength: 10000, placeholder: 'Impressions, où tu en es, ce que tu as aimé…', spellcheck: 'true' });
    area.value = (c.entry && c.entry.comment) || '';
    const saved = h('span', { class: 'saved', 'aria-live': 'polite' });
    let last = area.value;
    let t = 0;
    let busy = null;
    const flush = async () => {
      clearTimeout(t);
      if (area.value === last) return busy;
      const v = area.value;
      last = v;
      busy = guarded(() => rpc('set_comment', { id: c.id, comment: v })).then((r) => {
        if (r) { apply(r); state.dirty = true; saved.textContent = 'Enregistré'; saved.classList.add('on'); setTimeout(() => saved.classList.remove('on'), 1600); }
        else last = null; // échec : on réessaiera
      });
      return busy;
    };
    flushComment = flush;
    area.addEventListener('input', () => { saved.textContent = ''; clearTimeout(t); t = setTimeout(flush, 700); });
    area.addEventListener('blur', flush);
    return h('div', { class: 'field' },
      h('div', { class: 'label-row' }, h('label', { class: 'label', for: 'f-comment', text: 'Mon commentaire' }), saved), area);
  }

  function summaryField(c) {
    const fr = c.summary_fr;
    if (!c.summary) return null;
    const original = state.showOriginal || !fr;
    const tools = [];
    if (fr) {
      tools.push(h('button', { class: 'link', type: 'button', onclick: () => { state.showOriginal = !state.showOriginal; sheet(); }, text: state.showOriginal ? 'Voir la traduction' : 'Voir l’original' }));
    } else if (state.settings && state.settings.has_deepl) {
      tools.push(h('button', { class: 'btn small', type: 'button', onclick: () => translate(c) }, 'Traduire en français'));
    } else {
      tools.push(h('button', { class: 'link', type: 'button', onclick: () => { closeDrawer(); go('settings'); }, text: 'Ajouter une clé DeepL pour traduire' }));
    }
    return h('div', { class: 'field' },
      h('div', { class: 'label-row' }, h('span', { class: 'label', text: original && fr ? 'Résumé (original anglais)' : fr ? 'Résumé' : 'Résumé (en anglais)' }), h('span', { class: 'tools' }, tools)),
      h('p', { class: 'summary', lang: original ? 'en' : 'fr', text: original ? c.summary : fr }));
  }

  function sheet() {
    const c = state.open;
    if (!c) return;
    const inLib = !!c.entry;
    const cur = c.entry && c.entry.status;
    const ambient = h('div', { class: 'ambient', 'aria-hidden': 'true' }, validCover(c) ? h('img', { src: c.cover_url, alt: '' }) : fallbackEl(c.name));
    const pills = STATUSES.map((s) => h('button', {
      class: 'pill', type: 'button', 'aria-pressed': cur === s.key, vars: { '--c': `var(--st-${s.key})` },
      onclick: () => setStatus(c, s.key),
    }, h('span', { class: 'd' }), s.label));
    const stars = c.entry && c.entry.stars;
    const rating = h('div', { class: 'rating-row' }, starsWidget(stars, (v) => setRating(c, v)),
      stars ? h('span', { class: 'rating-val', text: fmt(stars) + ' / 5' }) : h('span', { class: 'rating-val none', text: 'Pas encore noté' }));

    const rows = [
      fact('Sortie', c.released || (c.year && String(c.year))),
      fact(c.developers && c.developers.length > 1 ? 'Studios' : 'Studio', c.developers),
      fact(c.publishers && c.publishers.length > 1 ? 'Éditeurs' : 'Éditeur', c.publishers),
      fact('Franchise', c.franchise),
      fact('Plateformes', c.platforms),
      fact('Modes de jeu', c.modes),
      fact('Perspective', c.perspectives),
      fact('Thèmes', c.themes),
      fact('Note IGDB', c.igdb_rating ? `${Math.round(c.igdb_rating)} sur 100` : null),
    ].filter(Boolean);
    const detailed = c.rev >= 2 || (c.developers && c.developers.length) || (c.publishers && c.publishers.length);
    const facts = h('dl', { class: 'facts' }, rows.flat());
    const hint = !detailed
      ? h('p', { class: 'note', text: state.mode !== 'igdb' ? 'Connecte IGDB (Réglages) pour afficher studio, éditeur, modes de jeu et plus.' : enrichingId === c.id ? 'Récupération des détails…' : 'Détails indisponibles pour le moment.' })
      : null;

    const main = h('div', { class: 'sheet-main' },
      h('div', { class: 'sheet-head' },
        h('h2', { id: 'sheet-title', text: c.name }),
        h('div', { class: 'chips' }, (c.genres || []).slice(0, 4).map((t) => h('span', { class: 'chip', text: t })))),
      inLib ? h('div', { class: 'field' }, h('span', { class: 'label', text: 'Statut' }), h('div', { class: 'pills', role: 'group', 'aria-label': 'Statut' }, pills)) : null,
      inLib ? h('div', { class: 'three' },
        platformField(c),
        hoursField(c),
        h('div', { class: 'field' }, h('span', { class: 'label', text: 'Ma note' }), rating)) : null,
      inLib ? datesField(c) : null,
      inLib ? commentField(c) : null,
      summaryField(c),
      h('div', { class: 'sheet-actions' },
        inLib
          ? h('button', { class: 'btn danger', type: 'button', onclick: () => removeGame(c) }, icon('trash', 17), 'Retirer de la bibliothèque')
          : [h('div', { class: 'pills add', role: 'group', 'aria-label': 'Ajouter avec le statut' }, STATUSES.filter((s) => s.key !== 'completed' && s.key !== 'abandoned').map((s) => h('button', {
            class: 'pill', type: 'button', vars: { '--c': `var(--st-${s.key})` }, onclick: () => add(c, s.key),
          }, h('span', { class: 'd' }), s.label))),
            h('button', { class: 'btn primary', type: 'button', onclick: () => add(c, 'backlog') }, icon('plus', 17), 'Ajouter au backlog')]));
    const aside = h('aside', { class: 'sheet-aside' }, coverEl(c), facts, hint);
    const oldHours = $('#f-hours');
    const hoursTyped = oldHours && document.activeElement === oldHours ? { v: oldHours.value, a: oldHours.selectionStart, b: oldHours.selectionEnd } : null;
    const oldArea = $('#f-comment');
    const typed = oldArea ? { v: oldArea.value, focus: document.activeElement === oldArea, a: oldArea.selectionStart, b: oldArea.selectionEnd } : null;
    const keepScroll = $('#drawer .sheet-scroll') ? $('#drawer .sheet-scroll').scrollTop : 0;
    const body = h('div', { class: 'sheet-scroll' },
      h('div', { class: 'sheet-body' }, aside, main));
    $('#drawer').setAttribute('aria-labelledby', 'sheet-title');
    $('#drawer').replaceChildren(ambient,
      h('button', { class: 'sheet-close', type: 'button', 'aria-label': 'Fermer', onclick: () => closeDrawer() }, icon('close', 18)),
      body);
    body.scrollTop = keepScroll;
    const newHours = $('#f-hours');
    if (hoursTyped && newHours) { newHours.value = hoursTyped.v; newHours.focus({ preventScroll: true }); newHours.setSelectionRange(hoursTyped.a, hoursTyped.b); }
    const newArea = $('#f-comment');
    if (typed && newArea) { // une mise à jour de la fiche ne doit jamais effacer ce qu'on est en train d'écrire
      if (newArea.value !== typed.v) newArea.value = typed.v;
      if (typed.focus) { newArea.focus({ preventScroll: true }); newArea.setSelectionRange(typed.a, typed.b); }
    }
  }

  /* ---------- Recherche globale ---------- */

  let seq = 0;
  let timer = 0;
  function onQuery() {
    const q = $('#q').value.trim();
    state.query = q;
    clearTimeout(timer);
    if (state.view !== 'search') { state.view = 'search'; renderNav(); }
    if (q.length < 2) { state.found = null; render(); return; }
    timer = setTimeout(async () => {
      const mine = ++seq;
      try {
        const [res, lib] = await Promise.all([rpc('search', { query: q }), rpc('library', { query: q })]);
        if (mine !== seq) return;
        remember(lib.entries);
        remember(res.results);
        const owned = new Set(lib.entries.map((e) => e.id));
        state.found = { source: res.source, warning: res.warning, mine: lib.entries, results: res.results.filter((r) => !owned.has(r.id)) };
        if (res.source === 'igdb') state.mode = 'igdb';
        render();
      } catch (e) {
        if (mine === seq) toast(e.message);
      }
    }, 240);
  }

  /* ---------- Démarrage ---------- */

  async function init() {
    $('#search-icon').append(icon('search', 20));
    const isMac = /Mac/i.test(navigator.platform || '');
    $('#kbd').textContent = isMac ? '⌘ K' : 'Ctrl K';
    $('#q').addEventListener('input', onQuery);
    $('#q').addEventListener('keydown', (e) => {
      if (e.key === 'Escape') { $('#q').value = ''; $('#q').blur(); state.query = ''; go('library'); }
    });
    $('#conn').addEventListener('click', () => go('settings'));
    $('#scrim').addEventListener('click', closeDrawer);
    document.addEventListener('keydown', (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); closeDrawer(); $('#q').focus(); $('#q').select(); }
      else if (e.key === '/' && !/INPUT|SELECT|TEXTAREA/.test(document.activeElement.tagName)) { e.preventDefault(); $('#q').focus(); }
      else if (e.key === 'Escape' && state.open) closeDrawer();
    });
    try {
      const st = await rpc('status');
      state.mode = st.mode;
      state.notice = st.notice || null;
      await loadLibrary();
      state.settings = await rpc('get_settings').catch(() => null);
    } catch (e) {
      toast('Impossible de joindre le moteur : ' + e.message);
    }
    renderNav();
    render();
    // Complète en arrière-plan les fiches enregistrées avant l'ajout des détails (studio, éditeur…).
    if (state.mode === 'igdb' && state.total) {
      rpc('sync_metadata').then(async (r) => {
        if (r && r.updated) { await loadLibrary(); if (state.view === 'library' && !state.open) render(true); }
      }).catch(() => {});
    }
  }

  init();
})();
