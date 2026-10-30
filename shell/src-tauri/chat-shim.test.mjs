import test from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import vm from 'node:vm';

const SOURCE = fs.readFileSync(new URL('./chat-shim.js', import.meta.url), 'utf8');

// Runs the shim against a fake window. `invoke` stands in for the `ask` command.
function load(invoke) {
  const fetched = [];
  const window = {
    fetch: (...args) => {
      fetched.push(args);
      return Promise.resolve('real fetch');
    },
    __TAURI__: { core: { invoke } },
  };
  window.window = window;
  vm.runInNewContext(SOURCE, { window, Response });
  return { window, fetched };
}

const post = (body) => ({ method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });

test('a /chat post becomes one ask call and the response line comes back as a 200', async () => {
  const calls = [];
  const { window } = load(async (command, args) => {
    calls.push([command, JSON.parse(args.line)]);
    return JSON.stringify({ id: 1, kind: 'reply', text: 'أهلاً', foreign_words: [] });
  });
  const response = await window.fetch('/chat', post({ text: 'مرحبا', language: 'ar' }));
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { id: 1, kind: 'reply', text: 'أهلاً', foreign_words: [] });
  assert.deepEqual(calls, [['ask', { id: 1, text: 'مرحبا', language: 'ar' }]]);
});

test('each request gets a new id', async () => {
  const ids = [];
  const { window } = load(async (_command, args) => {
    ids.push(JSON.parse(args.line).id);
    return '{"kind":"reply"}';
  });
  await window.fetch('/chat', post({ text: 'a', language: 'en' }));
  await window.fetch('/chat', post({ text: 'b', language: 'en' }));
  assert.deepEqual(ids, [1, 2]);
});

test('a failed command is a 502 error response the panel can read', async () => {
  const { window } = load(async () => {
    throw 'cannot start the core (scarfeeos-core): No such file or directory';
  });
  const response = await window.fetch('/chat', post({ text: 'hi', language: 'en' }));
  assert.equal(response.status, 502);
  assert.deepEqual(await response.json(), {
    kind: 'error',
    message: 'cannot start the core (scarfeeos-core): No such file or directory',
  });
});

test('a bad body is a 400 and never reaches the core', async () => {
  let called = false;
  const { window } = load(async () => {
    called = true;
    return '{}';
  });
  for (const init of [post({ text: 'hi', language: 'fr' }), post({ text: 5, language: 'en' }), { method: 'POST', body: '{' }]) {
    const response = await window.fetch('/chat', init);
    assert.equal(response.status, 400);
    assert.equal((await response.json()).kind, 'error');
  }
  assert.equal(called, false);
});

test('every other request goes to the real fetch', async () => {
  const { window, fetched } = load(async () => '{}');
  assert.equal(await window.fetch('../../locales/en/scarfeeos.ftl'), 'real fetch');
  assert.equal(await window.fetch('/chat'), 'real fetch');
  assert.equal(fetched.length, 2);
});
