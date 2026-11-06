(function () {
  'use strict';

  var KEY = 'bp.theme';
  var CHOICES = ['system', 'light', 'dark'];

  function resolveTheme(choice, systemDark) {
    if (choice === 'light' || choice === 'dark') return choice;
    return systemDark ? 'dark' : 'light';
  }

  function readChoice(storage) {
    try {
      var v = storage.getItem(KEY);
      return CHOICES.indexOf(v) !== -1 ? v : 'system';
    } catch (e) {
      return 'system';
    }
  }

  function saveChoice(storage, choice) {
    try {
      storage.setItem(KEY, choice);
    } catch (e) { /* stockage indisponible : le choix vaut pour la session */ }
  }

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = { resolveTheme: resolveTheme, readChoice: readChoice, saveChoice: saveChoice };
  }

  if (typeof document === 'undefined') return;

  function store() {
    try { return window.localStorage; } catch (e) { return null; }
  }

  var mql = null;
  try { mql = window.matchMedia('(prefers-color-scheme: dark)'); } catch (e) { mql = null; }

  var choice = readChoice(store());
  var listeners = [];

  function systemDark() {
    return mql ? mql.matches : true;
  }

  function resolved() {
    return resolveTheme(choice, systemDark());
  }

  function apply() {
    var r = resolved();
    document.documentElement.dataset.theme = r;
    for (var i = 0; i < listeners.length; i++) {
      try { listeners[i](r); } catch (e) { /* un écouteur ne bloque pas les autres */ }
    }
  }

  function set(next) {
    choice = CHOICES.indexOf(next) !== -1 ? next : 'system';
    saveChoice(store(), choice);
    apply();
  }

  if (mql) {
    var onSystem = function () { if (choice === 'system') apply(); };
    if (mql.addEventListener) mql.addEventListener('change', onSystem);
    else if (mql.addListener) mql.addListener(onSystem);
  }

  window.BPTheme = {
    get: function () { return choice; },
    set: set,
    resolved: resolved,
    onChange: function (cb) { if (typeof cb === 'function') listeners.push(cb); }
  };

  document.documentElement.dataset.theme = resolved();
})();
