import test from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import vm from 'node:vm';

const SOURCE = fs.readFileSync(new URL('./chat-shim.js', import.meta.url), 'utf8');

// Runs the shim against a fake window. `invoke` stands in for the `ask` command.
function load(invoke) {
  const fetched = [];
  const backs = [];
  const listeners = [];
  const window = {
    fetch: (...args) => {
      fetched.push(args);
      return Promise.resolve('real fetch');
    },
    __TAURI__: { core: { invoke } },
    history: { back: () => backs.push('back') },
    addEventListener: (type, listener) => listeners.push([type, listener]),
  };
  window.window = window;
  vm.runInNewContext(SOURCE, { window, Response });
  return { window, fetched, backs, listeners };
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

test('the held-action, calendar and hardware requests reach the core with only their own fields', async () => {
  const sent = [];
  const { window } = load(async (_command, args) => {
    sent.push(JSON.parse(args.line));
    return '{"kind":"events"}';
  });
  for (const body of [
    { confirm: 'abc' },
    { confirm: 'abc', time: '2026-11-02T11:00:00' },
    { cancel: 'abc' },
    { upcoming: true },
    { upcoming: true, from: '2026-11-01' },
    { delete_event: 'uid-1' },
    { hardware: true },
  ]) {
    const response = await window.fetch('/chat', post(body));
    assert.equal(response.status, 200, JSON.stringify(body));
  }
  assert.deepEqual(sent, [
    { id: 1, confirm: 'abc' },
    { id: 2, confirm: 'abc', time: '2026-11-02T11:00:00' },
    { id: 3, cancel: 'abc' },
    { id: 4, upcoming: true },
    { id: 5, upcoming: true, from: '2026-11-01' },
    { id: 6, delete_event: 'uid-1' },
    { id: 7, hardware: true },
  ]);
});

test('a caller cannot choose the id', async () => {
  const ids = [];
  const { window } = load(async (_command, args) => {
    ids.push(JSON.parse(args.line).id);
    return '{}';
  });
  await window.fetch('/chat', post({ id: 99, hardware: true }));
  assert.deepEqual(ids, [1]);
});

test('a body that mixes kinds or has a wrong field is a 400', async () => {
  let called = false;
  const { window } = load(async () => {
    called = true;
    return '{}';
  });
  for (const body of [
    { hardware: true, upcoming: true },
    { text: 'hi', language: 'en', confirm: 'abc' },
    { cancel: 'abc', time: '11:00' },
    { confirm: '' },
    { confirm: 5 },
    { upcoming: false },
    { upcoming: true, from: 3 },
    { hardware: 'yes' },
    { delete_event: '' },
    { language: 'en' },
    [],
    null,
  ]) {
    const response = await window.fetch('/chat', post(body));
    assert.equal(response.status, 400, JSON.stringify(body));
  }
  assert.equal(called, false);
});

test('every other request goes to the real fetch', async () => {
  const { window, fetched } = load(async () => '{}');
  assert.equal(await window.fetch('../../locales/en/scarfeeos.ftl'), 'real fetch');
  assert.equal(await window.fetch('/chat'), 'real fetch');
  assert.equal(fetched.length, 2);
});

test('Alt+Left goes back, and other keys do not', () => {
  const { backs, listeners } = load(async () => '{}');
  const [[type, onKey]] = listeners;
  assert.equal(type, 'keydown');
  let prevented = 0;
  const key = (fields) => onKey({ key: 'ArrowLeft', altKey: true, preventDefault: () => prevented++, ...fields });
  key({});
  key({ altKey: false });
  key({ key: 'ArrowRight' });
  key({ ctrlKey: true });
  key({ shiftKey: true });
  assert.deepEqual(backs, ['back']);
  assert.equal(prevented, 1);
});
