// Unit tests for the pure formatters in storage/files/js/services/pi-format.js,
// run by `node --test tests/frontend/` (`just test-frontend`) with no
// dependencies beyond node's built-in test runner.
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  summarise, pretty, approxTokens, countFences, compact, price,
  modelLabel, modelTitle,
} from '../../storage/files/js/services/pi-format.js';

test('summarise prefers the path-like key and falls back to key names', () => {
  assert.equal(summarise({ path: '/a/b.rs', pattern: 'x' }), '/a/b.rs');
  assert.equal(summarise({ file_path: 'f.rs' }), 'f.rs');
  assert.equal(summarise({ command: 'ls -la' }), 'ls -la');
  assert.equal(summarise({ alpha: 1, beta: 2 }), 'alpha, beta');
  assert.equal(summarise({}), '');
  assert.equal(summarise(null), '');
  assert.equal(summarise('not-an-object'), '');
});

test('summarise ignores a non-string value under a path-like key', () => {
  assert.equal(summarise({ path: 42, other: true }), 'path, other');
});

test('pretty stringifies and survives circular input', () => {
  assert.equal(pretty({ a: 1 }), '{\n  "a": 1\n}');
  const cyc = {};
  cyc.self = cyc;
  assert.equal(pretty(cyc), String(cyc));
});

test('approxTokens floors at one token', () => {
  assert.equal(approxTokens(''), 1);
  assert.equal(approxTokens('abcd'), 1);
  assert.equal(approxTokens('a'.repeat(40)), 10);
});

test('countFences counts fence openers line-anchored', () => {
  assert.equal(countFences('no fences'), 0);
  assert.equal(countFences('```js\ncode\n```'), 2);
  assert.equal(countFences('inline ``` mid-line is not a fence'), 0);
  assert.equal(countFences('  ```\nindented fence\n'), 1);
});

test('compact abbreviates thousands and millions without trailing .0', () => {
  assert.equal(compact(999), '999');
  assert.equal(compact(1000), '1k');
  assert.equal(compact(1200), '1.2k');
  assert.equal(compact(200000), '200k');
  assert.equal(compact(1000000), '1m');
  assert.equal(compact(1500000), '1.5m');
});

test('price keeps cents below a dollar and rounds above', () => {
  assert.equal(price(0.35), '$0.35');
  assert.equal(price(0.3), '$0.3');
  assert.equal(price(5), '$5');
  assert.equal(price(24.6), '$25');
  assert.equal(price(0), null);
  assert.equal(price(-1), null);
  assert.equal(price('5'), null);
  assert.equal(price(NaN), null);
});

test('modelLabel joins id, rates, and context when present', () => {
  const m = { id: 'claude-x', input_per_million: 5, output_per_million: 25, context_window: 200000 };
  assert.equal(modelLabel(m), 'claude-x  ·  $5/$25  ·  200k ctx');
  assert.equal(modelLabel({ id: 'bare' }), 'bare');
  assert.equal(modelLabel({ id: 'half', input_per_million: 5 }), 'half');
});

test('modelTitle spells the same facts out in prose', () => {
  const m = {
    id: 'claude-x', input_per_million: 5, output_per_million: 25,
    context_window: 200000, max_output_tokens: 64000,
  };
  assert.equal(
    modelTitle(m),
    'input $5 / output $25 per million tokens · context window 200k · max output 64k',
  );
  assert.equal(modelTitle({}), '');
});
