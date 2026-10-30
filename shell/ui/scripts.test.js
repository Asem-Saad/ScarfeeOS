// The page loads these as classic scripts, which share one global scope, so a
// repeated top-level name stops the whole panel. Each file's own tests cannot see that.
'use strict';

const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

test('every script in index.html can load into one global scope', () => {
  const html = fs.readFileSync(path.join(__dirname, 'index.html'), 'utf8');
  const scripts = [...html.matchAll(/<script src="([^"]+)"/g)].map((m) => m[1]);
  assert.ok(scripts.length > 0);
  const declared = new Map();
  for (const script of scripts) {
    const source = fs.readFileSync(path.join(__dirname, script), 'utf8');
    new vm.Script(source, { filename: script });
    for (const [, name] of source.matchAll(/^(?:const|let|class)\s+([A-Za-z_$][\w$]*)/gm)) {
      assert.ok(!declared.has(name), `${name} is declared in both ${declared.get(name)} and ${script}`);
      declared.set(name, script);
    }
  }
});
