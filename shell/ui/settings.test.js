'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const S = require('./settings.js');
const R = require('./render.js');
const { parseFtl } = require('./i18n.js');

const read = (...parts) => fs.readFileSync(path.join(__dirname, ...parts), 'utf8');
const locale = (lang) => parseFtl(read('..', '..', 'locales', lang, 'scarfeeos.ftl'));
const tFor = (lang) => { const m = locale(lang); return (key) => (m.has(key) ? m.get(key) : key); };
const hijri = (y, m, d) => S.toHijriApprox(y, m, d);
const h = (year, month, day) => ({ year, month, day });
const g = (year, month, day) => ({ year, month, day });

test('digits convert both ways and leave everything else alone', () => {
  assert.strictEqual(S.toEasternDigits('0123456789'), '٠١٢٣٤٥٦٧٨٩');
  assert.strictEqual(S.toWesternDigits('٠١٢٣٤٥٦٧٨٩'), '0123456789');
  assert.strictEqual(S.toEasternDigits('09:05 ab-12'), '٠٩:٠٥ ab-١٢');
  assert.strictEqual(S.toWesternDigits('٠٩:٠٥ ب-١٢'), '09:05 ب-12');
  assert.strictEqual(S.toEasternDigits(S.toWesternDigits('٢٠٢٦-٠١-٠١')), '٢٠٢٦-٠١-٠١');
  assert.strictEqual(S.toWesternDigits(S.toEasternDigits('2026-01-01')), '2026-01-01');
  assert.strictEqual(S.toEasternDigits(1447), '١٤٤٧');
  assert.strictEqual(S.shapeDigits('12', 'eastern'), '١٢');
  assert.strictEqual(S.shapeDigits('١٢', 'western'), '12');
  assert.strictEqual(S.shapeDigits('12', 'anything else'), '12');
});

test('Hijri: 12 Rajab 1447 is 2026-01-01', () => {
  assert.deepStrictEqual(hijri(2026, 1, 1), h(1447, 7, 12));
  assert.deepStrictEqual(S.fromHijriApprox(1447, 7, 12), g(2026, 1, 1));
});

test('Hijri: reference dates worked out by hand from the tabular algorithm', () => {
  // 2000-01-01 is 10957 days after 1970, so day 503105 since the epoch. 1420 starts at day 502846,
  // 259 days in; 8 months make 236, so 23 days into Ramadan: 24 Ramadan 1420.
  assert.deepStrictEqual(hijri(2000, 1, 1), h(1420, 9, 24));
  // 2024-02-29 is day 511930; 1445 starts at 511705, 225 days in; 7 months make 207, so 19 Sha'ban.
  assert.deepStrictEqual(hijri(2024, 2, 29), h(1445, 8, 19));
  // Ramadan 1445 starts 2024-03-11 (the day after 29 Sha'ban).
  assert.deepStrictEqual(hijri(2024, 3, 11), h(1445, 9, 1));
  assert.deepStrictEqual(hijri(2024, 3, 10), h(1445, 8, 29));
  // 1447 has 355 days, so 2026-12-31 (day 552 of 1447, counting from 0) is day 197 of 1448: 21 Rajab 1448.
  assert.deepStrictEqual(hijri(2026, 12, 31), h(1448, 7, 21));
  // 1 Muharram 1447 is 188 days before 2026-01-01: 2025-06-27.
  assert.deepStrictEqual(S.fromHijriApprox(1447, 1, 1), g(2025, 6, 27));
  // The epoch: Friday 19 July 622.
  assert.deepStrictEqual(hijri(622, 7, 19), h(1, 1, 1));
  assert.deepStrictEqual(S.fromHijriApprox(1, 1, 1), g(622, 7, 19));
});

test('Hijri: leap years, month lengths and the end of a leap year', () => {
  const leap = []; for (let y = 1; y <= 30; y += 1) if (S.isLeapHijriYear(y)) leap.push(y);
  assert.deepStrictEqual(leap, [2, 5, 7, 10, 13, 16, 18, 21, 24, 26, 29]);
  assert.strictEqual(S.isLeapHijriYear(1447), true);
  assert.strictEqual(S.isLeapHijriYear(1446), false);
  assert.strictEqual(S.hijriMonthLength(1447, 12), 30);
  assert.strictEqual(S.hijriMonthLength(1446, 12), 29);
  assert.strictEqual(S.hijriMonthLength(1446, 1), 30);
  assert.strictEqual(S.hijriMonthLength(1446, 2), 29);
  assert.strictEqual(S.hijriMonthLength(1446, 13), null);
  const last = S.fromHijriApprox(1447, 12, 30);
  assert.deepStrictEqual(hijri(last.year, last.month, last.day), h(1447, 12, 30));
  const next = new Date(Date.UTC(last.year, last.month - 1, last.day + 1));
  assert.deepStrictEqual(hijri(next.getUTCFullYear(), next.getUTCMonth() + 1, next.getUTCDate()), h(1448, 1, 1));
});

test('Hijri: invalid Hijri dates are rejected', () => {
  for (const [y, m, d] of [[1446, 12, 30], [1447, 2, 30], [1447, 1, 31], [1447, 1, 0], [1447, 0, 1], [1447, 13, 1], [0, 1, 1], [-5, 1, 1], [1000001, 1, 1], [1447.5, 1, 1]]) {
    assert.strictEqual(S.fromHijriApprox(y, m, d), null, `${y}-${m}-${d}`);
  }
});

test('Hijri: every day from 1900 to 2100 round trips and advances by one day', () => {
  const start = Date.UTC(1900, 0, 1);
  let previous = null;
  for (let i = 0; i < 200 * 366; i += 1) {
    const day = new Date(start + i * 86400000);
    const [y, m, d] = [day.getUTCFullYear(), day.getUTCMonth() + 1, day.getUTCDate()];
    const current = hijri(y, m, d);
    assert.deepStrictEqual(S.fromHijriApprox(current.year, current.month, current.day), g(y, m, d));
    if (previous) {
      const len = S.hijriMonthLength(previous.year, previous.month);
      const expected = previous.day < len ? h(previous.year, previous.month, previous.day + 1)
        : previous.month < 12 ? h(previous.year, previous.month + 1, 1) : h(previous.year + 1, 1, 1);
      assert.deepStrictEqual(current, expected, `${y}-${m}-${d}`);
    }
    previous = current;
  }
});

test('Gregorian date check', () => {
  assert.strictEqual(S.isGregorianDate(2024, 2, 29), true);
  assert.strictEqual(S.isGregorianDate(2026, 2, 29), false);
  assert.strictEqual(S.isGregorianDate(1900, 2, 29), false);
  assert.strictEqual(S.isGregorianDate(2000, 2, 29), true);
  assert.strictEqual(S.isGregorianDate(2026, 13, 1), false);
  assert.strictEqual(S.isGregorianDate(2026, 4, 31), false);
});

test('defaults: Western digits (also for Arabic), Gregorian only, week starts on Sunday, English', () => {
  assert.deepStrictEqual(S.createStore(() => null).get(), { language: 'en', digits: 'western', calendar: 'gregorian', weekStart: 'sunday' });
});

function storageOf(initial) {
  const data = new Map(Object.entries(initial || {}));
  return { data, getItem: (k) => (data.has(k) ? data.get(k) : null), setItem: (k, v) => { data.set(k, v); } };
}

test('the store saves each choice and a new store reads it back', () => {
  const storage = storageOf();
  const store = S.createStore(() => storage);
  assert.strictEqual(store.set('digits', 'eastern'), true);
  assert.strictEqual(store.set('language', 'ar'), true);
  assert.strictEqual(store.set('weekStart', 'saturday'), true);
  assert.strictEqual(store.set('calendar', 'both'), true);
  const again = S.createStore(() => storage).get();
  assert.deepStrictEqual(again, { language: 'ar', digits: 'eastern', calendar: 'both', weekStart: 'saturday' });
});

test('the store refuses unknown keys and values, and ignores bad saved data', () => {
  const storage = storageOf();
  const store = S.createStore(() => storage);
  assert.strictEqual(store.set('digits', 'roman'), false);
  assert.strictEqual(store.set('nope', 'x'), false);
  assert.strictEqual(store.set('__proto__', 'x'), false);
  assert.strictEqual(storage.data.size, 0);
  for (const bad of ['not json', '[1]', 'null', '{"digits":"roman","weekStart":5,"language":"fr"}', '{"digits":"eastern","extra":1}']) {
    const settings = S.createStore(() => storageOf({ [S.STORAGE_KEY]: bad })).get();
    assert.strictEqual(settings.language, 'en');
    assert.strictEqual(settings.weekStart, 'sunday');
    assert.strictEqual(settings.digits, bad.includes('"eastern"') ? 'eastern' : 'western');
    assert.deepStrictEqual(Object.keys(settings).sort(), ['calendar', 'digits', 'language', 'weekStart']);
  }
});

test('storage failure: missing, throwing lookup, throwing read and throwing write all keep settings working', () => {
  const failing = { getItem() { throw new Error('denied'); }, setItem() { throw new Error('quota'); } };
  for (const getStorage of [() => null, () => undefined, () => { throw new Error('SecurityError'); }, () => failing]) {
    const store = S.createStore(getStorage);
    assert.strictEqual(store.get().digits, 'western');
    assert.strictEqual(store.set('digits', 'eastern'), true);
    assert.strictEqual(store.get().digits, 'eastern');
  }
  assert.strictEqual(S.browserStorage(), null, 'no localStorage in Node');
  const saved = global.localStorage;
  Object.defineProperty(global, 'localStorage', { configurable: true, get() { throw new Error('SecurityError'); } });
  try {
    assert.strictEqual(S.browserStorage(), null);
  } finally {
    delete global.localStorage;
    if (saved !== undefined) global.localStorage = saved;
  }
});

test('formatDate: Gregorian, with approximate Hijri, with digits, and untouched when not a date', () => {
  const en = tFor('en');
  const ar = tFor('ar');
  const both = { digits: 'western', calendar: 'both' };
  assert.strictEqual(R.formatDate('2026-01-01'), '2026-01-01');
  assert.strictEqual(R.formatDate('2026-01-01', { digits: 'eastern', calendar: 'gregorian' }, en), '٢٠٢٦-٠١-٠١');
  assert.strictEqual(R.formatDate('2026-01-01', both, en).replace(/[⁨⁩]/g, ''), '2026-01-01 (about 12 Rajab 1447 AH)');
  assert.strictEqual(R.formatDate('2026-01-01', { digits: 'eastern', calendar: 'both' }, ar).replace(/[⁨⁩]/g, ''), '٢٠٢٦-٠١-٠١ (نحو ١٢ رجب ١٤٤٧ هـ)');
  assert.strictEqual(R.formatDate('2024-03-11', both, en).replace(/[⁨⁩]/g, ''), '2024-03-11 (about 1 Ramadan 1445 AH)');
  for (const text of ['2026-02-30', 'tomorrow', '٢٠٢٦-٠١-٠١', '2026-1-1', '']) {
    assert.strictEqual(R.formatDate(text, { digits: 'eastern', calendar: 'both' }, en), text);
  }
});

test('every Hijri date shown with the Gregorian one carries the word for approximate in both languages', () => {
  for (const lang of ['en', 'ar']) {
    const text = locale(lang).get('settings-date-both');
    assert.match(text, lang === 'en' ? /\(about / : /\(نحو /);
  }
});

test('describeResponse shapes the numbers the panel formats and nothing from the model', () => {
  const en = tFor('en');
  const eastern = { digits: 'eastern', calendar: 'gregorian' };
  const strip = (blocks) => blocks.map((b) => b.text.replace(/[⁨⁩]/g, '')).join('\n');
  assert.strictEqual(strip(R.describeResponse({ kind: 'time', date: '2026-10-30', hour: 9, minute: 5, weekday: 'Friday' }, en, 'en', eastern)),
    'The time on this device is ٠٩:٠٥, on Friday ٢٠٢٦-١٠-٣٠.');
  assert.strictEqual(R.formatElapsed(75, eastern), '١:١٥');
  assert.strictEqual(R.formatElapsed(75), '1:15');
  assert.strictEqual(R.formatTime('2026-10-30T14:30:00', eastern, en), '٢٠٢٦-١٠-٣٠ ١٤:٣٠');
  assert.strictEqual(R.formatTime('whenever 12', eastern, en), 'whenever 12');
  const run = strip(R.describeResponse({
    kind: 'run', tool: 'create_reminder', arguments: { text: 'call 555-0100 at https://a.example/b2', time: '2026-10-30T14:30' },
  }, en, 'en', eastern));
  assert.ok(run.includes('call 555-0100 at https://a.example/b2'), run);
  assert.ok(run.includes('٢٠٢٦-١٠-٣٠ ١٤:٣٠'), run);
  const files = strip(R.describeResponse({ kind: 'run', tool: 'list_files', arguments: { path: '/home/u/report-2024.pdf' } }, en, 'en', eastern));
  assert.ok(files.includes('/home/u/report-2024.pdf'), files);
  const reply = strip(R.describeResponse({ kind: 'reply', text: 'Room 12 at 10:30, see http://x.example/9' }, en, 'en', eastern));
  assert.strictEqual(reply, 'Room 12 at 10:30, see http://x.example/9');
  const core = strip(R.describeResponse({ kind: 'hijri_approx', gregorian: '2026-01-01', day: 12, month_name_en: 'Rajab', year: 1447, approximate: true }, en, 'en', eastern));
  assert.ok(core.startsWith('٢٠٢٦-٠١-٠١ is about ١٢ Rajab ١٤٤٧ AH.'), core);
});

test('the date calendar setting adds Hijri after times too, and Western digits are the default without settings', () => {
  const en = tFor('en');
  const text = R.formatTime('2026-01-01T09:05', { digits: 'western', calendar: 'both' }, en).replace(/[⁨⁩]/g, '');
  assert.strictEqual(text, '2026-01-01 (about 12 Rajab 1447 AH) 09:05');
  assert.strictEqual(R.formatTime('2026-01-01T09:05'), '2026-01-01 09:05');
});

test('every new settings key exists in both locale files, with the same placeholders', () => {
  const html = read('index.html');
  const keys = new Set();
  for (const [, key] of html.matchAll(/data-i18n(?:-[a-z-]+)?="((?:settings|hijri)-[^"]+)"/g)) keys.add(key);
  for (const [, key] of read('render.js').matchAll(/['"]((?:settings|hijri)-[a-z-]+)['"]/g)) keys.add(key);
  assert.ok(keys.size >= 21, `found ${keys.size} keys`);
  const placeholders = (text) => [...text.matchAll(/\{\s*\$([A-Za-z]+)\s*\}/g)].map((m) => m[1]).sort();
  for (const key of keys) {
    const en = locale('en').get(key);
    const ar = locale('ar').get(key);
    assert.ok(en, `en is missing ${key}`);
    assert.ok(ar, `ar is missing ${key}`);
    assert.deepStrictEqual(placeholders(ar), placeholders(en), key);
  }
  assert.deepStrictEqual([...locale('en').keys()].sort(), [...locale('ar').keys()].sort());
});

test('the scripts load together in one page: no top-level name clashes', () => {
  const context = vm.createContext({});
  for (const file of ['i18n.js', 'settings.js', 'render.js']) {
    vm.runInContext(read(file), context, { filename: file });
  }
  assert.strictEqual(typeof context.ScarfeeSettings.createStore, 'function');
  assert.strictEqual(context.ScarfeeRender.formatDate('2026-01-01', { digits: 'eastern' }), '٢٠٢٦-٠١-٠١');
  const order = [...read('index.html').matchAll(/<script src="([^"]+)">/g)].map((m) => m[1]);
  assert.deepStrictEqual(order, ['i18n.js', 'settings.js', 'render.js', 'panel.js']);
});
