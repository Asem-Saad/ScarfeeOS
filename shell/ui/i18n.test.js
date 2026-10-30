'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const { parseFtl, directionOf, createI18n } = require('./i18n.js');
const { segmentText } = require('./panel.js');

const read = (...parts) => fs.readFileSync(path.join(__dirname, ...parts), 'utf8');
const locale = (lang) => parseFtl(read('..', '..', 'locales', lang, 'scarfeeos.ftl'));

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

function fakeRoot() {
  return { lang: '', dir: '' };
}

test('parser reads key = value lines, skips blanks and comments, trims', () => {
  const messages = parseFtl('# comment\n\nhello = Hello there \r\nbye=Bye\nbad line\nempty =\n');
  assert.strictEqual(messages.get('hello'), 'Hello there');
  assert.strictEqual(messages.get('bye'), 'Bye');
  assert.strictEqual(messages.get('empty'), '');
  assert.strictEqual(messages.size, 3);
});

test('parser keeps "=" inside values and lets the last duplicate win', () => {
  const messages = parseFtl('a = x = y\na = z\n');
  assert.strictEqual(messages.get('a'), 'z');
  assert.strictEqual(parseFtl('a = x = y').get('a'), 'x = y');
});

test('real locale files parse to the expected existing messages', () => {
  assert.strictEqual(locale('en').get('assistant-greeting'), 'How can I help you?');
  assert.strictEqual(locale('ar').get('installer-continue'), 'متابعة');
});

test('missing key shows the key and does not throw', () => {
  const i18n = createI18n({ en: new Map([['a', 'A']]) }, fakeRoot(), 'en');
  assert.strictEqual(i18n.t('a'), 'A');
  assert.strictEqual(i18n.t('no-such-key'), 'no-such-key');
  assert.strictEqual(i18n.t('constructor'), 'constructor');
  assert.strictEqual(i18n.t('__proto__'), '__proto__');
});

test('missing key shows the key when the language catalog is empty', () => {
  const i18n = createI18n({ en: new Map(), ar: new Map() }, fakeRoot(), 'ar');
  assert.strictEqual(i18n.t('assistant-send'), 'assistant-send');
});

test('setLanguage sets lang and dir on the root and changes translations', () => {
  const root = fakeRoot();
  const i18n = createI18n({ en: locale('en'), ar: locale('ar') }, root, 'en');
  assert.strictEqual(i18n.setLanguage('ar'), true);
  assert.deepStrictEqual(root, { lang: 'ar', dir: 'rtl' });
  assert.strictEqual(i18n.t('assistant-send'), 'إرسال');
  assert.strictEqual(i18n.setLanguage('en'), true);
  assert.deepStrictEqual(root, { lang: 'en', dir: 'ltr' });
  assert.strictEqual(i18n.t('assistant-send'), 'Send');
});

test('unknown language is rejected and leaves the root unchanged', () => {
  const root = fakeRoot();
  const i18n = createI18n({ en: new Map(), ar: new Map() }, root, 'en');
  i18n.setLanguage('ar');
  assert.strictEqual(i18n.setLanguage('xx'), false);
  assert.strictEqual(i18n.setLanguage('toString'), false);
  assert.deepStrictEqual(root, { lang: 'ar', dir: 'rtl' });
  assert.strictEqual(i18n.getLanguage(), 'ar');
  assert.strictEqual(directionOf('en'), 'ltr');
  assert.strictEqual(directionOf('ar'), 'rtl');
});

test('translate fills text and attributes from data-i18n markers', () => {
  const attrs = {};
  const node = (marks) => ({
    textContent: '',
    getAttribute: (name) => marks[name],
    setAttribute: (name, value) => { attrs[name] = value; },
  });
  const textNode = node({ 'data-i18n': 'k' });
  const attrNode = node({ 'data-i18n-placeholder': 'k', 'data-i18n-aria-label': 'missing' });
  const container = {
    querySelectorAll: (selector) => {
      if (selector === '[data-i18n]') return [textNode];
      if (selector === '[data-i18n-placeholder]') return [attrNode];
      if (selector === '[data-i18n-aria-label]') return [attrNode];
      return [];
    },
  };
  createI18n({ en: new Map([['k', 'Value']]) }, fakeRoot(), 'en').translate(container);
  assert.strictEqual(textNode.textContent, 'Value');
  assert.strictEqual(attrs.placeholder, 'Value');
  assert.strictEqual(attrs['aria-label'], 'missing');
});

test('both locale files define the same keys', () => {
  assert.deepStrictEqual([...locale('en').keys()].sort(), [...locale('ar').keys()].sort());
});

test('every key used in index.html, panel.js, render.js and settings.js exists in both locale files', () => {
  const html = read('index.html');
  const js = read('panel.js') + read('render.js') + read('settings.js');
  const used = new Set();
  for (const [, key] of html.matchAll(/data-i18n(?:-[a-z-]+)?="([^"]+)"/g)) used.add(key);
  for (const [, key] of js.matchAll(/\bt\(\s*['"]([^'"]+)['"]\s*\)/g)) used.add(key);
  assert.ok(used.size >= 50, `expected to find the panel's keys, found ${used.size}`);
  for (const lang of ['en', 'ar']) {
    const messages = locale(lang);
    for (const key of used) {
      assert.ok(messages.has(key), `${lang} is missing key "${key}"`);
    }
  }
});

test('panel.js, render.js and settings.js have no t() call with a non-literal key', () => {
  const calls = (read('panel.js') + read('render.js') + read('settings.js')).match(/\bt\(\s*[^'"\s)]/g) || [];
  assert.deepStrictEqual(calls, []);
});

test('index.html has no hard-coded text or plain text attributes', () => {
  const html = read('index.html')
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<script[\s\S]*?<\/script>/g, '');
  const text = html.replace(/<[^>]*>/g, '').trim();
  assert.strictEqual(text, '');
  assert.doesNotMatch(html, /\s(placeholder|title|alt|aria-label)=/);
});

test('panel.css uses no physical left/right properties or values', () => {
  const findings = physicalDirectionFindings(read('panel.css'));
  assert.deepStrictEqual(findings, []);
});

test('the CSS scanner itself catches physical properties', () => {
  assert.ok(physicalDirectionFindings('a { margin-left: 1px }').length > 0);
  assert.ok(physicalDirectionFindings('a { padding-right: 1px }').length > 0);
  assert.ok(physicalDirectionFindings('a { border-left-width: 1px }').length > 0);
  assert.ok(physicalDirectionFindings('a { text-align: left }').length > 0);
  assert.ok(physicalDirectionFindings('a { float: right }').length > 0);
  assert.ok(physicalDirectionFindings('a { transform: translateX(4px) }').length > 0);
  assert.ok(physicalDirectionFindings('a { margin: 1px 2px 3px 4px }').length > 0);
  assert.deepStrictEqual(physicalDirectionFindings('a { margin-inline-start: 1px; text-align: start }'), []);
  assert.deepStrictEqual(physicalDirectionFindings('/* left: 0 */ a { margin: 1px 2px }'), []);
});

test('messages use dir="auto" and the root starts with lang and dir', () => {
  assert.match(read('panel.js'), /body\.dir = 'auto'/);
  assert.match(read('index.html'), /<html lang="en" dir="ltr">/);
});

test('URLs, file names, paths and numbers are kept intact as LTR runs', () => {
  const ltr = (text) => segmentText(text).filter((s) => s.ltr).map((s) => s.text);
  assert.deepStrictEqual(ltr('افتح https://example.com/a/b?x=1، ثم أغلقه'), ['https://example.com/a/b?x=1']);
  assert.deepStrictEqual(ltr('أرسل الملف report-2024.pdf إلى سارة.'), ['report-2024.pdf']);
  assert.deepStrictEqual(ltr('Open /home/user/notes.txt now.'), ['/home/user/notes.txt']);
  assert.deepStrictEqual(ltr('الاجتماع الساعة 10:30 يوم 12/05/2026 في القاعة 7'), ['10:30', '12/05/2026', '7']);
  assert.deepStrictEqual(ltr('السعر 1,234.50 ريال.'), ['1,234.50']);
  assert.deepStrictEqual(ltr('Visit https://example.com.'), ['https://example.com']);
  assert.deepStrictEqual(ltr('مرحبا بالعالم'), []);
});

test('segments rebuild the original text exactly', () => {
  for (const text of ['افتح https://example.com/a، ثم report.pdf في 10:30.', 'plain', '', '42']) {
    assert.strictEqual(segmentText(text).map((s) => s.text).join(''), text);
  }
});
