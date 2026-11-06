const test = require('node:test');
const assert = require('node:assert/strict');
const { resolveTheme, readChoice, saveChoice } = require('../../ui/theme.js');

test('resolveTheme', () => {
  assert.equal(resolveTheme('system', true), 'dark');
  assert.equal(resolveTheme('system', false), 'light');
  assert.equal(resolveTheme('light', true), 'light');
  assert.equal(resolveTheme('dark', false), 'dark');
  assert.equal(resolveTheme('bleu', false), 'light');
});

test('readChoice', () => {
  assert.equal(readChoice({ getItem: () => 'dark' }), 'dark');
  assert.equal(readChoice({ getItem: () => null }), 'system');
  assert.equal(readChoice({ getItem: () => 'bleu' }), 'system');
  assert.equal(readChoice({ getItem: () => { throw new Error('x'); } }), 'system');
  assert.equal(readChoice(null), 'system');
});

test('saveChoice', () => {
  const calls = [];
  saveChoice({ setItem: (k, v) => calls.push([k, v]) }, 'light');
  assert.deepEqual(calls, [['bp.theme', 'light']]);
  assert.doesNotThrow(() => saveChoice({ setItem: () => { throw new Error('x'); } }, 'dark'));
  assert.doesNotThrow(() => saveChoice(null, 'dark'));
});
