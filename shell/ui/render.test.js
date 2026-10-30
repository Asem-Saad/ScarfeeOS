'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const { parseFtl } = require('./i18n.js');
const { fill, formatElapsed, formatTime, describeTool, describeResponse, responseFromHttp } = require('./render.js');

const read = (...parts) => fs.readFileSync(path.join(__dirname, ...parts), 'utf8');
const catalogs = {
  en: parseFtl(read('..', '..', 'locales', 'en', 'scarfeeos.ftl')),
  ar: parseFtl(read('..', '..', 'locales', 'ar', 'scarfeeos.ftl')),
};
const translator = (lang) => (key) => (catalogs[lang].has(key) ? catalogs[lang].get(key) : key);
const plain = (text) => text.replace(/[\u2068\u2069]/g, '');
const render = (response, lang = 'en') => describeResponse(response, translator(lang), lang).map((b) => ({ ...b, text: plain(b.text) }));
const texts = (response, lang = 'en') => render(response, lang).map((b) => b.text);

const REMINDER = { text: 'call the bank', time: '2026-10-31T10:00:00', repeat: 'daily' };
const MEETING = { title: 'Review', time: '2026-11-01T14:00:00', attendees: ['Sara', 'Omar'], repeat: null };
const EVERY_KIND = [
  { kind: 'reply', text: 'Hello', foreign_words: [{ word: 'привет', script: 'cyrillic' }] },
  { kind: 'time', date: '2026-10-30', hour: 9, minute: 5, weekday: 'Friday' },
  { kind: 'unsupported_time_zone', zone: 'Tokyo' },
  { kind: 'cannot_do_offline', what: 'news' },
  { kind: 'cannot_do_offline', what: 'weather' },
  { kind: 'cannot_do_offline', what: 'time' },
  { kind: 'declined' },
  { kind: 'hijri_approx', gregorian: '2026-10-30', year: 1448, month: 5, day: 18, month_name_en: 'Jumada al-Awwal', month_name_ar: 'جمادى الأولى', approximate: true },
  { kind: 'missing_date' },
  { kind: 'run', tool: 'open_app', arguments: { name: 'calculator' } },
  { kind: 'run', tool: 'create_reminder', arguments: REMINDER },
  { kind: 'run', tool: 'schedule_meeting', arguments: MEETING },
  { kind: 'run', tool: 'list_files', arguments: { path: '/home/user' } },
  { kind: 'run', tool: 'search_files', arguments: { query: 'report' } },
  { kind: 'run', tool: 'current_time', arguments: { timezone: null } },
  { kind: 'run', tool: 'set_language', arguments: { lang: 'ar' } },
  { kind: 'needs_clarification', tool: 'create_reminder', arguments: { text: 'x', time: '2026-10-30T11:00:00', repeat: null } },
  { kind: 'needs_confirmation', tool: 'set_language', arguments: { lang: 'ar' } },
  { kind: 'missing_time', tool: 'create_reminder', arguments: { text: 'call the bank', time: '', repeat: null } },
  { kind: 'error', message: 'local provider: request failed' },
  { kind: 'unreachable' },
  { kind: 'other', debug: 'Something(1)' },
  { kind: 'a_kind_from_the_future' },
];

test('every kind renders through locale text in both languages, with nothing left unfilled', () => {
  for (const lang of ['en', 'ar']) {
    for (const response of EVERY_KIND) {
      const blocks = render(response, lang);
      assert.ok(blocks.length > 0, `${response.kind} gave no blocks`);
      for (const block of blocks) {
        if (response.kind === 'reply' && block.tone === 'reply') continue;
        assert.doesNotMatch(block.text, /assistant-|language-|\{\s*\$/, `${lang} ${response.kind}: ${block.text}`);
        assert.ok(block.text.trim(), `${lang} ${response.kind} has an empty block`);
      }
    }
  }
});

test('every message has the same placeholders in en and ar', () => {
  const placeholders = (lang, key) => [...catalogs[lang].get(key).matchAll(/\{\s*\$([A-Za-z0-9_-]+)\s*\}/g)].map((m) => m[1]).sort();
  for (const key of catalogs.en.keys()) {
    assert.deepStrictEqual(placeholders('ar', key), placeholders('en', key), key);
  }
});

test('model text is passed through as plain text, never interpreted', () => {
  const evil = '<img src=x onerror=alert(1)> & <b>bold</b>';
  assert.deepStrictEqual(render({ kind: 'reply', text: evil, foreign_words: [] }), [{ tone: 'reply', text: evil }]);
  assert.ok(texts({ kind: 'run', tool: 'open_app', arguments: { name: evil } })[0].includes(evil));
  assert.ok(texts({ kind: 'error', message: evil })[0].includes(evil));
});

test('no source file builds HTML from text', () => {
  for (const file of ['panel.js', 'render.js']) {
    assert.doesNotMatch(read(file), /innerHTML|outerHTML|insertAdjacentHTML|document\.write|eval\(|new Function/, file);
  }
});

test('reply: plain reply has one block, foreign words add a visible warning naming them', () => {
  assert.deepStrictEqual(render({ kind: 'reply', text: 'Hi', foreign_words: [] }), [{ tone: 'reply', text: 'Hi' }]);
  const blocks = render({ kind: 'reply', text: 'تم فتح الملف привет 你好', foreign_words: [{ word: 'привет', script: 'cyrillic' }, { word: '你好', script: 'cjk' }] }, 'ar');
  assert.strictEqual(blocks.length, 2);
  assert.strictEqual(blocks[0].text, 'تم فتح الملف привет 你好');
  assert.strictEqual(blocks[1].tone, 'warning');
  assert.match(blocks[1].text, /^تنبيه:/);
  assert.ok(blocks[1].text.includes('привет') && blocks[1].text.includes('你好'));
  assert.ok(blocks[1].text.includes('،'), 'Arabic list separator');
  assert.match(render({ kind: 'reply', text: 'Hi', foreign_words: [{ word: 'Привет', script: 'cyrillic' }] })[1].text, /^Warning:.*Привет/);
});

test('time: weekday and sentence follow the language, digits are padded', () => {
  const time = { kind: 'time', date: '2026-10-30', hour: 9, minute: 5, weekday: 'Friday' };
  assert.strictEqual(texts(time)[0], 'The time on this device is 09:05, on Friday 2026-10-30.');
  assert.strictEqual(texts(time, 'ar')[0], 'الوقت على هذا الجهاز 09:05، يوم الجمعة بتاريخ 2026-10-30.');
  assert.match(texts({ ...time, weekday: 'Sunday' }, 'ar')[0], /الأحد/);
  assert.match(texts({ ...time, weekday: 'Funday' })[0], /Funday/);
});

test('unsupported_time_zone and cannot_do_offline say what is not possible', () => {
  assert.match(texts({ kind: 'unsupported_time_zone', zone: 'Tokyo' })[0], /this device.*Tokyo/);
  assert.match(texts({ kind: 'cannot_do_offline', what: 'news' })[0], /news/);
  assert.match(texts({ kind: 'cannot_do_offline', what: 'weather' })[0], /weather/);
  assert.match(texts({ kind: 'cannot_do_offline', what: 'weather' }, 'ar')[0], /الطقس/);
  assert.notStrictEqual(texts({ kind: 'cannot_do_offline', what: 'bogus' })[0], texts({ kind: 'cannot_do_offline', what: 'news' })[0]);
});

test('declined is a plain refusal', () => {
  assert.deepStrictEqual(render({ kind: 'declined' }), [{ tone: 'info', text: 'Sorry, ScarfeeOS does not answer religious questions.' }]);
});

test('hijri_approx shows the date, the month name in the current language, and that it is approximate', () => {
  const hijri = { kind: 'hijri_approx', gregorian: '2026-01-01', year: 1447, month: 7, day: 12, month_name_en: 'Rajab', month_name_ar: 'رجب', approximate: true };
  const en = render(hijri);
  assert.strictEqual(en[0].text, '2026-01-01 is about 12 Rajab 1447 AH.');
  assert.deepStrictEqual([en[1].tone, /approximate/.test(en[1].text)], ['warning', true]);
  const ar = render(hijri, 'ar');
  assert.strictEqual(ar[0].text, 'يوافق 2026-01-01 نحو 12 رجب 1447 هـ.');
  assert.match(ar[1].text, /تقريبي/);
  assert.strictEqual(render({ ...hijri, approximate: undefined }).length, 2, 'a missing flag is treated as approximate');
  assert.strictEqual(render({ ...hijri, approximate: false }).length, 1);
});

test('missing_date and missing_time ask for the missing piece', () => {
  assert.deepStrictEqual(render({ kind: 'missing_date' }).map((b) => b.tone), ['question']);
  assert.match(texts({ kind: 'missing_date' })[0], /Gregorian date in digits/);
  assert.deepStrictEqual(render({ kind: 'missing_time', tool: 'create_reminder', arguments: {} }).map((b) => b.tone), ['question']);
  assert.match(texts({ kind: 'missing_time' })[0], /date and a time/);
});

test('run says what would happen and that nothing was done', () => {
  const blocks = render({ kind: 'run', tool: 'open_app', arguments: { name: 'calculator' } });
  assert.deepStrictEqual(blocks, [
    { tone: 'info', text: 'I understood your request as: open the app calculator.' },
    { tone: 'note', text: 'Nothing was done. Actions are not connected to this panel yet.' },
  ]);
  const ar = render({ kind: 'run', tool: 'open_app', arguments: { name: 'calculator' } }, 'ar');
  assert.strictEqual(ar[0].text, 'فهمتُ طلبك على أنه: فتح تطبيق calculator.');
  assert.match(ar[1].text, /^لم يُنفَّذ أي شيء/);
  for (const response of EVERY_KIND.filter((r) => ['run', 'needs_clarification', 'needs_confirmation'].includes(r.kind))) {
    assert.ok(render(response).some((b) => b.tone === 'note' && /Nothing was done/.test(b.text)), `${response.kind} ${response.tool}`);
    assert.ok(!render(response).some((b) => /\b(done|opened|created|scheduled) (it|successfully)\b/i.test(b.text)));
  }
});

test('needs_confirmation and needs_clarification show the question and no more', () => {
  const confirm = render({ kind: 'needs_confirmation', tool: 'set_language', arguments: { lang: 'ar' } });
  assert.strictEqual(confirm[0].tone, 'question');
  assert.strictEqual(confirm[0].text, 'This changes a setting. Do you want to go ahead: change the system language to العربية?');
  const clarify = render({ kind: 'needs_clarification', tool: 'create_reminder', arguments: { text: 'x', time: '2026-10-30T11:00:00', repeat: null } });
  assert.strictEqual(clarify[0].tone, 'question');
  assert.strictEqual(clarify[0].text, 'The time could mean two things. Is this what you meant: create the reminder “x” at 2026-10-30 11:00?');
  assert.strictEqual(confirm.length + clarify.length, 4);
});

test('tool descriptions: every tool, repeat and attendees, in both languages', () => {
  const t = (lang) => (tool, args) => plain(describeTool(tool, args, translator(lang)));
  assert.strictEqual(t('en')('create_reminder', REMINDER), 'create the reminder “call the bank” at 2026-10-31 10:00, repeating every day');
  assert.strictEqual(t('ar')('create_reminder', REMINDER), 'إنشاء تذكير «call the bank» في 2026-10-31 10:00، ويتكرر كل يوم');
  assert.strictEqual(t('en')('schedule_meeting', MEETING), 'schedule the meeting “Review” at 2026-11-01 14:00, with Sara, Omar');
  assert.strictEqual(t('ar')('schedule_meeting', MEETING), 'جدولة اجتماع «Review» في 2026-11-01 14:00، مع Sara، Omar');
  assert.strictEqual(t('en')('schedule_meeting', { ...MEETING, attendees: [], repeat: 'weekly' }), 'schedule the meeting “Review” at 2026-11-01 14:00, repeating every week');
  assert.strictEqual(t('en')('list_files', { path: '/home/user' }), 'list the files in /home/user');
  assert.strictEqual(t('ar')('search_files', { query: 'report' }), 'البحث في الملفات عن report');
  assert.strictEqual(t('en')('current_time', {}), 'get the current time');
  assert.strictEqual(t('en')('set_language', { lang: 'en' }), 'change the system language to English');
  assert.strictEqual(t('en')('no_such_tool', {}), 'run the tool no_such_tool');
  assert.strictEqual(t('en')('open_app', undefined), 'open the app ');
});

test('error, unreachable and other', () => {
  assert.deepStrictEqual(render({ kind: 'error', message: 'boom' }), [{ tone: 'error', text: 'Something went wrong: boom' }]);
  assert.deepStrictEqual(render({ kind: 'error', message: 'boom' }, 'ar'), [{ tone: 'error', text: 'حدث خطأ: boom' }]);
  assert.strictEqual(render({ kind: 'unreachable' })[0].tone, 'error');
  assert.deepStrictEqual(render({ kind: 'other', debug: 'Foo(1)' }).map((b) => b.text), [
    'The assistant answered in a way this panel cannot show yet.',
    'Details: Foo(1)',
  ]);
  assert.strictEqual(render({ kind: 'brand_new' }).length, 1);
});

test('garbage input never throws', () => {
  for (const value of [null, undefined, 5, 'text', [], {}, { kind: 'run' }, { kind: 'reply' }, { kind: 'time' }, { kind: 'hijri_approx' }]) {
    assert.ok(Array.isArray(describeResponse(value, translator('en'), 'en')));
  }
});

test('fill replaces named placeholders, isolates values, and blanks missing ones', () => {
  assert.strictEqual(plain(fill('A { $x } and {$y}.', { x: 'one', y: 2 })), 'A one and 2.');
  assert.strictEqual(fill('{ $x }', { x: 'مرحبا' }), '\u2068مرحبا\u2069');
  assert.strictEqual(plain(fill('[{ $x }]', {})), '[]');
  assert.strictEqual(plain(fill('[{ $x }]', { x: null })), '[]');
  assert.strictEqual(plain(fill('[{ $toString }]', {})), '[]');
  assert.strictEqual(fill('no placeholders', { x: 1 }), 'no placeholders');
});

test('elapsed time and ISO times are formatted', () => {
  assert.deepStrictEqual([0, 7, 59.9, 60, 75, 3600, -3, NaN].map(formatElapsed), ['0:00', '0:07', '0:59', '1:00', '1:15', '60:00', '0:00', '0:00']);
  assert.strictEqual(formatTime('2026-11-01T14:00:00'), '2026-11-01 14:00');
  assert.strictEqual(formatTime(''), '');
  assert.strictEqual(formatTime('tomorrow'), 'tomorrow');
});

test('responseFromHttp takes a response body from the core or the bridge, else an error', () => {
  assert.deepStrictEqual(responseFromHttp(200, '{"id":1,"kind":"time"}'), { id: 1, kind: 'time' });
  assert.deepStrictEqual(responseFromHttp(502, '{"kind":"error","message":"core stopped"}'), { kind: 'error', message: 'core stopped' });
  for (const body of ['', 'not json', '[]', 'null', '{"kind":5}', '{}', '<html>']) {
    assert.deepStrictEqual(responseFromHttp(500, body), { kind: 'error', message: 'HTTP 500' }, body);
  }
});
