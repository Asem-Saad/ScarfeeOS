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
  constructor(tag) {
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
  replaceChildren(...nodes) { this.ownText = ''; this.children = nodes; }
  setAttribute(name, value) { this.attributes[name] = String(value); }
  getAttribute(name) { return this.attributes[name] ?? null; }
  addEventListener(type, fn) { (this.listeners[type] ||= []).push(fn); }
  fire(type) { for (const fn of this.listeners[type] || []) fn({ type }); }
  focus() { this.focused = true; }
  all() { return [this, ...this.children.flatMap((c) => c.all())]; }
  find(predicate) { return this.all().filter(predicate); }
  byClass(name) { return this.find((n) => n.className.split(/\s+/).includes(name)); }
  byTag(name) { return this.find((n) => n.tagName === name); }

  // Only `input[name="x"]:checked`, the one selector installer.js uses.
  querySelector(selector) {
    const [, name] = /^input\[name="([a-z]+)"\]:checked$/.exec(selector) || [];
    assert.ok(name, `unsupported selector ${selector}`);
    return this.find((n) => n.tagName === 'input' && n.name === name && n.checked)[0] || null;
  }
}

function setup(response) {
  const document = { title: '', createElement: (tag) => new FakeNode(tag) };
  const container = new FakeNode('main');
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
    ask(body) {
      asked.push(body);
      return Promise.resolve(typeof response === 'function' ? response(body) : response);
    },
    save(choices) {
      saved.push(choices);
      return Installer.saveChoicesToJsonFile(choices);
    },
  });
  installer.render();
  const radio = (name, value) => container.byTag('input').find((n) => n.name === name && n.value === value);
  const buttons = () => container.byTag('button');
  const forward = () => container.byClass('nav-forward')[0];
  const flush = () => new Promise((resolve) => setImmediate(resolve));
  return { installer, container, root, document, asked, saved, radio, buttons, forward, flush, catalogs };
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
  assert.strictEqual(forward().disabled, true);
  assert.strictEqual(installer.next(), false);
  assert.strictEqual(installer.state.step, 'language');
  assert.deepStrictEqual(asked, []);
  assert.strictEqual(root.dir, '');
});

test('choosing a language switches the whole page to it, RTL for Arabic', () => {
  const { container, document, forward, radio, root } = setup(REAL_RESPONSE);
  radio('language', 'ar').fire('change');
  assert.strictEqual(root.lang, 'ar');
  assert.strictEqual(root.dir, 'rtl');
  assert.strictEqual(document.title, 'اختر لغتك');
  assert.strictEqual(forward().textContent, 'متابعة');
  assert.strictEqual(forward().disabled, false);
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
  assert.strictEqual(forward().disabled, false);
});

test('choosing a tier above the hardware warns but does not block', async () => {
  const { container, radio, forward, installer } = await toTierScreen(REAL_RESPONSE);
  radio('tier', 'dual').fire('change');
  const warning = container.byClass('notice-warning');
  assert.strictEqual(warning.length, 1);
  assert.strictEqual(isolates(warning[0].textContent), 'Dual needs more than this computer has. You can still choose it, but the assistant may run slowly or fail to load its models.');
  assert.strictEqual(warning[0].getAttribute('role'), 'alert');
  assert.strictEqual(forward().disabled, false);
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
  assert.match(isolates(container.textContent), /Graphics card: no NVIDIA GPU detected/);
  radio('tier', 'full').fire('change');
  assert.strictEqual(container.byClass('notice-warning').length, 1);
  radio('tier', 'dual').fire('change');
  assert.strictEqual(container.byClass('notice-warning').length, 1);
  assert.strictEqual(radio('tier', 'dual').checked, true);
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
    assert.strictEqual(forward().disabled, true);
    radio('tier', 'dual').fire('change');
    assert.strictEqual(container.byClass('notice-warning').length, 1);
    assert.match(container.byClass('notice-warning')[0].textContent, /could not be checked/);
    assert.strictEqual(forward().disabled, false);
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
  assert.match(page.container.textContent, /Checking this computer/);
  page.radio('tier', 'lite').fire('change');
  page.installer.loadHardware();
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
  const allowed = new Set([Installer.LANGUAGE_PROMPT, 'English', 'العربية', 'use strict']);
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
