'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const Installer = require('./installer.js');
const { parseFtl, createI18n } = require('../ui/i18n.js');

const read = (...parts) => fs.readFileSync(path.join(__dirname, ...parts), 'utf8');
const locale = (lang) => parseFtl(read('..', '..', 'locales', lang, 'scarfeeos.ftl'));
const isolates = (text) => text.replace(/[\u2068\u2069]/g, '');
const html = read('index.html');
const source = read('installer.js');

// The real answer of `scarfeeos-core serve` to {"id":1,"hardware":true} on a laptop with a 6 GB GPU.
const REAL_RESPONSE = {
  cpu: { avx2: true, avx512f: false },
  gpu: { memory_mib: 6144, name: 'NVIDIA GeForce RTX 3050 6GB Laptop GPU' },
  kind: 'hardware',
  plan: { command: { measured: true, memory_mib: 5939, name: 'Qwen3-8B' }, gpu_layers: 999, tier: 'full', writer: null },
  plans: {
    dual: {
      command: { measured: true, memory_mib: 3789, name: 'Qwen3-4B' },
      gpu_layers: 999,
      tier: 'dual',
      writer: { measured: true, memory_mib: 4096, name: 'Jais-3-3.7B' },
    },
    full: { command: { measured: true, memory_mib: 5939, name: 'Qwen3-8B' }, gpu_layers: 999, tier: 'full', writer: null },
    lite: { command: { measured: false, memory_mib: 3789, name: 'Qwen3-4B' }, gpu_layers: 999, tier: 'lite', writer: null },
  },
  ram_kib: 7944996,
  recommended: { reason: 'NVIDIA GeForce RTX 3050 6GB Laptop GPU has 6144 MiB of memory (at least 6016 MiB needed)', tier: 'full' },
};
const NO_GPU_RESPONSE = {
  ...REAL_RESPONSE,
  gpu: null,
  ram_kib: null,
  recommended: { reason: 'no NVIDIA GPU detected', tier: 'lite' },
  plan: { ...REAL_RESPONSE.plans.lite, gpu_layers: 0 },
  plans: { ...REAL_RESPONSE.plans, lite: { ...REAL_RESPONSE.plans.lite, gpu_layers: 0 } },
};

// A very small DOM: enough for installer.js, and it refuses anything that would parse text as markup.
class FakeNode {
  constructor(tag, doc) {
    this.doc = doc;
    this.tagName = tag;
    this.children = [];
    this.attributes = {};
    this.listeners = {};
    this.className = '';
    this.disabled = false;
    this.checked = false;
    this.ownText = '';
    this.focused = false;
  }

  get textContent() { return this.ownText + this.children.map((c) => c.textContent).join(''); }
  set textContent(value) { this.ownText = String(value); this.children = []; }
  set innerHTML(value) { throw new Error(`innerHTML is not allowed: ${value}`); }
  set outerHTML(value) { throw new Error(`outerHTML is not allowed: ${value}`); }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) {
    // A focused control that is removed loses focus, as in a browser.
    if (this.doc.activeElement && this.all().includes(this.doc.activeElement)) this.doc.activeElement = null;
    this.ownText = '';
    this.children = nodes;
  }
  setAttribute(name, value) { this.attributes[name] = String(value); }
  getAttribute(name) { return this.attributes[name] ?? null; }
  addEventListener(type, fn) { (this.listeners[type] ||= []).push(fn); }
  fire(type, extra = {}) { for (const fn of this.listeners[type] || []) fn({ type, ...extra }); }
  focus() { this.focused = true; this.doc.activeElement = this; }
  contains(node) { return this.all().includes(node); }
  all() { return [this, ...this.children.flatMap((c) => c.all())]; }
  find(predicate) { return this.all().filter(predicate); }
  byClass(name) { return this.find((n) => n.className.split(/\s+/).includes(name)); }
  byTag(name) { return this.find((n) => n.tagName === name); }

  // Only the selectors installer.js uses: `h1` and `[data-focus="key"]`.
  querySelector(selector) {
    if (selector === 'h1') return this.byTag('h1')[0] || null;
    const [, key] = /^\[data-focus="([a-z-]+)"\]$/.exec(selector) || [];
    assert.ok(key, `unsupported selector ${selector}`);
    return this.find((n) => n.getAttribute('data-focus') === key)[0] || null;
  }
}

// The part of window.history that installer.js uses: a list of entries, an index, and a popstate on back and forward.
function fakeHistory() {
  const entries = [{ installerStep: 'language' }];
  const h = {
    index: 0,
    entries,
    onPop: null,
    get state() { return entries[h.index]; },
    pushState(state) { entries.splice(h.index + 1, entries.length, state); h.index += 1; },
    replaceState(state) { entries[h.index] = state; },
    back() { h.go(-1); },
    go(delta) {
      const to = h.index + delta;
      if (to < 0 || to >= entries.length) return;
      h.index = to;
      h.onPop({ state: entries[to] });
    },
  };
  return h;
}

function setup(response, { withHistory = true } = {}) {
  const document = { title: '', activeElement: null, createElement: (tag) => new FakeNode(tag, document) };
  const container = new FakeNode('main', document);
  const history = fakeHistory();
  const root = { lang: '', dir: '' };
  const catalogs = { en: locale('en'), ar: locale('ar') };
  const i18n = createI18n(catalogs, root, 'en');
  const asked = [];
  const saved = [];
  const installer = Installer.createInstaller({
    document,
    container,
    i18n,
    catalogs,
    history: withHistory ? history : null,
    ask(body) {
      asked.push(body);
      return Promise.resolve(typeof response === 'function' ? response(body) : response);
    },
    save(choices) {
      saved.push(choices);
      return Installer.saveChoicesToJsonFile(choices);
    },
  });
  history.onPop = installer.popState;
  installer.render();
  const radio = (name, value) => container.byTag('input').find((n) => n.name === name && n.value === value);
  const buttons = () => container.byTag('button');
  const forward = () => container.byClass('nav-forward')[0];
  const flush = () => new Promise((resolve) => setImmediate(resolve));
  return { installer, container, root, document, history, asked, saved, radio, buttons, forward, flush, catalogs };
}

async function toTierScreen(response, language = 'en') {
  const page = setup(response);
  page.radio('language', language).fire('change');
  page.forward().fire('click');
  await page.flush();
  return page;
}

test('the prompt is one constant and matches docs/installer.md', () => {
  assert.strictEqual(Installer.LANGUAGE_PROMPT, 'Select your language / اختر لغتك');
  assert.ok(read('..', '..', 'docs', 'installer.md').includes(Installer.LANGUAGE_PROMPT));
  assert.strictEqual(source.split(Installer.LANGUAGE_PROMPT).length - 1, 1);
});

test('screen 1 is bilingual and Continue cannot proceed before a choice', () => {
  const { container, document, forward, installer, asked, root } = setup(REAL_RESPONSE);
  assert.strictEqual(document.title, Installer.LANGUAGE_PROMPT);
  assert.strictEqual(container.byTag('h1')[0].textContent, Installer.LANGUAGE_PROMPT);
  assert.deepStrictEqual(container.byClass('choice-name').map((n) => n.textContent), ['English', 'العربية']);
  assert.deepStrictEqual(container.byClass('choice-name').map((n) => n.dir), ['ltr', 'rtl']);
  assert.strictEqual(forward().textContent, 'Continue / متابعة');
  assert.strictEqual(forward().getAttribute('aria-disabled'), 'true');
  assert.strictEqual(installer.next(), false);
  assert.strictEqual(installer.state.step, 'language');
  assert.deepStrictEqual(asked, []);
  assert.strictEqual(root.dir, '');
});

test('choosing a language switches the whole page to it, RTL for Arabic', () => {
  const { container, document, forward, radio, root } = setup(REAL_RESPONSE);
  radio('language', 'ar').focus();
  radio('language', 'ar').fire('change');
  assert.strictEqual(root.lang, 'ar');
  assert.strictEqual(root.dir, 'rtl');
  assert.strictEqual(document.title, 'اختر لغتك');
  assert.strictEqual(forward().textContent, 'متابعة');
  assert.strictEqual(forward().getAttribute('aria-disabled'), null);
  assert.strictEqual(radio('language', 'ar').checked, true);
  assert.strictEqual(radio('language', 'en').checked, false);
  assert.strictEqual(radio('language', 'ar').focused, true);
  assert.deepStrictEqual(container.byClass('choice-name').map((n) => n.textContent), ['English', 'العربية']);
  radio('language', 'en').fire('change');
  assert.strictEqual(root.lang, 'en');
  assert.strictEqual(root.dir, 'ltr');
  assert.strictEqual(forward().textContent, 'Continue');
});

test('only English and Arabic can be chosen', () => {
  const { installer } = setup(REAL_RESPONSE);
  assert.strictEqual(installer.chooseLanguage('fr'), false);
  assert.strictEqual(installer.chooseLanguage('constructor'), false);
  assert.strictEqual(installer.state.language, null);
  assert.strictEqual(installer.chooseTier('huge'), false);
});

test('screen 2 asks the core for the hardware and highlights the recommended tier', async () => {
  const page = await toTierScreen(REAL_RESPONSE);
  const { container, asked, radio, forward, installer } = page;
  assert.deepStrictEqual(asked, [{ hardware: true }]);
  assert.strictEqual(installer.state.step, 'tier');
  assert.strictEqual(container.byTag('h1')[0].textContent, 'Choose the AI tier');
  const text = isolates(container.textContent);
  assert.match(text, /Memory \(RAM\): 7\.6 GB/);
  assert.match(text, /AVX2: Yes/);
  assert.match(text, /AVX-512: No/);
  assert.match(text, /Graphics card: NVIDIA GeForce RTX 3050 6GB Laptop GPU, 6 GB/);
  assert.deepStrictEqual(container.byTag('input').filter((n) => n.name === 'tier').map((n) => [n.value, n.checked, n.disabled]), [
    ['lite', false, false],
    ['full', true, false],
    ['dual', false, false],
  ]);
  const recommended = container.byClass('tier-recommended');
  assert.strictEqual(recommended.length, 1);
  assert.strictEqual(radio('tier', 'full').checked, true);
  assert.match(isolates(recommended[0].textContent), /^Full.*Recommended/);
  assert.match(isolates(container.byClass('tier')[2].textContent), /Models: Qwen3-4B, Jais-3-3.7B/);
  assert.match(isolates(recommended[0].textContent), /Models: Qwen3-8B$/);
  assert.strictEqual(container.byClass('notice-warning').length, 0);
  assert.strictEqual(forward().getAttribute('aria-disabled'), null);
});

test('choosing a tier above the hardware warns but does not block', async () => {
  const { container, radio, forward, installer } = await toTierScreen(REAL_RESPONSE);
  radio('tier', 'dual').fire('change');
  const warning = container.byClass('notice-warning');
  assert.strictEqual(warning.length, 1);
  assert.strictEqual(isolates(warning[0].textContent), 'Dual needs more than this computer has. You can still choose it, but the assistant may run slowly or fail to load its models.');
  assert.strictEqual(warning[0].getAttribute('role'), 'alert');
  assert.strictEqual(forward().getAttribute('aria-disabled'), null);
  assert.strictEqual(installer.next(), true);
  assert.strictEqual(installer.state.step, 'disk');
  assert.deepStrictEqual(installer.state.saveResult.saved, false);
});

test('no warning at or below the recommended tier', async () => {
  const { container, radio } = await toTierScreen(REAL_RESPONSE);
  radio('tier', 'lite').fire('change');
  assert.strictEqual(container.byClass('notice-warning').length, 0);
  radio('tier', 'full').fire('change');
  assert.strictEqual(container.byClass('notice-warning').length, 0);
});

test('with no GPU the recommendation is Lite and the other tiers still can be chosen', async () => {
  const { container, radio } = await toTierScreen(NO_GPU_RESPONSE);
  assert.strictEqual(radio('tier', 'lite').checked, true);
  assert.match(isolates(container.textContent), /Memory \(RAM\): could not be read/);
  assert.match(isolates(container.textContent), /Graphics card: none found/);
  radio('tier', 'full').fire('change');
  assert.strictEqual(container.byClass('notice-warning').length, 1);
  radio('tier', 'dual').fire('change');
  assert.strictEqual(container.byClass('notice-warning').length, 1);
  assert.strictEqual(radio('tier', 'dual').checked, true);
});

// The core's `gpus` list: one discrete card, one integrated GPU and one GPU found but not usable for AI.
const GPU_LIST = [
  { vendor: 'nvidia', name: 'NVIDIA GeForce RTX 3060', kind: 'discrete', memory_mib: 12288, llama_cpp_usable: true },
  { vendor: 'amd', name: 'AMD GPU 1002:1638', kind: 'integrated', memory_mib: null, llama_cpp_usable: true },
  { vendor: 'intel', name: 'Intel GPU 8086:46A6', kind: 'unknown', memory_mib: null, llama_cpp_usable: false },
];
const lines = (page) => page.container.byTag('li').map((n) => isolates(n.textContent));

test('every GPU the core found is listed with its kind and memory, and one that cannot be used for AI says so', async () => {
  const page = await toTierScreen({ ...REAL_RESPONSE, gpus: GPU_LIST });
  assert.deepStrictEqual(lines(page).slice(3), [
    'Graphics card (dedicated): NVIDIA GeForce RTX 3060, 12 GB',
    'Integrated graphics: AMD GPU 1002:1638',
    'Graphics card (type unknown): Intel GPU 8086:46A6. Found, but it cannot be used for AI.',
  ]);
  assert.strictEqual(page.installer.state.step, 'tier');
  assert.strictEqual(page.container.byClass('tier-recommended').length, 1, 'the recommendation is still the core\'s');
});

test('the GPU list is in Arabic after choosing Arabic, with the integrated GPU named as such and wrapped in isolates', async () => {
  const unusable = { vendor: 'amd', name: 'AMD GPU 1002:1638', kind: 'integrated', memory_mib: null, llama_cpp_usable: false };
  const page = await toTierScreen({ ...REAL_RESPONSE, gpus: [GPU_LIST[0], unusable, GPU_LIST[2]] }, 'ar');
  assert.deepStrictEqual(lines(page).slice(3), [
    'بطاقة الرسوميات المنفصلة: NVIDIA GeForce RTX 3060، 12 غيغابايت',
    'معالج الرسوميات المدمج: AMD GPU 1002:1638. عُثر عليه، لكن لا يمكن استخدامه للذكاء الاصطناعي.',
    'بطاقة الرسوميات (النوع غير معروف): Intel GPU 8086:46A6. عُثر عليها، لكن لا يمكن استخدامها للذكاء الاصطناعي.',
  ]);
  assert.match(page.container.byTag('li')[3].textContent, /⁨NVIDIA GeForce RTX 3060⁩/);
});

test('an empty GPU list says none was found, and a response without `gpus` keeps the single-GPU line', async () => {
  const none = await toTierScreen({ ...NO_GPU_RESPONSE, gpus: [] });
  assert.deepStrictEqual(lines(none).slice(3), ['Graphics card: none found']);
  const old = await toTierScreen(REAL_RESPONSE);
  assert.strictEqual(Installer.readHardware(REAL_RESPONSE).gpus, null);
  assert.deepStrictEqual(lines(old).slice(3), ['Graphics card: NVIDIA GeForce RTX 3050 6GB Laptop GPU, 6 GB']);
});

test('readHardware reads `gpus` leniently: a bad entry is skipped and an unknown kind is shown as unknown', () => {
  const read = Installer.readHardware({
    ...REAL_RESPONSE,
    gpus: [GPU_LIST[0], null, { name: 3 }, { name: 'X', kind: 'quantum', memory_mib: -1 }],
  });
  assert.deepStrictEqual(read.gpus, [
    { name: 'NVIDIA GeForce RTX 3060', kind: 'discrete', memoryMib: 12288, usable: true },
    { name: 'X', kind: 'unknown', memoryMib: null, usable: true },
  ]);
  assert.strictEqual(Installer.readHardware({ ...REAL_RESPONSE, gpus: 'many' }).gpus, null);
});

test('the graphics strings no longer name only NVIDIA, and Arabic says a graphics card except for integrated graphics', () => {
  for (const lang of ['en', 'ar']) {
    for (const [key, value] of locale(lang)) {
      if (key.startsWith('installer-')) assert.doesNotMatch(value, /NVIDIA/, `${lang} ${key}`);
    }
  }
  for (const [key, value] of locale('ar')) {
    if (!key.startsWith('installer-')) continue;
    assert.doesNotMatch(value.replace(/معالج الرسوميات المدمج/g, ''), /معالج رسوميات|معالج الرسوميات/, key);
  }
});

test('screen 2 is in Arabic after choosing Arabic', async () => {
  const { container, radio } = await toTierScreen(REAL_RESPONSE, 'ar');
  assert.strictEqual(container.byTag('h1')[0].textContent, 'اختر مستوى الذكاء الاصطناعي');
  assert.match(isolates(container.textContent), /الذاكرة \(RAM\): 7\.6 غيغابايت/);
  assert.match(isolates(container.byClass('tier-recommended')[0].textContent), /^كامل.*موصى به/);
  radio('tier', 'dual').fire('change');
  assert.match(isolates(container.byClass('notice-warning')[0].textContent), /^يحتاج المستوى ثنائي إلى أكثر/);
  assert.match(isolates(container.textContent), /النماذج: Qwen3-4B، Jais-3-3\.7B/);
});

test('when the hardware cannot be read every tier can still be chosen', async () => {
  for (const response of [{ kind: 'unreachable' }, { kind: 'hardware', recommended: { tier: 'huge' } }, null]) {
    const { container, radio, forward, asked, buttons, flush } = await toTierScreen(response);
    assert.match(container.textContent, /could not be checked/);
    assert.strictEqual(container.byClass('tier-recommended').length, 0);
    assert.strictEqual(container.byClass('badge').length, 0);
    assert.strictEqual(forward().getAttribute('aria-disabled'), 'true');
    radio('tier', 'dual').fire('change');
    assert.strictEqual(container.byClass('notice-warning').length, 1);
    assert.match(container.byClass('notice-warning')[0].textContent, /could not be checked/);
    assert.strictEqual(forward().getAttribute('aria-disabled'), null);
    buttons().find((b) => b.textContent === 'Check again').fire('click');
    await flush();
    assert.strictEqual(asked.length, 2);
  }
});

test('a late answer for an older request is ignored and the user\'s tier is kept', async () => {
  const answers = [];
  const page = setup(() => new Promise((resolve) => answers.push(resolve)));
  page.radio('language', 'en').fire('change');
  page.forward().fire('click');
  await page.flush();
  assert.match(page.container.textContent, /Checking this computer/);
  page.radio('tier', 'lite').fire('change');
  page.installer.loadHardware();
  await page.flush();
  answers[0](REAL_RESPONSE);
  await page.flush();
  assert.match(page.container.textContent, /Checking this computer/);
  answers[1](REAL_RESPONSE);
  await page.flush();
  assert.strictEqual(page.radio('tier', 'lite').checked, true);
  assert.strictEqual(page.container.byClass('tier-recommended').length, 1);
});

test('screen 3 is a placeholder that says it is not built and offers nothing but Back', async () => {
  const { container, forward, installer, saved, buttons, document } = await toTierScreen(REAL_RESPONSE);
  forward().fire('click');
  assert.strictEqual(installer.state.step, 'disk');
  assert.strictEqual(document.title, 'Disk and account setup');
  assert.match(container.byClass('notice-not-built')[0].textContent, /^Not built yet/);
  assert.match(container.textContent, /does not change any disk and does not create any account\. Your choices have not been saved\./);
  assert.deepStrictEqual(buttons().map((b) => b.textContent), ['Back']);
  assert.strictEqual(container.byTag('input').length, 0);
  assert.strictEqual(installer.next(), false);
  assert.deepStrictEqual(saved, [{ language: 'en', tier: 'full' }]);
  buttons()[0].fire('click');
  assert.strictEqual(installer.state.step, 'tier');
});

test('choices are saved only through saveChoicesToJsonFile, which is a stub that writes nothing', () => {
  const stub = Installer.saveChoicesToJsonFile({ language: 'ar', tier: 'lite' });
  assert.strictEqual(stub.saved, false);
  assert.deepStrictEqual(JSON.parse(stub.json), { language: 'ar', tier: 'lite' });
  assert.match(source, /STUB\. Nothing is written yet/);
  assert.doesNotMatch(source, /writeFile|localStorage|sessionStorage|XMLHttpRequest|sendBeacon|indexedDB/);
  assert.deepStrictEqual([...source.matchAll(/\bfetch\(([^)]*)\)/g)].map((m) => m[1].split(',')[0]), ["'/chat'", '`../../locales/${lang}/scarfeeos.ftl`']);
});

test('readHardware accepts the real response and rejects anything else', () => {
  const read = Installer.readHardware(REAL_RESPONSE);
  assert.strictEqual(read.recommended, 'full');
  assert.strictEqual(read.ramKib, 7944996);
  assert.deepStrictEqual(read.gpu, { name: 'NVIDIA GeForce RTX 3050 6GB Laptop GPU', memoryMib: 6144 });
  assert.deepStrictEqual(read.plans.dual.models, ['Qwen3-4B', 'Jais-3-3.7B']);
  assert.deepStrictEqual(read.plans.lite.models, ['Qwen3-4B']);
  assert.strictEqual(Installer.readHardware(NO_GPU_RESPONSE).gpu, null);
  for (const bad of [
    null, 'x', [], {}, { ...REAL_RESPONSE, kind: 'error' }, { ...REAL_RESPONSE, recommended: { tier: 'huge' } },
    { ...REAL_RESPONSE, recommended: null }, { ...REAL_RESPONSE, plans: { lite: REAL_RESPONSE.plans.lite } },
    { ...REAL_RESPONSE, plans: { ...REAL_RESPONSE.plans, dual: { command: {}, writer: null } } },
    { ...REAL_RESPONSE, gpu: { name: 3, memory_mib: 1 } }, { ...REAL_RESPONSE, gpu: undefined },
    { ...REAL_RESPONSE, ram_kib: 'lots' }, { ...REAL_RESPONSE, cpu: null },
  ]) {
    assert.strictEqual(Installer.readHardware(bad), null, JSON.stringify(bad));
  }
});

test('tier order: only a tier above the recommended one exceeds the hardware', () => {
  assert.deepStrictEqual(Installer.TIERS, ['lite', 'full', 'dual']);
  assert.strictEqual(Installer.exceedsHardware('dual', 'full'), true);
  assert.strictEqual(Installer.exceedsHardware('full', 'lite'), true);
  assert.strictEqual(Installer.exceedsHardware('full', 'full'), false);
  assert.strictEqual(Installer.exceedsHardware('lite', 'dual'), false);
});

test('fill isolates values the way Fluent does', () => {
  assert.strictEqual(isolates(Installer.fill('A { $x } B { $y }', { x: 'مرحبا' })), 'A مرحبا B ');
  assert.strictEqual(Installer.fill('{ $x }', { x: 'v' }), '\u2068v\u2069');
});

// Strings

function stringLiterals(js) {
  const code = js.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
  return [...code.matchAll(/'((?:[^'\\\n]|\\.)*)'/g)].map((m) => m[1]);
}

test('every locale key installer.js uses exists in both languages, and none is left unused', () => {
  const used = new Set(stringLiterals(source).filter((s) => /^(installer|language|assistant)-[a-z0-9-]+$/.test(s)));
  assert.ok([...used].filter((k) => k.startsWith('installer-')).length >= 30, 'expected to find the installer keys');
  for (const lang of ['en', 'ar']) {
    const messages = locale(lang);
    for (const key of used) assert.ok(messages.has(key), `${lang} is missing key "${key}"`);
    for (const key of messages.keys()) {
      if (key.startsWith('installer-')) assert.ok(used.has(key), `${lang} defines "${key}" but installer.js never uses it`);
    }
  }
  assert.deepStrictEqual([...locale('en').keys()].sort(), [...locale('ar').keys()].sort());
});

test('placeholders in the installer strings are the same in English and Arabic', () => {
  const names = (text) => [...text.matchAll(/\{\s*\$([A-Za-z0-9_-]+)\s*\}/g)].map((m) => m[1]).sort();
  const en = locale('en');
  const ar = locale('ar');
  for (const key of en.keys()) {
    if (key.startsWith('installer-')) assert.deepStrictEqual(names(ar.get(key)), names(en.get(key)), key);
  }
});

test('installer.js has no hard-coded text except the prompt and the two language names', () => {
  const allowed = new Set([Installer.LANGUAGE_PROMPT, 'English', 'العربية', 'use strict', 'Enter']);
  for (const literal of stringLiterals(source)) {
    if (allowed.has(literal) || !/\p{L}/u.test(literal)) continue;
    assert.doesNotMatch(literal, /[^\x00-\x7f]/, `non-ASCII text: ${literal}`);
    // Lower-case tokens with spaces are CSS class lists; anything else with spaces is text.
    if (!/^\s*[a-z0-9-]+(\s+[a-z0-9-]+)*\s*$/.test(literal)) assert.doesNotMatch(literal, /\s/, `text with spaces: ${literal}`);
    assert.doesNotMatch(literal, /^[A-Z][a-z]+$/, `a word shown to the user?: ${literal}`);
    for (const lang of ['en', 'ar']) assert.ok(![...locale(lang).values()].includes(literal), `${literal} is a locale string`);
  }
  assert.deepStrictEqual(Installer.LANGUAGES.map((l) => [l.code, l.name]), [['en', 'English'], ['ar', 'العربية']]);
  assert.strictEqual(locale('en').get('language-english'), 'English');
  assert.strictEqual(locale('ar').get('language-arabic'), 'العربية');
});

test('index.html has no hard-coded text or plain text attributes', () => {
  const page = html.replace(/<!--[\s\S]*?-->/g, '').replace(/<script[\s\S]*?<\/script>/g, '');
  assert.strictEqual(page.replace(/<[^>]*>/g, '').trim(), '');
  assert.doesNotMatch(page, /\s(placeholder|title|alt|aria-label)=/);
  assert.match(html, /<html lang="en" dir="ltr">/);
});

// Navigation, focus and refusal

const choices = (page) => page.container.byTag('input').filter((n) => n.checked).map((n) => `${n.name}=${n.value}`);

test('Back keeps the choices, forward again does not ask the hardware a second time', async () => {
  const page = await toTierScreen(REAL_RESPONSE, 'ar');
  page.radio('tier', 'dual').fire('change');
  page.buttons().find((b) => b.textContent === 'رجوع').fire('click');
  assert.strictEqual(page.installer.state.step, 'language');
  assert.deepStrictEqual(choices(page), ['language=ar']);
  assert.strictEqual(page.root.dir, 'rtl');
  page.forward().fire('click');
  await page.flush();
  assert.deepStrictEqual(choices(page), ['tier=dual']);
  assert.deepStrictEqual(page.asked, [{ hardware: true }]);
  page.forward().fire('click');
  assert.strictEqual(page.installer.state.step, 'disk');
  page.buttons()[0].fire('click');
  assert.strictEqual(page.installer.state.step, 'tier');
  assert.deepStrictEqual(choices(page), ['tier=dual']);
});

test('changing the language after going back switches the whole page and keeps the tier', async () => {
  const page = await toTierScreen(REAL_RESPONSE, 'en');
  page.radio('tier', 'lite').fire('change');
  page.buttons().find((b) => b.textContent === 'Back').fire('click');
  page.radio('language', 'ar').fire('change');
  assert.strictEqual(page.root.dir, 'rtl');
  page.forward().fire('click');
  await page.flush();
  assert.strictEqual(page.container.byTag('h1')[0].textContent, 'اختر مستوى الذكاء الاصطناعي');
  assert.deepStrictEqual(choices(page), ['tier=lite']);
});

test('a failed hardware check is tried again when the screen is shown again', async () => {
  const page = await toTierScreen({ kind: 'unreachable' });
  page.buttons().find((b) => b.textContent === 'Back').fire('click');
  page.forward().fire('click');
  await page.flush();
  assert.strictEqual(page.asked.length, 2);
});

test('a hardware request that throws or never resolves to a response ends as "could not be checked"', async () => {
  const page = setup(() => { throw new Error('the shell is gone'); });
  page.radio('language', 'en').fire('change');
  page.forward().fire('click');
  await page.flush();
  assert.match(page.container.textContent, /could not be checked/);
  assert.strictEqual(page.installer.state.hardware.phase, 'unavailable');
});

test('Continue refuses without a choice and says why, in both languages on screen 1', async () => {
  const page = setup(REAL_RESPONSE);
  page.forward().fire('click');
  assert.strictEqual(page.installer.state.step, 'language');
  const alert = page.container.byClass('notice-warning');
  assert.strictEqual(alert.length, 1);
  assert.strictEqual(alert[0].getAttribute('role'), 'alert');
  assert.strictEqual(alert[0].textContent, 'Choose a language to continue. / اختر لغة للمتابعة.');
  assert.deepStrictEqual(page.asked, []);
  page.radio('language', 'en').fire('change');
  assert.strictEqual(page.container.byClass('notice-warning').length, 0);
  assert.strictEqual(page.history.entries.length, 1);
});

test('Continue on the tier screen refuses until a tier is chosen, and says why', async () => {
  const page = await toTierScreen({ kind: 'unreachable' }, 'ar');
  page.forward().fire('click');
  assert.strictEqual(page.installer.state.step, 'tier');
  assert.deepStrictEqual(page.container.byClass('notice-warning').map((n) => n.textContent).filter((t) => /مستوى للمتابعة/.test(t)), ['اختر مستوى للمتابعة.']);
  assert.deepStrictEqual(page.saved, []);
  page.radio('tier', 'lite').fire('change');
  assert.ok(!page.container.textContent.includes('للمتابعة.'));
  page.forward().fire('click');
  assert.strictEqual(page.installer.state.step, 'disk');
});

test('a tier that is not one of the three is refused, and nothing is saved for it', async () => {
  const page = await toTierScreen({ kind: 'unreachable' });
  assert.strictEqual(page.installer.chooseTier('huge'), false);
  assert.strictEqual(page.installer.next(), false);
  assert.deepStrictEqual(page.saved, []);
});

test('Enter on a radio continues, or refuses with the reason', async () => {
  const page = setup(REAL_RESPONSE);
  page.radio('language', 'en').fire('keydown', { key: 'Enter' });
  assert.strictEqual(page.installer.state.step, 'language');
  assert.strictEqual(page.container.byClass('notice-warning').length, 1);
  page.radio('language', 'ar').fire('keydown', { key: 'ArrowDown' });
  assert.strictEqual(page.installer.state.step, 'language');
  page.radio('language', 'ar').fire('change');
  page.radio('language', 'ar').fire('keydown', { key: 'Enter' });
  await page.flush();
  assert.strictEqual(page.installer.state.step, 'tier');
  page.radio('tier', 'full').fire('keydown', { key: 'Enter' });
  assert.strictEqual(page.installer.state.step, 'disk');
});

test('focus moves to the heading on every new screen and stays on the control that had it', async () => {
  const page = setup(REAL_RESPONSE);
  page.radio('language', 'ar').focus();
  page.radio('language', 'ar').fire('change');
  assert.strictEqual(page.document.activeElement, page.radio('language', 'ar'));
  page.forward().focus();
  page.forward().fire('click');
  assert.strictEqual(page.document.activeElement, page.container.byTag('h1')[0]);
  assert.strictEqual(page.container.byTag('h1')[0].getAttribute('tabindex'), '-1');
  page.radio('tier', 'lite').focus();
  await page.flush();
  assert.strictEqual(page.document.activeElement, page.radio('tier', 'lite'), 'the hardware answer must not take focus away');
  page.radio('tier', 'dual').focus();
  page.radio('tier', 'dual').fire('change');
  assert.strictEqual(page.document.activeElement, page.radio('tier', 'dual'));
  page.forward().focus();
  page.forward().fire('click');
  assert.strictEqual(page.document.activeElement, page.container.byTag('h1')[0]);
  page.buttons()[0].focus();
  page.buttons()[0].fire('click');
  assert.strictEqual(page.document.activeElement, page.container.byTag('h1')[0]);
});

test('Check again keeps focus on its button, and the refusal message keeps it on Continue', async () => {
  const page = await toTierScreen({ kind: 'unreachable' });
  const retry = () => page.buttons().find((b) => b.textContent === 'Check again');
  retry().focus();
  retry().fire('click');
  assert.strictEqual(retry(), undefined, 'there is nothing to press while checking');
  await page.flush();
  assert.strictEqual(page.document.activeElement, retry());
  page.forward().focus();
  page.forward().fire('click');
  assert.strictEqual(page.document.activeElement, page.forward());
});

test('the browser history follows the screens: Back leaves tier for language, and Forward returns', async () => {
  const page = await toTierScreen(REAL_RESPONSE);
  page.radio('tier', 'dual').fire('change');
  assert.deepStrictEqual(page.history.entries.map((e) => e.installerStep), ['language', 'tier']);
  page.history.go(-1);
  assert.strictEqual(page.installer.state.step, 'language');
  assert.deepStrictEqual(choices(page), ['language=en']);
  page.history.go(1);
  await page.flush();
  assert.strictEqual(page.installer.state.step, 'tier');
  assert.deepStrictEqual(choices(page), ['tier=dual']);
  page.forward().fire('click');
  assert.deepStrictEqual(page.history.entries.map((e) => e.installerStep), ['language', 'tier', 'disk']);
  page.history.go(-2);
  assert.strictEqual(page.installer.state.step, 'language');
});

test('the Back button goes through the browser history, so Back and Forward stay in step', async () => {
  const page = await toTierScreen(REAL_RESPONSE);
  page.buttons().find((b) => b.textContent === 'Back').fire('click');
  assert.strictEqual(page.history.index, 0);
  assert.strictEqual(page.installer.state.step, 'language');
  page.history.go(1);
  assert.strictEqual(page.installer.state.step, 'tier');
});

test('without browser history the Back button still works', async () => {
  const page = setup(REAL_RESPONSE, { withHistory: false });
  page.radio('language', 'en').fire('change');
  page.forward().fire('click');
  await page.flush();
  assert.strictEqual(page.installer.back(), true);
  assert.strictEqual(page.installer.state.step, 'language');
  assert.strictEqual(page.installer.back(), false);
});

test('a history entry for a screen whose choices were not made is not shown', async () => {
  const page = setup(REAL_RESPONSE);
  page.installer.popState({ state: { installerStep: 'disk' } });
  assert.strictEqual(page.installer.state.step, 'language');
  page.installer.popState({ state: { installerStep: 'tier' } });
  assert.strictEqual(page.installer.state.step, 'language');
  page.installer.popState({ state: null });
  assert.strictEqual(page.installer.state.step, 'language');
  assert.deepStrictEqual(page.history.entries, [{ installerStep: 'language' }]);
  assert.deepStrictEqual(page.asked, []);
  page.radio('language', 'en').fire('change');
  page.installer.popState({ state: { installerStep: 'disk' } });
  await page.flush();
  assert.strictEqual(page.installer.state.step, 'tier');
  assert.deepStrictEqual(page.history.entries.map((e) => e.installerStep), ['tier']);
  assert.deepStrictEqual(page.saved, []);
});

// Right to left

test('lists and notes follow the page direction: nothing is laid out by guessing from its first letter', async () => {
  const page = await toTierScreen(REAL_RESPONSE, 'ar');
  assert.ok(page.container.byTag('li').length >= 4);
  for (const node of page.container.all()) assert.ok(!node.dir || node.className === 'choice-name', `${node.tagName} has dir=${node.dir}`);
  assert.doesNotMatch(source, /dir\s*=\s*'auto'|setAttribute\('dir'/);
});

test('Latin names and numbers inside Arabic text are isolated, so they cannot reorder the words around them', async () => {
  const page = await toTierScreen(REAL_RESPONSE, 'ar');
  const gpu = page.container.byTag('li').map((n) => n.textContent).find((t) => /RTX/.test(t));
  assert.match(gpu, /⁨NVIDIA GeForce RTX 3050 6GB Laptop GPU⁩/);
  assert.match(gpu, /⁨⁨6⁩ غيغابايت⁩/);
  assert.match(page.container.byClass('tier-models')[2].textContent, /⁨Qwen3-4B، Jais-3-3\.7B⁩/);
});

// The installer cannot act on a disk

const FORBIDDEN_APIS = /writeFile|localStorage|sessionStorage|indexedDB|XMLHttpRequest|WebSocket|EventSource|sendBeacon|showSaveFilePicker|showDirectoryPicker|createObjectURL|navigator\.|require\(['"]node:(fs|child_process)|\beval\(|new Function|\.download\b|location\s*=|location\.(href|assign|replace)/;

test('every screen, in both languages, with Back and Forward, asks only for the hardware and saves nothing', async () => {
  for (const language of ['en', 'ar']) {
    const page = await toTierScreen(REAL_RESPONSE, language);
    page.radio('tier', 'dual').fire('change');
    page.forward().fire('click');
    page.history.go(-1);
    page.history.go(1);
    page.installer.back();
    page.installer.loadHardware();
    await page.flush();
    page.forward().fire('click');
    assert.ok(page.asked.length >= 2);
    for (const body of page.asked) assert.deepStrictEqual(body, { hardware: true });
    assert.ok(page.saved.length >= 1);
    for (const result of page.saved) assert.deepStrictEqual(result, { language, tier: 'dual' });
    assert.strictEqual(page.installer.state.saveResult.saved, false);
  }
});

test('isHardwareRequest accepts exactly {"hardware": true}', () => {
  assert.strictEqual(Installer.isHardwareRequest({ hardware: true }), true);
  for (const body of [{}, null, [], 'hardware', { hardware: 1 }, { hardware: true, install: true }, { hardware: true, disk: '/dev/sda' },
    { confirm: 'x' }, { delete_event: 'x' }, { text: 'format the disk', language: 'en' }, { install: true }, { format: '/dev/sda' }]) {
    assert.strictEqual(Installer.isHardwareRequest(body), false, JSON.stringify(body));
  }
});

test('installer.js uses no API that could write a file, start a program or leave the page', () => {
  assert.doesNotMatch(source, FORBIDDEN_APIS);
  assert.doesNotMatch(read('index.html'), /<(form|iframe|object|embed|a)\b/);
  assert.match('localStorage.setItem()', FORBIDDEN_APIS);
  assert.match('new XMLHttpRequest()', FORBIDDEN_APIS);
  assert.match('location.href = x', FORBIDDEN_APIS);
});

// The installer's real wiring (start()) in a stand-in browser: what it fetches, and what it does with a disk-like request.

async function runInBrowserStandIn(response) {
  const document = { title: '', activeElement: null, documentElement: { lang: '', dir: '' } };
  document.createElement = (tag) => new FakeNode(tag, document);
  const main = new FakeNode('main', document);
  document.getElementById = (id) => (id === 'installer' ? main : null);
  const fetched = [];
  const history = fakeHistory();
  const listeners = {};
  const window = { history, addEventListener: (type, fn) => { listeners[type] = fn; } };
  history.onPop = (event) => listeners.popstate(event);
  const fetchStandIn = (url, init) => {
    fetched.push({ url: String(url), init });
    const body = String(url).startsWith('/chat')
      ? JSON.stringify(response)
      : fs.readFileSync(path.join(__dirname, String(url).replace(/^\.\.\/\.\.\//, '../../')), 'utf8');
    return Promise.resolve({ status: 200, text: () => Promise.resolve(body) });
  };
  const context = vm.createContext({ console, document, window, fetch: fetchStandIn });
  vm.runInContext(read('..', 'ui', 'i18n.js'), context);
  vm.runInContext(source, context, { filename: 'installer.js' });
  const flush = () => new Promise((resolve) => setImmediate(resolve));
  await flush();
  const radio = (name, value) => main.byTag('input').find((n) => n.name === name && n.value === value);
  return { main, fetched, history, listeners, document, flush, radio, context };
}

test('the page fetches only the two locale files and /chat with {"hardware":true}, through a whole walk', async () => {
  const page = await runInBrowserStandIn(REAL_RESPONSE);
  assert.strictEqual(JSON.stringify(page.history.entries), '[{"installerStep":"language"}]');
  assert.strictEqual(page.main.byTag('h1')[0].textContent, Installer.LANGUAGE_PROMPT);
  page.radio('language', 'ar').fire('change');
  page.main.byClass('nav-forward')[0].fire('click');
  await page.flush();
  page.radio('tier', 'lite').fire('change');
  page.main.byClass('nav-forward')[0].fire('click');
  assert.strictEqual(page.document.title, 'إعداد القرص والحساب');
  page.listeners.popstate({ state: { installerStep: 'tier' } });
  page.main.byClass('nav-forward')[0].fire('click');
  assert.deepStrictEqual(page.fetched.map((f) => f.url).sort(), ['../../locales/ar/scarfeeos.ftl', '../../locales/en/scarfeeos.ftl', '/chat']);
  const chat = page.fetched.find((f) => f.url === '/chat');
  assert.strictEqual(chat.init.method, 'POST');
  assert.strictEqual(chat.init.body, '{"hardware":true}');
  for (const f of page.fetched) if (f.url !== '/chat') assert.strictEqual(f.init, undefined);
});

test('the page does not fetch /chat at all for a hardware request that is not exactly {"hardware":true}', async () => {
  const page = await runInBrowserStandIn(REAL_RESPONSE);
  const before = page.fetched.length;
  // `ask` is private to start(); it is reachable only through the installer, which sends one fixed request.
  page.radio('language', 'en').fire('change');
  page.main.byClass('nav-forward')[0].fire('click');
  await page.flush();
  assert.strictEqual(page.fetched.length - before, 1);
  assert.strictEqual(page.fetched.at(-1).init.body, '{"hardware":true}');
});

// CSS

function physicalDirectionFindings(css) {
  const findings = [];
  const declaration = /([a-z-]+)\s*:\s*([^;{}]+)/g;
  const stripped = css.replace(/\/\*[\s\S]*?\*\//g, '');
  for (const [, property, value] of stripped.matchAll(declaration)) {
    const words = value.trim().split(/\s+/);
    if (/(^|-)(left|right)(-|$)/.test(property)) {
      findings.push(`${property}: ${value.trim()}`);
    } else if (/\b(left|right)\b/.test(value) || /translateX|scaleX|rotateY/.test(value)) {
      findings.push(`${property}: ${value.trim()}`);
    } else if (/^(margin|padding|inset|border-radius|border-width|border-color)$/.test(property) && words.length > 2 && !/\(/.test(value)) {
      findings.push(`${property}: ${value.trim()} (physical shorthand)`);
    }
  }
  return findings;
}

test('installer.css uses no physical left/right properties or values', () => {
  const css = read('installer.css');
  assert.ok(css.length > 500);
  assert.deepStrictEqual(physicalDirectionFindings(css), []);
});

test('the CSS scanner itself catches physical properties', () => {
  assert.ok(physicalDirectionFindings('a { margin-left: 1px }').length > 0);
  assert.ok(physicalDirectionFindings('a { padding-right: 1px }').length > 0);
  assert.ok(physicalDirectionFindings('a { border-left-width: 1px }').length > 0);
  assert.ok(physicalDirectionFindings('a { text-align: left }').length > 0);
  assert.ok(physicalDirectionFindings('a { float: right }').length > 0);
  assert.ok(physicalDirectionFindings('a { transform: translateX(4px) }').length > 0);
  assert.ok(physicalDirectionFindings('a { margin: 1px 2px 3px 4px }').length > 0);
  assert.ok(physicalDirectionFindings('a { inset: 0 1px 2px }').length > 0);
  assert.deepStrictEqual(physicalDirectionFindings('a { margin-inline-start: 1px; text-align: start }'), []);
  assert.deepStrictEqual(physicalDirectionFindings('/* left: 0 */ a { margin: 1px 2px }'), []);
});

// Scripts

test('every script in index.html loads together in one global scope', () => {
  const scripts = [...html.matchAll(/<script src="([^"]+)"/g)].map((m) => m[1]);
  assert.deepStrictEqual(scripts, ['../ui/i18n.js', 'installer.js']);
  const context = vm.createContext({ console });
  const declared = new Map();
  for (const script of scripts) {
    const code = read(script);
    vm.runInContext(code, context, { filename: script });
    for (const [, name] of code.matchAll(/^(?:const|let|class|var|function)\s+([A-Za-z_$][\w$]*)/gm)) {
      assert.ok(!declared.has(name), `${name} is declared in both ${declared.get(name)} and ${script}`);
      declared.set(name, script);
    }
  }
  assert.strictEqual(typeof context.ScarfeeI18n.parseFtl, 'function');
  assert.strictEqual(typeof context.ScarfeeInstaller.createInstaller, 'function');
  assert.deepStrictEqual([...declared.values()].filter((s) => s === 'installer.js'), []);
});
