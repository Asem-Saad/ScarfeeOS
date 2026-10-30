'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const C = require('./calendar.js');
const { parseFtl } = require('./i18n.js');

const read = (...parts) => fs.readFileSync(path.join(__dirname, ...parts), 'utf8');
const locale = (lang) => parseFtl(read('..', '..', 'locales', lang, 'scarfeeos.ftl'));
const tFor = (lang) => { const m = locale(lang); return (key) => (m.has(key) ? m.get(key) : `<<${key}>>`); };
const isolates = (text) => text.replace(/[⁨⁩]/g, '');
const html = read('calendar.html');
const TODAY = { year: 2026, month: 10, day: 30 };

// A very small DOM: enough for calendar.js, and it refuses anything that would parse event text as markup.
class FakeNode {
  constructor(tag) {
    this.tagName = tag;
    this.children = [];
    this.attributes = {};
    this.listeners = {};
    this.className = '';
    this.hidden = false;
    this.ownText = '';
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
  click() { this.fire('click'); }
  all() { return [this, ...this.children.flatMap((c) => c.all())]; }
  find(predicate) { return this.all().filter(predicate); }
  byClass(name) { return this.find((n) => n.className.split(/\s+/).includes(name)); }
  byTag(name) { return this.find((n) => n.tagName === name); }
}

function fakeDocument() {
  const byId = new Map();
  for (const [, id] of html.matchAll(/\sid="([^"]+)"/g)) {
    byId.set(id, new FakeNode('x'));
  }
  return {
    byId,
    documentElement: { lang: '', dir: '' },
    getElementById: (id) => byId.get(id) || null,
    createElement: (tag) => new FakeNode(tag),
    querySelectorAll: () => [],
  };
}

function storageOf(initial = {}) {
  const data = new Map(Object.entries(initial));
  return { data, getItem: (k) => (data.has(k) ? data.get(k) : null), setItem: (k, v) => data.set(k, String(v)) };
}

const settingsWith = (over = {}) => ({ language: 'en', digits: 'western', calendar: 'gregorian', weekStart: 'sunday', ...over });

function make({ replies = [], settings = settingsWith(), lang = 'en', weekend, today = TODAY } = {}) {
  const document = fakeDocument();
  const requests = [];
  const queue = [...replies];
  const weekendStore = C.createWeekendStore(() => storageOf(weekend ? { [C.WEEKEND_KEY]: JSON.stringify(weekend) } : {}));
  const calendar = C.createCalendar({
    document,
    t: tFor(lang),
    settings: () => settings,
    weekendStore,
    today: () => today,
    ask(body) {
      requests.push(body);
      const next = queue.shift();
      return Promise.resolve(typeof next === 'function' ? next(body) : next);
    },
  });
  const node = (id) => document.byId.get(id);
  return { calendar, document, requests, node, weekendStore, queue };
}

const ev = (uid, summary, date, over = {}) => ({
  uid, summary, date, start: `${date}T09:00:00`, attendees: [], repeat: null, ...over,
});
const events = (...list) => ({ kind: 'events', from: '2026-10-30', events: list });

test('the upcoming request is {"upcoming": true, "from": today}', async () => {
  const { calendar, requests } = make({ replies: [events()] });
  await calendar.refresh();
  assert.deepStrictEqual(requests, [{ upcoming: true, from: '2026-10-30' }]);
});

test('events are listed in date order with their time, whatever order they arrive in', async () => {
  const { calendar, node } = make({
    replies: [events(ev('c', 'Third', '2026-11-20'), ev('b', 'Second', '2026-11-02', { start: '2026-11-02T14:00:00' }), ev('a', 'First', '2026-11-02', { start: '2026-11-02T08:30:00' }))],
  });
  await calendar.refresh();
  const items = node('events-list').children;
  assert.deepStrictEqual(items.map((i) => i.byClass('event-title')[0].textContent), ['First', 'Second', 'Third']);
  assert.strictEqual(isolates(items[0].byClass('event-meta')[0].textContent), '2026-11-02 at 08:30');
  assert.strictEqual(node('events-status').hidden, true);
});

test('Hijri comes after the Gregorian date only when the setting is on, and is worded as approximate', async () => {
  const reply = () => events(ev('a', 'Review', '2026-01-01', { start: '2026-01-01T10:00:00' }));
  const off = make({ replies: [reply()] });
  await off.calendar.refresh();
  assert.strictEqual(isolates(off.node('events-list').children[0].textContent).includes('AH'), false);

  const on = make({ replies: [reply()], settings: settingsWith({ calendar: 'both' }) });
  await on.calendar.refresh();
  const when = isolates(on.node('events-list').children[0].byClass('event-meta')[0].textContent);
  assert.strictEqual(when, '2026-01-01 (about 12 Rajab 1447 AH) at 10:00');
  assert.match(locale('en').get('settings-date-both'), /about/);
  assert.strictEqual(on.node('month-hijri-note').hidden, false, 'the grid explains that Hijri is approximate');
  assert.strictEqual(off.node('month-hijri-note').hidden, true);

  const ar = make({ replies: [reply()], settings: settingsWith({ calendar: 'both', digits: 'eastern', language: 'ar' }), lang: 'ar' });
  await ar.calendar.refresh();
  assert.strictEqual(
    isolates(ar.node('events-list').children[0].byClass('event-meta')[0].textContent),
    '٢٠٢٦-٠١-٠١ (نحو ١٢ رجب ١٤٤٧ هـ) الساعة ١٠:٠٠'
  );
});

test('repeats, attendees and all-day events are shown', async () => {
  const { calendar, node } = make({
    replies: [events(
      ev('w', 'Standup', '2026-11-02', { repeat: 'weekly', attendees: ['Sara', 'Omar'] }),
      ev('d', 'Pills', '2026-11-03', { repeat: 'daily' }),
      ev('x', 'Holiday', '2026-11-04', { start: '2026-11-04' })
    )],
  });
  await calendar.refresh();
  const text = node('events-list').children.map((i) => isolates(i.textContent));
  assert.match(text[0], /Repeats every week/);
  assert.match(text[0], /With Sara, Omar/);
  assert.match(text[1], /Repeats every day/);
  assert.match(text[2], /All day/);
  assert.doesNotMatch(text[2], /Repeats/);
});

test('an empty list says so, and an error or a lost connection is shown without losing the page', async () => {
  const empty = make({ replies: [events()] });
  await empty.calendar.refresh();
  assert.strictEqual(empty.node('events-status').textContent, 'You have no upcoming meetings or reminders.');
  assert.strictEqual(empty.node('events-list').children.length, 0);
  assert.ok(empty.node('month-grid').byTag('td').length >= 28, 'the month grid still draws');

  const bad = make({ replies: [{ kind: 'error', message: 'disk full' }] });
  await bad.calendar.refresh();
  assert.strictEqual(isolates(bad.node('events-status').textContent), 'Something went wrong: disk full');
  assert.strictEqual(bad.node('events-status').getAttribute('role'), 'alert');

  const down = make({ replies: [{ kind: 'unreachable' }] });
  await down.calendar.refresh();
  assert.strictEqual(down.node('events-status').textContent, locale('en').get('assistant-unreachable'));

  for (const odd of [undefined, null, { kind: 'events' }, { kind: 'events', events: 'x' }, { kind: 'run' }]) {
    const m = make({ replies: [odd] });
    await m.calendar.refresh();
    assert.strictEqual(m.calendar.state.phase, 'error');
    assert.strictEqual(m.node('events-list').children.length, 0);
  }

  const junk = make({ replies: [events(null, 7, { uid: 'u' }, ev('', 'no uid', '2026-11-01'), ev('u', 'bad date', '2026-02-30'), ev('ok', 'Fine', '2026-11-01'))] });
  await junk.calendar.refresh();
  assert.deepStrictEqual(junk.node('events-list').children.map((i) => i.byClass('event-title')[0].textContent), ['Fine']);
});

test('the status line shows loading until the reply arrives', async () => {
  let release;
  const { calendar, node } = make({ replies: [() => new Promise((resolve) => { release = resolve; })] });
  const pending = calendar.refresh();
  assert.strictEqual(node('events-status').textContent, locale('en').get('calendar-loading'));
  release(events());
  await pending;
  assert.notStrictEqual(node('events-status').textContent, locale('en').get('calendar-loading'));
});

test('delete asks first: nothing is sent until the confirmation, and keeping the event sends nothing', async () => {
  const { calendar, node, requests } = make({ replies: [events(ev('u1', 'Review', '2026-11-02')), { kind: 'event_deleted', uid: 'u1' }, events()] });
  await calendar.refresh();
  const row = () => node('events-list').children[0];
  const deleteButton = row().byClass('button-danger')[0];
  assert.strictEqual(deleteButton.textContent, 'Delete');
  deleteButton.click();
  assert.strictEqual(requests.length, 1, 'clicking delete only asks');
  assert.strictEqual(isolates(row().textContent).includes('Delete “Review”?'), true);

  row().byClass('button').find((b) => b.textContent === 'Keep it').click();
  assert.strictEqual(requests.length, 1);
  assert.strictEqual(row().byClass('button-danger')[0].textContent, 'Delete');

  row().byClass('button-danger')[0].click();
  const yes = row().byClass('button-danger')[0];
  assert.strictEqual(yes.textContent, 'Yes, delete');
  yes.click();
  assert.deepStrictEqual(requests[1], { delete_event: 'u1' });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepStrictEqual(requests[2], { upcoming: true, from: '2026-10-30' }, 'the list is read again');
  assert.strictEqual(node('events-list').children.length, 0);
  assert.strictEqual(isolates(node('events-notice').textContent), 'Deleted “Review”.');
  assert.strictEqual(node('events-notice').hidden, false);
});

test('deleting a repeating event says the whole series goes', async () => {
  const { calendar, node } = make({ replies: [events(ev('u1', 'Standup', '2026-11-02', { repeat: 'weekly' }))] });
  await calendar.refresh();
  node('events-list').children[0].byClass('button-danger')[0].click();
  assert.match(isolates(node('events-list').children[0].textContent), /Delete “Standup” and all of its repeats\?/);
});

test('a failed delete shows the error, keeps the page and reads the list again', async () => {
  const gone = make({ replies: [events(ev('u1', 'Review', '2026-11-02')), { kind: 'error', message: 'no event with that id' }, events()] });
  await gone.calendar.refresh();
  gone.node('events-list').children[0].byClass('button-danger')[0].click();
  gone.node('events-list').children[0].byClass('button-danger')[0].click();
  await new Promise((resolve) => setImmediate(resolve));
  assert.strictEqual(isolates(gone.node('events-notice').textContent), 'Something went wrong: no event with that id');
  assert.strictEqual(gone.node('events-notice').getAttribute('role'), 'alert');
  assert.strictEqual(gone.requests.length, 3);

  const down = make({ replies: [events(ev('u1', 'Review', '2026-11-02')), { kind: 'unreachable' }, events(ev('u1', 'Review', '2026-11-02'))] });
  await down.calendar.refresh();
  down.node('events-list').children[0].byClass('button-danger')[0].click();
  down.node('events-list').children[0].byClass('button-danger')[0].click();
  await new Promise((resolve) => setImmediate(resolve));
  assert.strictEqual(down.node('events-notice').textContent, locale('en').get('assistant-unreachable'));
  assert.strictEqual(down.node('events-list').children.length, 1, 'the event is still listed');
});

test('event text is only ever text: no markup is parsed, and the fake DOM would throw on innerHTML', async () => {
  const evil = '<img src=x onerror="alert(1)"><b>hi</b>';
  const { calendar, node } = make({ replies: [events(ev('u1', evil, '2026-10-31', { attendees: [evil] }))] });
  await calendar.refresh();
  assert.strictEqual(node('events-list').children[0].byClass('event-title')[0].textContent, evil);
  assert.strictEqual(node('events-list').children[0].byClass('event-title')[0].dir, 'auto');
  assert.strictEqual(node('events-list').byTag('img').length, 0);
  assert.strictEqual(node('month-grid').byClass('day-events')[0].children[0].textContent, evil);
  assert.throws(() => { node('events-list').innerHTML = evil; });
});

const headers = (node) => node('month-grid').byTag('th').map((th) => th.textContent);
const cells = (node) => node('month-grid').byTag('td');
const dayOf = (td) => td.byClass('day-head')[0].ownText;

test('month grid: week start comes from the stored setting', async () => {
  const names = {
    saturday: ['Saturday', 'Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday'],
    sunday: ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'],
    monday: ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'],
  };
  // 1 November 2026 is a Sunday.
  const firstCell = { saturday: '31', sunday: '1', monday: '26' };
  for (const weekStart of ['saturday', 'sunday', 'monday']) {
    const m = make({ replies: [events()], settings: settingsWith({ weekStart }), today: { year: 2026, month: 11, day: 15 } });
    await m.calendar.refresh();
    assert.deepStrictEqual(headers(m.node), names[weekStart]);
    assert.strictEqual(dayOf(cells(m.node)[0]), firstCell[weekStart]);
    assert.strictEqual(cells(m.node).length % 7, 0);
    assert.strictEqual(isolates(m.node('month-title').textContent), 'November 2026');
  }
});

test('month grid: weekend days are configurable and saved, and follow the week start', async () => {
  const m = make({ replies: [events()], settings: settingsWith({ weekStart: 'sunday' }), today: { year: 2026, month: 11, day: 15 } });
  await m.calendar.refresh();
  const weekendColumns = () => cells(m.node).slice(0, 7).map((td) => td.byClass('day-weekend').length > 0);
  assert.deepStrictEqual(weekendColumns(), [true, false, false, false, false, false, true], 'default: Saturday and Sunday');

  const boxes = () => m.node('weekend-options').byTag('input');
  assert.strictEqual(boxes().length, 7);
  assert.deepStrictEqual(boxes().map((b) => b.checked), [true, false, false, false, false, false, true]);
  boxes()[0].checked = false; boxes()[0].fire('change');
  boxes()[5].checked = true; boxes()[5].fire('change');
  boxes()[6].checked = true; boxes()[6].fire('change');
  assert.deepStrictEqual(m.weekendStore.get(), [5, 6], 'Friday and Saturday');
  assert.deepStrictEqual(weekendColumns(), [false, false, false, false, false, true, true]);
  assert.strictEqual(m.node('month-grid').byTag('th')[5].className, 'day-weekend');
});

test('weekend store: default, saving, bad data and failing storage', () => {
  assert.deepStrictEqual(C.createWeekendStore(() => storageOf()).get(), [0, 6]);
  const storage = storageOf();
  const store = C.createWeekendStore(() => storage);
  assert.strictEqual(store.set([5, 6, 6]), true);
  assert.deepStrictEqual(C.createWeekendStore(() => storage).get(), [5, 6]);
  assert.strictEqual(store.set([]), true);
  assert.deepStrictEqual(C.createWeekendStore(() => storage).get(), [], 'no weekend at all is allowed');
  for (const bad of [[7], [-1], ['5'], [1.5], 'sat', null]) {
    assert.strictEqual(store.set(bad), false);
  }
  assert.deepStrictEqual(store.get(), []);
  for (const raw of ['nope', '{"a":1}', '"x"', '[9,2,2,"a"]']) {
    const got = C.createWeekendStore(() => storageOf({ [C.WEEKEND_KEY]: raw })).get();
    assert.deepStrictEqual(got, raw === '[9,2,2,"a"]' ? [2] : [0, 6]);
  }
  const failing = { getItem() { throw new Error('denied'); }, setItem() { throw new Error('quota'); } };
  for (const getStorage of [() => null, () => { throw new Error('SecurityError'); }, () => failing]) {
    const s = C.createWeekendStore(getStorage);
    assert.deepStrictEqual(s.get(), [0, 6]);
    assert.strictEqual(s.set([4]), true);
    assert.deepStrictEqual(s.get(), [4]);
  }
});

test('month grid: repeats appear on every matching day, plain events once, nothing before the first date', async () => {
  const m = make({
    replies: [events(
      ev('w', 'Weekly', '2026-11-02', { repeat: 'weekly' }),
      ev('d', 'Daily', '2026-11-28', { repeat: 'daily' }),
      ev('o', 'Once', '2026-11-10')
    )],
    today: { year: 2026, month: 11, day: 1 },
  });
  await m.calendar.refresh();
  const marks = (title) => cells(m.node).filter((td) => !td.className.includes('day-outside')).filter((td) => td.byTag('li').some((li) => li.textContent === title)).map((td) => dayOf(td));
  assert.deepStrictEqual(marks('Weekly'), ['2', '9', '16', '23', '30']);
  assert.deepStrictEqual(marks('Daily'), ['28', '29', '30']);
  assert.deepStrictEqual(marks('Once'), ['10']);

  m.calendar.moveMonth(1);
  assert.strictEqual(isolates(m.node('month-title').textContent), 'December 2026');
  const december = cells(m.node).filter((td) => !td.className.includes('day-outside'));
  assert.strictEqual(december.length, 31);
  assert.strictEqual(december.filter((td) => td.byTag('li').some((li) => li.textContent === 'Daily')).length, 31);
  assert.strictEqual(december.filter((td) => td.byTag('li').some((li) => li.textContent === 'Once')).length, 0);

  m.calendar.moveMonth(-1);
  m.calendar.moveMonth(-1);
  m.calendar.moveMonth(-1);
  assert.strictEqual(isolates(m.node('month-title').textContent), 'September 2026');
  assert.strictEqual(m.node('month-grid').byTag('li').length, 0);
  m.calendar.showToday();
  assert.strictEqual(isolates(m.node('month-title').textContent), 'November 2026');
});

test('month grid: January and December wrap the year, today is marked, digits and Hijri follow the settings', async () => {
  const m = make({
    replies: [events()],
    settings: settingsWith({ digits: 'eastern', calendar: 'both', language: 'ar' }),
    lang: 'ar',
    today: { year: 2026, month: 1, day: 1 },
  });
  await m.calendar.refresh();
  assert.strictEqual(isolates(m.node('month-title').textContent), 'يناير ٢٠٢٦');
  const todayCell = cells(m.node).filter((td) => td.getAttribute('aria-current') === 'date');
  assert.strictEqual(todayCell.length, 1);
  assert.strictEqual(todayCell[0].className.includes('day-today'), true);
  assert.strictEqual(dayOf(todayCell[0]), '١');
  const hijri = todayCell[0].byClass('day-hijri')[0];
  assert.strictEqual(hijri.textContent, '١٢', '12 Rajab 1447');
  assert.strictEqual(hijri.title, locale('ar').get('settings-hijri-note'));
  m.calendar.moveMonth(-1);
  assert.strictEqual(isolates(m.node('month-title').textContent), 'ديسمبر ٢٠٢٥');
  m.calendar.moveMonth(1);
  m.calendar.moveMonth(11);
  assert.strictEqual(isolates(m.node('month-title').textContent), 'ديسمبر ٢٠٢٦');
  m.calendar.moveMonth(1);
  assert.strictEqual(isolates(m.node('month-title').textContent), 'يناير ٢٠٢٧');
});

test('pure helpers: dates, weeks and repeats', () => {
  assert.deepStrictEqual(C.parseDate('2026-02-28'), { year: 2026, month: 2, day: 28 });
  for (const bad of ['2026-02-30', '2026-2-3', '', 'x', null, undefined, 20260101]) {
    assert.strictEqual(C.parseDate(bad), null);
  }
  const weeks = C.monthWeeks(2024, 2, 'monday');
  assert.deepStrictEqual(weeks.map((w) => w.length), [7, 7, 7, 7, 7]);
  assert.strictEqual(weeks.flat().filter((c) => c.inMonth).length, 29, '2024 is a leap year');
  assert.deepStrictEqual(weeks[0].map((c) => c.weekday), [1, 2, 3, 4, 5, 6, 0]);
  assert.strictEqual(C.monthWeeks(2026, 11, 'bogus')[0][0].weekday, 0, 'unknown week start falls back to Sunday');
  assert.strictEqual(C.monthWeeks(2026, 2, 'sunday').length, 4, 'a 28-day month starting on the week start');
  const base = C.readEvents([ev('a', 'x', '2026-11-02', { repeat: 'weekly' }), ev('b', 'y', '2026-11-02', { repeat: 'yearly' })]);
  assert.strictEqual(base[0].repeat, 'weekly');
  assert.strictEqual(base[1].repeat, null);
  assert.strictEqual(C.occursOn(base[0], base[0].day + 14), true);
  assert.strictEqual(C.occursOn(base[0], base[0].day + 13), false);
  assert.strictEqual(C.occursOn(base[0], base[0].day - 7), false);
  assert.strictEqual(C.occursOn(base[1], base[1].day + 7), false);
});

test('switching language re-draws from the same data with the new words', async () => {
  const state = { lang: 'en' };
  const document = fakeDocument();
  const catalogs = { en: tFor('en'), ar: tFor('ar') };
  const calendar = C.createCalendar({
    document,
    t: (key) => catalogs[state.lang](key),
    settings: () => settingsWith({ language: state.lang }),
    weekendStore: C.createWeekendStore(() => null),
    today: () => TODAY,
    ask: () => Promise.resolve(events(ev('u', 'Review', '2026-11-02', { repeat: 'daily' }))),
  });
  await calendar.refresh();
  assert.match(isolates(document.byId.get('events-list').textContent), /Repeats every day/);
  state.lang = 'ar';
  calendar.render();
  assert.match(isolates(document.byId.get('events-list').textContent), /يتكرر كل يوم/);
  assert.strictEqual(isolates(document.byId.get('month-title').textContent), 'أكتوبر 2026');
});

// ---- page, strings and styles ----

test('calendar.js builds no HTML from text', () => {
  const source = read('calendar.js');
  assert.doesNotMatch(source, /innerHTML|outerHTML|insertAdjacentHTML|document\.write|createContextualFragment|DOMParser|\beval\(|new Function/);
});

test('calendar.js has no t() call with a non-literal key', () => {
  const calls = read('calendar.js').match(/\bt\(\s*[^'"\s)]/g) || [];
  assert.deepStrictEqual(calls, []);
});

test('every locale key used by calendar.js or calendar.html exists in both languages with the same placeholders', () => {
  const keys = new Set([
    ...[...read('calendar.js').matchAll(/\bt\('([A-Za-z0-9-]+)'\)/g)].map((m) => m[1]),
    ...[...html.matchAll(/data-i18n(?:-[a-z-]+)?="([^"]+)"/g)].map((m) => m[1]),
  ]);
  assert.ok(keys.size > 40);
  const en = locale('en');
  const ar = locale('ar');
  const holes = (text) => [...String(text).matchAll(/\{\s*\$([A-Za-z][A-Za-z0-9_-]*)\s*\}/g)].map((m) => m[1]).sort();
  for (const key of keys) {
    assert.ok(en.has(key) && en.get(key) !== '', `en ${key}`);
    assert.ok(ar.has(key) && ar.get(key) !== '', `ar ${key}`);
    assert.deepStrictEqual(holes(ar.get(key)), holes(en.get(key)), key);
  }
  const added = [...en.keys()].filter((key) => key.startsWith('calendar-'));
  for (const key of added) {
    assert.ok(keys.has(key), `${key} is defined but never used`);
    assert.ok(ar.has(key), `${key} is missing in Arabic`);
  }
});

test('calendar.html has no hard-coded text or plain text attributes', () => {
  const stripped = html.replace(/<!--[\s\S]*?-->/g, '').replace(/<script[\s\S]*?<\/script>/g, '');
  assert.strictEqual(stripped.replace(/<[^>]*>/g, '').trim(), '');
  assert.doesNotMatch(stripped, /\s(placeholder|title|alt|aria-label)=/);
  assert.match(html, /<html lang="en" dir="ltr">/);
});

test('every id calendar.js looks up exists in calendar.html', () => {
  const wanted = [...read('calendar.js').matchAll(/(?:byId|getElementById)\('([^']+)'\)/g)].map((m) => m[1]);
  assert.ok(wanted.length >= 10);
  const ids = [...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]);
  for (const id of wanted) {
    assert.ok(ids.includes(id), id);
  }
  assert.strictEqual(new Set(ids).size, ids.length, 'ids are unique');
});

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
    } else if (/^(float|clear)$/.test(property) && /^(left|right)$/.test(value.trim())) {
      findings.push(`${property}: ${value.trim()}`);
    }
  }
  return findings;
}

test('calendar.css uses no physical left/right properties or values', () => {
  const css = read('calendar.css');
  assert.ok(css.length > 500);
  assert.deepStrictEqual(physicalDirectionFindings(css), []);
});

test('the CSS scanner itself catches physical properties', () => {
  for (const bad of [
    'a { margin-left: 1px }', 'a { padding-right: 1px }', 'a { border-left-width: 1px }', 'a { text-align: left }',
    'a { float: right }', 'a { transform: translateX(4px) }', 'a { margin: 1px 2px 3px 4px }', 'a { left: 0 }',
    'a { border-top-left-radius: 2px }', 'a { inset: 0 1px 2px 3px }',
  ]) {
    assert.ok(physicalDirectionFindings(bad).length > 0, bad);
  }
  assert.deepStrictEqual(physicalDirectionFindings('a { margin-inline-start: 1px; text-align: start; inset-inline-end: 0 }'), []);
  assert.deepStrictEqual(physicalDirectionFindings('/* left: 0 */ a { margin: 1px 2px }'), []);
});

test('every script in calendar.html can load into one global scope', () => {
  const scripts = [...html.matchAll(/<script src="([^"]+)"/g)].map((m) => m[1]);
  assert.deepStrictEqual(scripts, ['i18n.js', 'settings.js', 'calendar.js']);
  const declared = new Map();
  for (const script of scripts) {
    const source = read(script);
    new vm.Script(source, { filename: script });
    for (const [, name] of source.matchAll(/^(?:const|let|class|var|function\*?|async function)\s+([A-Za-z_$][\w$]*)/gm)) {
      assert.ok(!declared.has(name), `${name} is declared in both ${declared.get(name)} and ${script}`);
      declared.set(name, script);
    }
  }
  assert.deepStrictEqual([...declared].filter(([, script]) => script === 'calendar.js'), [], 'calendar.js adds no top-level names');
});

test('the page boots from the three classic scripts in one context and lists the events', async () => {
  const document = fakeDocument();
  const requests = [];
  const sandbox = {
    document,
    window: { addEventListener() {} },
    console,
    setTimeout,
    fetch(url, init) {
      if (url.startsWith('../../locales/')) {
        return Promise.resolve({ status: 200, text: () => Promise.resolve(read('..', '..', url.slice('../../'.length))) });
      }
      requests.push([url, JSON.parse(init.body)]);
      const body = JSON.stringify(events(ev('u1', 'Review', '2026-11-02', { repeat: 'weekly' })));
      return Promise.resolve({ status: 200, text: () => Promise.resolve(body) });
    },
  };
  sandbox.globalThis = sandbox;
  const context = vm.createContext(sandbox);
  for (const script of ['i18n.js', 'settings.js', 'calendar.js']) {
    vm.runInContext(read(script), context, { filename: script });
  }
  for (let i = 0; i < 10; i += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
  assert.strictEqual(requests.length, 1);
  assert.strictEqual(requests[0][0], '/chat');
  assert.deepStrictEqual(requests[0][1].upcoming, true);
  assert.match(requests[0][1].from, /^\d{4}-\d{2}-\d{2}$/);
  assert.strictEqual(document.byId.get('events-list').children.length, 1);
  assert.strictEqual(document.documentElement.dir, 'ltr');
});
