'use strict';

// Drives panel.js against a minimal fake DOM. There is no browser here, so this checks the wiring
// (send, waiting state, disabled sending, rendering of answers), not the look of the page.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

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

// Loads the scripts exactly as index.html lists them, as classic scripts in one global scope (no require,
// no module), against a fake DOM that only answers for ids that index.html really has.
async function loadPanel(chat, storage, session) {
  const html = read('index.html');
  const known = new Set([...html.matchAll(/\bid="([^"]+)"/g)].map((m) => m[1]));
  const ids = {};
  for (const id of known) ids[id] = new Node(id);
  ids.language.value = 'en';
  const sendButton = new Node('button');
  const posts = [];
  const document = {
    documentElement: { lang: '', dir: '' },
    querySelectorAll: () => [],
    getElementById: (id) => {
      assert.ok(known.has(id), `panel.js asks for #${id}, which index.html does not have`);
      return ids[id];
    },
    querySelector: (selector) => (selector === '#composer button[type="submit"]' ? sendButton : null),
    createElement: (tag) => new Node(tag),
    createTextNode: (text) => { const n = new Node('#text'); n.nodeText = text; return n; },
  };
  const fetch = async (url, options) => {
    if (url.startsWith('../../locales/')) {
      const lang = url.split('/')[3];
      return { text: async () => read('..', '..', 'locales', lang, 'scarfeeos.ftl') };
    }
    assert.strictEqual(url, '/chat');
    posts.push(JSON.parse(options.body));
    assert.strictEqual(options.method, 'POST');
    return chat();
  };
  const sandbox = { document, fetch, console, setInterval, clearInterval, setTimeout, Date };
  if (storage === 'throws') {
    Object.defineProperty(sandbox, 'localStorage', { get() { throw new Error('SecurityError'); } });
  } else if (storage !== undefined) {
    sandbox.localStorage = storage;
  }
  if (session === 'throws') {
    Object.defineProperty(sandbox, 'sessionStorage', { get() { throw new Error('SecurityError'); } });
  } else if (session !== undefined) {
    sandbox.sessionStorage = session;
  }
  const context = vm.createContext(sandbox);
  const scripts = [...html.matchAll(/<script src="([^"]+)"/g)].map((m) => m[1]);
  assert.deepStrictEqual(scripts, ['i18n.js', 'settings.js', 'render.js', 'panel.js']);
  for (const script of scripts) {
    vm.runInContext(read(script), context, { filename: script });
  }
  await tick();
  await tick();
  return { ids, sendButton, posts, document };
}

const submit = (ids) => ids.composer.listeners.submit({ preventDefault() {} });
const shown = (ids) => ids['message-list'].textContent.replace(/[⁨⁩]/g, '');

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
  const { ids, document } = await loadPanel(async () => ({ status: 200, text: async () => JSON.stringify({ kind: 'declined' }) }));
  ids['message-input'].value = 'question';
  submit(ids);
  await tick();
  await tick();
  assert.match(shown(ids), /does not answer religious questions/);
  ids.language.value = 'ar';
  ids.language.listeners.change();
  assert.match(shown(ids), /لا يجيب ScarfeeOS عن الأسئلة الدينية/);
  assert.strictEqual(document.documentElement.dir, 'rtl');
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
  assert.strictEqual(again.document.documentElement.dir, 'rtl');
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

// ---- results and buttons ----

const reply = (body, status = 200) => ({ status, text: async () => JSON.stringify(body) });
// Answers the requests one after another, as the core would.
const sequence = (...bodies) => { const left = [...bodies]; return async () => reply(left.shift()); };
const run = (tool, args, result) => ({ kind: 'run', tool, arguments: args, result });
const buttons = (ids) => ids['message-list'].all((n) => n.tagName === 'button');
const button = (ids, label) => buttons(ids).find((b) => b.textContent === label);
const ask = async (ids, text = 'x') => { ids['message-input'].value = text; submit(ids); await tick(); await tick(); };
const click = async (node) => { node.listeners.click(); await tick(); await tick(); };
const ID = 'f'.repeat(32);
const SHOWN_TAGS = ['message-list', 'li', 'ul', 'div', 'span', 'p', 'bdi', 'input', 'button', '#text'];
const unknownTags = (ids) => [...new Set(ids['message-list'].all(() => true).map((n) => n.tagName))].filter((tag) => !SHOWN_TAGS.includes(tag));

test('a run shows what happened for each result type, and never says that nothing was done', async () => {
  const cases = [
    [run('open_app', { name: 'calc' }, { ok: true, type: 'opened', app: { id: 'calc.desktop', name: 'Calculator' } }), /Opened Calculator\./],
    [run('create_reminder', { text: 'x', time: '2026-10-31T10:00:00', repeat: null }, { ok: true, type: 'event', event: { uid: 'u', summary: 'call the bank', start: '2026-10-31T10:00:00', attendees: [], repeat: null } }),
      /Event created: “call the bank” at 2026-10-31 10:00\./],
    [run('open_app', { name: 'calc' }, { ok: false, error: { code: 'app_not_found', message: 'x', closest: ['Calculator', 'Calendar'] } }),
      /I could not find an app called calc\.[\s\S]*Closest app names: Calculator, Calendar/],
    [run('list_files', { path: '/etc' }, { ok: false, error: { code: 'outside_root', message: 'x' } }), /outside the folders I am allowed to use/],
    [run('list_files', { path: '/x' }, { ok: true, type: 'listing', path: '/x', entries: [], truncated: false }), /\/x is empty\./],
  ];
  for (const [response, expected] of cases) {
    const { ids } = await loadPanel(sequence(response));
    await ask(ids);
    assert.match(shown(ids), expected);
    assert.doesNotMatch(shown(ids), /Nothing was done|not connected/);
    assert.strictEqual(buttons(ids).length, 0);
    assert.deepStrictEqual(unknownTags(ids), []);
  }
});

test('a listing shows names in bidi text, 20 at most, and the digits setting leaves them alone', async () => {
  const names = ['report 2026.txt', 'تقرير ٢٠٢٦.pdf', '<img src=x onerror=alert(1)>.txt', ...Array.from({ length: 27 }, (_, i) => `file ${i}`)];
  const entries = names.map((name) => ({ path: `/home/u/Documents/${name}`, kind: 'file' }));
  const listing = run('list_files', { path: '/home/u/Documents' }, { ok: true, type: 'listing', path: '/home/u/Documents', entries, truncated: false });
  const { ids } = await loadPanel(sequence(listing), memoryStorage());
  await ask(ids);
  const files = () => ids['message-list'].all((n) => n.className === 'message-file').map((li) => li.children[0]);
  assert.strictEqual(files().length, 20);
  assert.ok(files().every((n) => n.tagName === 'bdi'));
  assert.deepStrictEqual(files().map((n) => n.textContent), names.slice(0, 20));
  assert.ok(files().every((n) => n.dir === 'auto'), 'each name is isolated and takes its own direction');
  assert.ok(ids['message-list'].all((n) => n.tagName === 'ul').length === 1);
  assert.match(shown(ids), /10 more not shown\./);
  assert.ok(shown(ids).includes('<img src=x onerror=alert(1)>.txt'));
  assert.deepStrictEqual(unknownTags(ids), []);

  change(ids['setting-digits'], 'eastern');
  assert.deepStrictEqual(files().map((n) => n.textContent), names.slice(0, 20), 'names keep their digits');
  assert.match(shown(ids), /١٠ more not shown\./);
  change(ids.language, 'ar');
  assert.deepStrictEqual(files().map((n) => n.textContent), names.slice(0, 20));
});

test('a search shows names with their folders and the notes about what is missing', async () => {
  const search = run('search_files', { query: 'report' }, {
    ok: true, type: 'search', truncated: true, unreadable: 3,
    matches: [{ path: '/home/u/Downloads/report.pdf', kind: 'file' }, { path: '/home/u/Documents/تقارير', kind: 'dir' }],
  });
  const { ids } = await loadPanel(sequence(search));
  await ask(ids);
  const text = shown(ids);
  assert.match(text, /Files matching “report”:/);
  assert.ok(text.includes('report.pdf') && text.includes('~/Downloads') && text.includes('تقارير') && text.includes('~/Documents'));
  assert.ok(!text.includes('/home/u/'));
  const folders = ids['message-list'].all((n) => n.className === 'message-file-folder');
  assert.deepStrictEqual(folders.map((n) => [n.tagName, n.dir]), [['bdi', 'ltr'], ['bdi', 'ltr']]);
  assert.match(text, /folder/);
  assert.match(text, /There may be more than this/);
  assert.match(text, /Folders that could not be read: 3\./);
});

test('each file name is its own isolated unit on its own line, with its folder and kind after it', async () => {
  const names = ['ميزانية 2026.txt', 'budget final 2026.txt', 'Q3 تقرير.pdf', 'ملف', '.حخ', 'ميزانية.tar.gz', 'ميزانية.'];
  const matches = names.map((name) => ({ path: `/home/u/المستندات/${name}`, kind: 'file' }));
  const { ids } = await loadPanel(sequence(run('search_files', { query: 'x' }, { ok: true, type: 'search', matches, truncated: false, unreadable: 0 })));
  await ask(ids);
  const items = ids['message-list'].all((n) => n.className === 'message-file');
  assert.strictEqual(items.length, names.length);
  items.forEach((li, i) => {
    const [name, folder] = li.children;
    assert.deepStrictEqual([name.tagName, name.className, name.dir, name.textContent], ['bdi', 'message-file-name', 'auto', names[i]]);
    assert.deepStrictEqual([folder.tagName, folder.className, folder.dir], ['bdi', 'message-file-folder', 'ltr']);
    assert.ok(!folder.textContent.includes(names[i]), 'the folder is a separate element from the name');
  });
  const extensions = items.map((li) => li.children[0].children.filter((c) => c.tagName === 'bdi').map((c) => [c.dir, c.textContent]));
  assert.deepStrictEqual(extensions, [[['ltr', '.txt']], [], [['ltr', '.pdf']], [], [], [['ltr', '.gz']], []], 'only a Latin extension of an Arabic name is split off');
  const css = fs.readFileSync(path.join(__dirname, 'panel.css'), 'utf8');
  assert.match(css, /\.message-file-name\s*\{[^}]*display:\s*block/, 'the name takes its own line');
});

test('a language change from the core goes through the panel\'s language switch', async () => {
  const storage = memoryStorage();
  const { ids, posts, document } = await loadPanel(sequence(
    run('set_language', { lang: 'ar' }, { ok: true, type: 'language', lang: 'ar' }),
    { kind: 'declined' },
    run('set_language', { lang: 'xx' }, { ok: true, type: 'language', lang: 'xx' }),
    run('set_language', { lang: 'en' }, { ok: false, error: { code: 'unsupported', message: 'x' } }),
  ), storage);
  await ask(ids, 'switch to Arabic');
  assert.strictEqual(ids.language.value, 'ar');
  assert.strictEqual(document.documentElement.dir, 'rtl');
  assert.strictEqual(JSON.parse(storage.data.get('scarfeeos.panel.settings')).language, 'ar');
  assert.match(shown(ids), /أصبحت لغة النظام الآن العربية\./);
  await ask(ids, 'next');
  assert.strictEqual(posts[1].language, 'ar');

  await ask(ids, 'bogus');
  assert.strictEqual(ids.language.value, 'ar', 'an unknown language is ignored');
  await ask(ids, 'failed');
  assert.strictEqual(ids.language.value, 'ar', 'a failed run changes nothing');
});

test('Confirm sends {confirm: action_id}, shows the result, and the buttons stay disabled', async () => {
  let answer;
  let calls = 0;
  const first = reply({ kind: 'needs_confirmation', tool: 'set_language', arguments: { lang: 'ar' }, action_id: ID });
  const { ids, posts, sendButton } = await loadPanel(async () => {
    calls += 1;
    return calls === 1 ? first : new Promise((resolve) => { answer = resolve; });
  });
  await ask(ids, 'switch to Arabic');
  assert.match(shown(ids), /This changes a setting\. Do you want to go ahead: change the system language to العربية\?/);
  assert.deepStrictEqual(buttons(ids).map((b) => [b.textContent, b.disabled]), [['Confirm', false], ['Cancel', false]]);
  assert.strictEqual(ids['message-list'].all((n) => n.tagName === 'input').length, 0);

  await click(button(ids, 'Confirm'));
  assert.deepStrictEqual(posts[1], { confirm: ID });
  assert.ok(buttons(ids).every((b) => b.disabled), 'disabled while waiting');
  assert.strictEqual(sendButton.disabled, true);
  assert.match(shown(ids), /Working on it/);

  answer(reply(run('set_language', { lang: 'ar' }, { ok: true, type: 'language', lang: 'ar' })));
  await tick();
  await tick();
  assert.strictEqual(ids.language.value, 'ar');
  assert.match(shown(ids), /أصبحت لغة النظام الآن العربية\./);
  assert.deepStrictEqual(buttons(ids).map((b) => [b.textContent, b.disabled]), [['تأكيد', true], ['إلغاء', true]], 'still disabled, now in Arabic');
  assert.strictEqual(sendButton.disabled, false);

  await click(button(ids, 'تأكيد'));
  await click(button(ids, 'إلغاء'));
  assert.strictEqual(posts.length, 2, 'a used button sends nothing again');
});

test('Cancel sends {cancel: action_id} and shows that it was cancelled', async () => {
  const { ids, posts } = await loadPanel(sequence(
    { kind: 'needs_confirmation', tool: 'set_language', arguments: { lang: 'ar' }, action_id: ID },
    { kind: 'cancelled' },
  ));
  await ask(ids);
  await click(button(ids, 'Cancel'));
  assert.deepStrictEqual(posts[1], { cancel: ID });
  assert.match(shown(ids), /Cancelled\. Nothing was done\./);
  assert.ok(buttons(ids).every((b) => b.disabled));
  assert.strictEqual(ids.language.value, 'en');
});

test('a clarification asks for a time, and Confirm sends the chosen one', async () => {
  const held = { kind: 'needs_clarification', tool: 'create_reminder', arguments: { text: 'call', time: '2026-10-30T11:00:00', repeat: null }, action_id: ID };
  const event = (start) => run('create_reminder', held.arguments, { ok: true, type: 'event', event: { uid: 'u', summary: 'call', start, attendees: [], repeat: null } });
  const { ids, posts } = await loadPanel(sequence(held, event('2026-10-30T15:30:00')));
  await ask(ids, 'remind me at 11');
  const [time] = ids['message-list'].all((n) => n.tagName === 'input');
  assert.strictEqual(time.type, 'time');
  assert.strictEqual(time.value, '11:00');
  assert.ok(time.attributes['aria-label']);
  time.value = '15:30';
  time.listeners.input();
  change(ids['setting-digits'], 'western');
  assert.strictEqual(ids['message-list'].all((n) => n.tagName === 'input')[0].value, '15:30', 'the choice survives a redraw');

  await click(button(ids, 'Confirm'));
  assert.deepStrictEqual(posts[1], { confirm: ID, time: '2026-10-30T15:30' });
  assert.match(shown(ids), /Event created: “call” at 2026-10-30 15:30\./);
  const [after] = ids['message-list'].all((n) => n.tagName === 'input');
  assert.deepStrictEqual([after.disabled, after.value], [true, '15:30']);
  assert.ok(buttons(ids).every((b) => b.disabled));
});

test('Confirm on a clarification without a change sends the guessed time, and Cancel sends no time', async () => {
  const held = { kind: 'needs_clarification', tool: 'create_reminder', arguments: { text: 'call', time: '2026-10-30T11:00:00', repeat: null }, action_id: ID };
  const kept = await loadPanel(sequence(held, { kind: 'error', message: 'unknown or already used action id' }));
  await ask(kept.ids);
  await click(button(kept.ids, 'Confirm'));
  assert.deepStrictEqual(kept.posts[1], { confirm: ID, time: '2026-10-30T11:00' });
  assert.match(shown(kept.ids), /Something went wrong: unknown or already used action id/);

  const cancelled = await loadPanel(sequence(held, { kind: 'cancelled' }));
  await ask(cancelled.ids);
  await click(button(cancelled.ids, 'Cancel'));
  assert.deepStrictEqual(cancelled.posts[1], { cancel: ID });
  assert.ok(cancelled.ids['message-list'].all((n) => n.tagName === 'input').every((n) => n.disabled));
});

test('buttons are off while a message is being answered, and a held call without an id has no buttons', async () => {
  let answer;
  let calls = 0;
  const { ids, posts } = await loadPanel(async () => {
    calls += 1;
    if (calls === 1) return reply({ kind: 'needs_confirmation', tool: 'set_language', arguments: { lang: 'ar' }, action_id: ID });
    return new Promise((resolve) => { answer = resolve; });
  });
  await ask(ids);
  ids['message-input'].value = 'another question';
  submit(ids);
  assert.ok(buttons(ids).every((b) => b.disabled));
  await click(button(ids, 'Confirm'));
  assert.strictEqual(posts.length, 2, 'no confirm while another request is running');
  answer(reply({ kind: 'declined' }));
  await tick();
  await tick();
  assert.ok(buttons(ids).every((b) => !b.disabled), 'the held call is still open');

  const none = await loadPanel(sequence({ kind: 'needs_confirmation', tool: 'set_language', arguments: { lang: 'ar' } }));
  await ask(none.ids);
  assert.strictEqual(buttons(none.ids).length, 0);
});

const SESSION_KEY = 'scarfeeos.panel.conversations';

test('the conversation comes back after a reload, with the same text and direction, and is saved as the user goes', async () => {
  const session = memoryStorage();
  const first = await loadPanel(sequence(
    { kind: 'reply', text: '<b>hi</b> <img src=x onerror=alert(1)>' },
    { kind: 'time', date: '2026-10-30', hour: 9, minute: 5, weekday: 'Friday' },
  ), undefined, session);
  await ask(first.ids, '<i>مرحبا</i> see /tmp/a.txt');
  await ask(first.ids, 'time?');
  const before = shown(first.ids);
  assert.ok(session.data.has(SESSION_KEY));

  const second = await loadPanel(async () => assert.fail('a restore sends nothing'), undefined, session);
  assert.strictEqual(shown(second.ids), before);
  const roles = (ids) => ids['message-list'].children.map((li) => li.className);
  assert.deepStrictEqual(roles(second.ids), roles(first.ids));
  assert.deepStrictEqual(roles(second.ids), ['message message-user', 'message message-assistant', 'message message-user', 'message message-assistant']);
  assert.strictEqual(second.posts.length, 0);
  const tags = new Set(second.ids['message-list'].all(() => true).map((n) => n.tagName));
  assert.deepStrictEqual([...tags].filter((tag) => !['message-list', 'li', 'div', 'span', 'p', 'bdi', '#text'].includes(tag)), [], 'stored text is only ever a text node');
  assert.strictEqual(second.ids['conversation-list'].children.length, 1);
  assert.strictEqual(second.sendButton.disabled, false);
});

test('a restored conversation is drawn again with the current settings and language', async () => {
  const session = memoryStorage();
  const first = await loadPanel(async () => TIME_ANSWER, undefined, session);
  await ask(first.ids, 'time?');
  const second = await loadPanel(async () => TIME_ANSWER, undefined, session);
  change(second.ids['setting-digits'], 'eastern');
  assert.match(shown(second.ids), /٠٩:٠٥, on Thursday ٢٠٢٦-٠١-٠١\./);
  change(second.ids.language, 'ar');
  assert.match(shown(second.ids), /time\?/);
});

test('a Confirm or Cancel that was waiting is not clickable after a reload', async () => {
  const session = memoryStorage();
  const first = await loadPanel(sequence(
    { kind: 'needs_confirmation', tool: 'set_language', arguments: { lang: 'ar' }, action_id: ID },
    { kind: 'needs_clarification', tool: 'schedule_meeting', arguments: { title: 'x', time: '2026-11-01T14:00' }, action_id: ID },
  ), undefined, session);
  await ask(first.ids, 'one');
  await ask(first.ids, 'two');
  assert.ok(buttons(first.ids).some((b) => !b.disabled), 'live before the reload');

  const second = await loadPanel(async () => assert.fail('nothing may be sent'), undefined, session);
  assert.strictEqual(buttons(second.ids).length, 4, 'the question is still shown');
  assert.ok(buttons(second.ids).every((b) => b.disabled));
  assert.ok(second.ids['message-list'].all((n) => n.tagName === 'input').every((n) => n.disabled));
  assert.match(shown(second.ids), /Do you want to go ahead/);
  for (const b of buttons(second.ids)) {
    if (b.listeners.click) await click(b);
  }
  assert.strictEqual(second.posts.length, 0);

  const third = await loadPanel(async () => assert.fail('nothing may be sent'), undefined, session);
  assert.ok(buttons(third.ids).every((b) => b.disabled), 'still off after a second reload');
});

test('a reply still waiting is not stored, and a restored conversation can go on', async () => {
  const session = memoryStorage();
  let finish;
  const first = await loadPanel(() => new Promise((resolve) => { finish = resolve; }), undefined, session);
  await ask(first.ids, 'slow question');
  const stored = JSON.parse(session.data.get(SESSION_KEY));
  assert.deepStrictEqual(stored.conversations[0].messages, [{ role: 'user', text: 'slow question' }]);

  const second = await loadPanel(sequence({ kind: 'declined' }), undefined, session);
  assert.match(shown(second.ids), /slow question/);
  assert.doesNotMatch(shown(second.ids), /Working on it/);
  await ask(second.ids, 'again');
  assert.deepStrictEqual(second.posts, [{ text: 'again', language: 'en' }]);
  const third = await loadPanel(async () => TIME_ANSWER, undefined, session);
  assert.match(shown(third.ids), /slow question[^]*again[^]*does not answer religious questions/);
  finish(reply({ kind: 'declined' }));
  await tick();
  await tick();
});

test('a question whose reply was lost when the page was left says so, in the current language, and is never resent', async () => {
  const session = memoryStorage();
  let finish;
  const first = await loadPanel(() => new Promise((resolve) => { finish = resolve; }), undefined, session);
  await ask(first.ids, 'slow question');
  assert.doesNotMatch(shown(first.ids), /interrupted/, 'nothing is wrong while the reply is still coming');
  const savedBefore = session.data.get(SESSION_KEY);

  const second = await loadPanel(sequence({ kind: 'declined' }), undefined, session);
  assert.match(shown(second.ids), /slow question[^]*The reply was interrupted\. Ask again to get an answer\./);
  assert.strictEqual(second.posts.length, 0, 'nothing is resent');
  assert.strictEqual(second.sendButton.disabled, false);
  assert.strictEqual(session.data.get(SESSION_KEY), savedBefore, 'the stored format has no new field');
  const note = second.ids['message-list'].all((n) => n.className === 'message-text message-note');
  assert.strictEqual(note.length, 1);
  assert.strictEqual(second.ids['message-list'].children[0].children.includes(note[0]), true, 'the note is inside the question');
  assert.strictEqual(second.ids['message-list'].all((n) => n.attributes['aria-busy'] === 'true').length, 0);

  change(second.ids.language, 'ar');
  assert.match(shown(second.ids), /انقطع الرد\. اسأل مرة أخرى للحصول على إجابة\./);
  assert.doesNotMatch(shown(second.ids), /The reply was interrupted/);
  change(second.ids.language, 'en');

  await ask(second.ids, 'again');
  assert.deepStrictEqual(second.posts, [{ text: 'again', language: 'en' }]);
  assert.strictEqual((shown(second.ids).match(/The reply was interrupted/g) || []).length, 1, 'only the lost question carries the note');
  const third = await loadPanel(async () => assert.fail('nothing is sent'), undefined, session);
  assert.match(shown(third.ids), /slow question[^]*The reply was interrupted[^]*again[^]*does not answer religious/);
  assert.strictEqual((shown(third.ids).match(/The reply was interrupted/g) || []).length, 1);
  finish(reply({ kind: 'declined' }));
  await tick();
  await tick();
});

test('conversations stored before the note, and answered ones, show no note', async () => {
  const stored = JSON.stringify({ version: 1, active: 1, conversations: [{ id: 1, messages: [
    { role: 'user', text: 'old question' },
    { role: 'assistant', response: { kind: 'declined' } },
    { role: 'user', text: 'old time' },
    { role: 'assistant', response: { kind: 'unreachable' } },
  ] }] });
  const { ids } = await loadPanel(async () => assert.fail('nothing is sent'), undefined, memoryStorage({ [SESSION_KEY]: stored }));
  assert.match(shown(ids), /old question[^]*old time/);
  assert.doesNotMatch(shown(ids), /interrupted/);
});

test('conversations come back with their order, the active one, and new ones get new ids', async () => {
  const session = memoryStorage();
  const first = await loadPanel(sequence({ kind: 'declined' }, { kind: 'declined' }), undefined, session);
  await ask(first.ids, 'first topic');
  first.ids['new-conversation'].listeners.click();
  await ask(first.ids, 'second topic');
  first.ids['conversation-list'].children[1].children[0].listeners.click();

  const second = await loadPanel(async () => assert.fail('nothing is sent'), undefined, session);
  const titles = second.ids['conversation-list'].children.map((li) => li.children[0].textContent);
  assert.deepStrictEqual(titles, ['second topic', 'first topic']);
  assert.match(shown(second.ids), /first topic/);
  assert.doesNotMatch(shown(second.ids), /second topic/);
  second.ids['new-conversation'].listeners.click();
  const ids = JSON.parse(session.data.get(SESSION_KEY)).conversations.map((c) => c.id);
  assert.strictEqual(new Set(ids).size, ids.length);
  second.ids['conversation-list'].children[0].children[0].listeners.click();
  assert.strictEqual(second.ids['conversation-list'].children.length, 3);
  const all = JSON.parse(session.data.get(SESSION_KEY)).conversations.map((c) => c.id);
  assert.strictEqual(new Set(all).size, all.length);
});

test('storage that is damaged, edited, missing, blocked or full leaves a working panel', async () => {
  for (const bad of ['', 'not json', 'null', '[]', '{"version":2}', '{"version":1,"conversations":5}',
    JSON.stringify({ version: 1, active: 1, conversations: [{ id: 'x', messages: [{ role: 'user', text: 'a' }] }, { id: 1, messages: [{ role: 'user', text: 5 }, { role: 'assistant', response: 'str' }, null] }] })]) {
    const { ids } = await loadPanel(sequence({ kind: 'declined' }), undefined, memoryStorage({ [SESSION_KEY]: bad }));
    assert.match(shown(ids), /How can I help you\?/, JSON.stringify(bad));
    await ask(ids, 'works');
    assert.match(shown(ids), /does not answer religious/);
  }
  for (const session of ['throws', undefined]) {
    const { ids } = await loadPanel(sequence({ kind: 'declined' }), undefined, session);
    await ask(ids, 'works');
    assert.match(shown(ids), /does not answer religious/);
  }
  const throwing = { getItem: () => { throw new Error('denied'); }, setItem: () => { throw new Error('QuotaExceededError'); } };
  const { ids } = await loadPanel(sequence({ kind: 'declined' }), undefined, throwing);
  await ask(ids, 'works');
  assert.match(shown(ids), /does not answer religious/);

  const full = memoryStorage({ [SESSION_KEY]: JSON.stringify({ version: 1, active: 1, conversations: [{ id: 1, messages: [{ role: 'user', text: 'old' }] }] }) });
  full.setItem = () => { throw new Error('QuotaExceededError'); };
  full.removeItem = (k) => full.data.delete(k);
  const afterFull = await loadPanel(sequence({ kind: 'declined' }), undefined, full);
  await ask(afterFull.ids, 'new');
  assert.ok(!full.data.has(SESSION_KEY), 'a save that fails does not leave the older conversation behind');
});

test('only the newest messages are stored', async () => {
  const session = memoryStorage();
  const { ids } = await loadPanel(async () => reply({ kind: 'declined' }), undefined, session);
  for (let i = 0; i < 110; i += 1) {
    await ask(ids, `question ${i}`);
  }
  const stored = JSON.parse(session.data.get(SESSION_KEY)).conversations[0].messages;
  assert.strictEqual(stored.length, 200);
  assert.strictEqual(stored[stored.length - 2].text, 'question 109');
  assert.strictEqual(stored[0].text, 'question 10');
});

test('index.html links to the meetings and reminders screen with the calendar-title text', () => {
  const html = fs.readFileSync(path.join(__dirname, 'index.html'), 'utf8');
  assert.match(html, /<a [^>]*href="calendar\.html"[^>]*data-i18n="calendar-title"/);
});
