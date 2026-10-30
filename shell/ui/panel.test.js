'use strict';

// Drives panel.js against a minimal fake DOM. There is no browser here, so this checks the wiring
// (send, waiting state, disabled sending, rendering of answers), not the look of the page.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');

class Node {
  constructor(tag) {
    this.tagName = tag;
    this.children = [];
    this.attributes = {};
    this.listeners = {};
    this.className = '';
    this.value = '';
    this.disabled = false;
    this.dir = '';
    this.scrollTop = 0;
    this.scrollHeight = 0;
    this.nodeText = '';
  }
  set textContent(text) { this.children = []; this.nodeText = String(text); }
  get textContent() { return this.nodeText + this.children.map((c) => c.textContent).join(''); }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; this.nodeText = ''; }
  setAttribute(name, value) { this.attributes[name] = String(value); }
  getAttribute(name) { return this.attributes[name]; }
  addEventListener(type, fn) { this.listeners[type] = fn; }
  querySelectorAll() { return []; }
  focus() {}
  all(predicate, found = []) {
    if (predicate(this)) found.push(this);
    this.children.forEach((c) => c.all(predicate, found));
    return found;
  }
}

const read = (...parts) => fs.readFileSync(path.join(__dirname, ...parts), 'utf8');
const tick = () => new Promise((resolve) => setImmediate(resolve));

async function loadPanel(chat, storage) {
  const ids = {};
  for (const id of ['conversation-list', 'message-list', 'message-input', 'language', 'composer', 'new-conversation', 'setting-digits', 'setting-calendar', 'setting-week-start']) ids[id] = new Node(id);
  ids.language.value = 'en';
  const sendButton = new Node('button');
  const posts = [];
  global.document = {
    documentElement: { lang: '', dir: '' },
    querySelectorAll: () => [],
    getElementById: (id) => ids[id],
    querySelector: (selector) => (selector === '#composer button[type="submit"]' ? sendButton : null),
    createElement: (tag) => new Node(tag),
    createTextNode: (text) => { const n = new Node('#text'); n.nodeText = text; return n; },
  };
  global.fetch = async (url, options) => {
    if (url.startsWith('../../locales/')) {
      const lang = url.split('/')[3];
      return { text: async () => read('..', '..', 'locales', lang, 'scarfeeos.ftl') };
    }
    assert.strictEqual(url, '/chat');
    posts.push(JSON.parse(options.body));
    assert.strictEqual(options.method, 'POST');
    return chat();
  };
  if (storage === undefined) {
    delete global.localStorage;
  } else if (storage === 'throws') {
    Object.defineProperty(global, 'localStorage', { configurable: true, get() { throw new Error('SecurityError'); } });
  } else {
    Object.defineProperty(global, 'localStorage', { configurable: true, value: storage });
  }
  global.ScarfeeSettings = require('./settings.js');
  global.ScarfeeI18n = require('./i18n.js');
  global.ScarfeeRender = require('./render.js');
  delete require.cache[require.resolve('./panel.js')];
  require('./panel.js');
  await tick();
  await tick();
  return { ids, sendButton, posts };
}

const submit = (ids) => ids.composer.listeners.submit({ preventDefault() {} });
const shown = (ids) => ids['message-list'].textContent.replace(/[⁨⁩]/g, '');

test.after(() => {
  delete global.document;
  delete global.fetch;
  delete global.localStorage;
  delete global.ScarfeeSettings;
});

function memoryStorage(initial) {
  const data = new Map(Object.entries(initial || {}));
  return { data, getItem: (k) => (data.has(k) ? data.get(k) : null), setItem: (k, v) => { data.set(k, String(v)); } };
}

const change = (control, value) => { control.value = value; control.listeners.change(); };

test('sending shows a waiting state, blocks a second send, then shows the answer', async () => {
  let answer;
  const { ids, sendButton, posts } = await loadPanel(() => new Promise((resolve) => { answer = resolve; }));
  assert.match(shown(ids), /How can I help you\?/);

  ids['message-input'].value = '  What time is it?  ';
  submit(ids);
  assert.deepStrictEqual(posts, [{ text: 'What time is it?', language: 'en' }]);
  assert.strictEqual(sendButton.disabled, true);
  assert.strictEqual(ids['message-input'].value, '');
  assert.match(shown(ids), /Working on it\. A reply can take more than a minute\./);
  assert.match(shown(ids), /Elapsed: 0:0\d/);
  const busy = ids['message-list'].all((n) => n.attributes['aria-busy'] === 'true');
  assert.strictEqual(busy.length, 1);

  ids['message-input'].value = 'second';
  submit(ids);
  ids['message-input'].listeners.keydown({ key: 'Enter', shiftKey: false, isComposing: false, preventDefault() {} });
  assert.strictEqual(posts.length, 1, 'no second request while waiting');

  answer({ status: 200, text: async () => JSON.stringify({ id: 1, kind: 'time', date: '2026-10-30', hour: 9, minute: 5, weekday: 'Friday' }) });
  await tick();
  await tick();
  assert.strictEqual(sendButton.disabled, false);
  assert.match(shown(ids), /What time is it\?/);
  assert.match(shown(ids), /The time on this device is 09:05, on Friday 2026-10-30\./);
  assert.doesNotMatch(shown(ids), /Working on it/);
  assert.strictEqual(ids['message-list'].all((n) => n.attributes['aria-busy'] === 'true').length, 0);
});

test('the answer is shown again in the other language when the language changes', async () => {
  const { ids } = await loadPanel(async () => ({ status: 200, text: async () => JSON.stringify({ kind: 'declined' }) }));
  ids['message-input'].value = 'question';
  submit(ids);
  await tick();
  await tick();
  assert.match(shown(ids), /does not answer religious questions/);
  ids.language.value = 'ar';
  ids.language.listeners.change();
  assert.match(shown(ids), /لا يجيب ScarfeeOS عن الأسئلة الدينية/);
  assert.strictEqual(global.document.documentElement.dir, 'rtl');
});

test('model text lands in a text node, a bridge failure is an error, and sending works again after it', async () => {
  let fail = true;
  const { ids, sendButton, posts } = await loadPanel(async () => {
    if (fail) throw new TypeError('fetch failed');
    return { status: 200, text: async () => JSON.stringify({ kind: 'reply', text: '<b>hi</b>', foreign_words: [{ word: 'привет', script: 'cyrillic' }] }) };
  });
  ids['message-input'].value = 'one';
  submit(ids);
  await tick();
  await tick();
  assert.match(shown(ids), /The assistant could not be reached/);
  assert.strictEqual(sendButton.disabled, false);

  fail = false;
  ids['message-input'].value = 'two';
  submit(ids);
  await tick();
  await tick();
  assert.strictEqual(posts.length, 2);
  assert.ok(shown(ids).includes('<b>hi</b>'));
  assert.match(shown(ids), /Warning: .*привет/);
  const tags = new Set(ids['message-list'].all(() => true).map((n) => n.tagName));
  assert.deepStrictEqual([...tags].filter((tag) => !['message-list', 'li', 'div', 'span', 'p', 'bdi', '#text'].includes(tag)), []);
});

test('an HTTP error from the bridge is shown with its message', async () => {
  const { ids } = await loadPanel(async () => ({ status: 502, text: async () => JSON.stringify({ kind: 'error', message: 'the core stopped (exit code 3)' }) }));
  ids['message-input'].value = 'x';
  submit(ids);
  await tick();
  await tick();
  assert.match(shown(ids), /Something went wrong: the core stopped \(exit code 3\)/);
});

const TIME_ANSWER = { status: 200, text: async () => JSON.stringify({ kind: 'time', date: '2026-01-01', hour: 9, minute: 5, weekday: 'Thursday' }) };

test('digits and calendar settings apply at once to answers already shown, and are saved', async () => {
  const storage = memoryStorage();
  const { ids } = await loadPanel(async () => TIME_ANSWER, storage);
  ids['message-input'].value = 'time?';
  submit(ids);
  await tick();
  await tick();
  assert.match(shown(ids), /09:05, on Thursday 2026-01-01\./);

  change(ids['setting-digits'], 'eastern');
  assert.match(shown(ids), /٠٩:٠٥, on Thursday ٢٠٢٦-٠١-٠١\./);
  assert.match(shown(ids), /time\?/, 'the user message is not changed');

  change(ids['setting-calendar'], 'both');
  assert.match(shown(ids), /٢٠٢٦-٠١-٠١ \(about ١٢ Rajab ١٤٤٧ AH\)/);

  change(ids['setting-digits'], 'western');
  assert.match(shown(ids), /2026-01-01 \(about 12 Rajab 1447 AH\)/);

  change(ids['setting-week-start'], 'monday');
  assert.deepStrictEqual(JSON.parse(storage.data.get('scarfeeos.panel.settings')),
    { language: 'en', digits: 'western', calendar: 'both', weekStart: 'monday' });
});

test('Arabic starts with Western digits, and the saved settings are used on the next start', async () => {
  const { ids } = await loadPanel(async () => TIME_ANSWER, memoryStorage());
  assert.strictEqual(ids['setting-digits'].value, 'western');
  change(ids.language, 'ar');
  ids['message-input'].value = 'x';
  submit(ids);
  await tick();
  await tick();
  assert.match(shown(ids), /09:05/);
  assert.doesNotMatch(shown(ids), /[٠-٩]/);

  const saved = memoryStorage({ 'scarfeeos.panel.settings': JSON.stringify({ language: 'ar', digits: 'eastern', calendar: 'both', weekStart: 'saturday' }) });
  const again = await loadPanel(async () => TIME_ANSWER, saved);
  assert.strictEqual(again.ids.language.value, 'ar');
  assert.strictEqual(again.ids['setting-digits'].value, 'eastern');
  assert.strictEqual(again.ids['setting-calendar'].value, 'both');
  assert.strictEqual(again.ids['setting-week-start'].value, 'saturday');
  assert.strictEqual(global.document.documentElement.dir, 'rtl');
});

test('settings still work when storage is missing, blocked or throws on write', async () => {
  for (const storage of [undefined, 'throws', { getItem() { throw new Error('x'); }, setItem() { throw new Error('quota'); } }]) {
    const { ids } = await loadPanel(async () => TIME_ANSWER, storage);
    ids['message-input'].value = 'x';
    submit(ids);
    await tick();
    await tick();
    change(ids['setting-digits'], 'eastern');
    assert.match(shown(ids), /٠٩:٠٥/);
  }
});

test('model text, URLs and file names keep their digits in either setting', async () => {
  const reply = { kind: 'reply', text: 'Open https://example.com/a1/b2 and report-2024.pdf at 10:30' };
  const { ids } = await loadPanel(async () => ({ status: 200, text: async () => JSON.stringify(reply) }), memoryStorage());
  ids['message-input'].value = 'file 123';
  submit(ids);
  await tick();
  await tick();
  change(ids['setting-digits'], 'eastern');
  assert.ok(shown(ids).includes(reply.text));
  assert.ok(shown(ids).includes('file 123'));
  assert.doesNotMatch(shown(ids), /[٠-٩]/);
});
