'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { parseFtl } = require('./i18n.js');
const { fill, formatElapsed, formatTime, describeTool, describeResponse, responseFromHttp } = require('./render.js');

const read = (...parts) => fs.readFileSync(path.join(__dirname, ...parts), 'utf8');
const catalogs = {
  en: parseFtl(read('..', '..', 'locales', 'en', 'scarfeeos.ftl')),
  ar: parseFtl(read('..', '..', 'locales', 'ar', 'scarfeeos.ftl')),
};
const translator = (lang) => (key) => (catalogs[lang].has(key) ? catalogs[lang].get(key) : key);
const plain = (text) => text.replace(/[\u2068\u2069]/g, '');
const render = (response, lang = 'en', settings) =>
  describeResponse(response, translator(lang), lang, settings).map((b) => (b.text === undefined ? b : { ...b, text: plain(b.text) }));
const texts = (response, lang = 'en', settings) => render(response, lang, settings).filter((b) => b.text !== undefined).map((b) => b.text);
const run = (tool, args, result) => ({ kind: 'run', tool, arguments: args, result });
const failure = (code, extra = {}) => ({ ok: false, error: { code, message: `core says ${code}`, ...extra } });

const REMINDER = { text: 'call the bank', time: '2026-10-31T10:00:00', repeat: 'daily' };
const MEETING = { title: 'Review', time: '2026-11-01T14:00:00', attendees: ['Sara', 'Omar'], repeat: null };
const EVENT = { uid: 'u1', summary: 'call the bank', start: '2026-10-31T10:00:00', attendees: [], repeat: 'daily' };
const ERROR_CODES = [
  'root_unusable', 'not_found', 'outside_root', 'not_a_directory', 'unreadable', 'empty_query', 'empty_name',
  'launch_failed', 'unsupported', 'calendar_io', 'calendar_malformed', 'not_an_event', 'invalid_time', 'empty_text',
];
const LISTING = {
  ok: true, type: 'listing', path: '/home/user/Documents', truncated: false,
  entries: [{ path: '/home/user/Documents/Reports', kind: 'dir' }, { path: '/home/user/Documents/تقرير 2026.txt', kind: 'file' }],
};
const SEARCH = {
  ok: true, type: 'search', truncated: true, unreadable: 2,
  matches: [{ path: '/home/user/Documents/report-2026.pdf', kind: 'file' }, { path: '/home/user/Downloads/تقرير', kind: 'dir' }],
};
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
  run('open_app', { name: 'calculator' }, { ok: true, type: 'opened', app: { id: 'calc.desktop', name: 'Calculator' } }),
  run('create_reminder', REMINDER, { ok: true, type: 'event', event: EVENT }),
  run('schedule_meeting', MEETING, { ok: true, type: 'event', event: { ...EVENT, attendees: ['Sara', 'Omar'], repeat: null } }),
  run('list_files', { path: '/home/user' }, LISTING),
  run('list_files', { path: '/home/user/Empty' }, { ok: true, type: 'listing', path: '/home/user/Empty', entries: [], truncated: false }),
  run('search_files', { query: 'report' }, SEARCH),
  run('search_files', { query: 'nothing' }, { ok: true, type: 'search', matches: [], truncated: false, unreadable: 0 }),
  run('set_language', { lang: 'ar' }, { ok: true, type: 'language', lang: 'ar' }),
  run('open_app', { name: 'calc' }, failure('app_not_found', { closest: ['Calculator', 'Calendar'] })),
  ...ERROR_CODES.map((code) => run('list_files', { path: '/x' }, failure(code))),
  run('open_app', { name: 'x' }, failure('a_code_from_the_future')),
  { kind: 'cancelled' },
  { kind: 'needs_clarification', tool: 'create_reminder', arguments: { text: 'x', time: '2026-10-30T11:00:00', repeat: null }, action_id: 'a'.repeat(32) },
  { kind: 'needs_confirmation', tool: 'set_language', arguments: { lang: 'ar' }, action_id: 'b'.repeat(32) },
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
        if (block.tone === 'files') {
          assert.ok(block.items.length > 0 && block.items.every((item) => item.name.trim()), `${lang} ${response.kind} files`);
          continue;
        }
        if (block.tone === 'actions') {
          assert.ok(/^[ab]{32}$/.test(block.actionId));
          continue;
        }
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

test('run without a usable result only restates the request and claims nothing', () => {
  const ar = render({ kind: 'run', tool: 'open_app', arguments: { name: 'calculator' } }, 'ar');
  assert.deepStrictEqual(render({ kind: 'run', tool: 'open_app', arguments: { name: 'calculator' } }), [
    { tone: 'info', text: 'I understood your request as: open the app calculator.' },
  ]);
  assert.strictEqual(ar[0].text, 'فهمتُ طلبك على أنه: فتح تطبيق calculator.');
  for (const result of [null, 'ok', {}, { ok: true }, { ok: true, type: 'from_the_future' }, { ok: 'yes', type: 'opened' }]) {
    assert.deepStrictEqual(texts(run('open_app', { name: 'calculator' }, result)), ['I understood your request as: open the app calculator.']);
  }
  for (const lang of ['en', 'ar']) {
    for (const key of catalogs[lang].keys()) assert.notStrictEqual(key, 'assistant-not-connected');
    for (const response of EVERY_KIND) assert.ok(!texts(response, lang).some((text) => /not connected|غير متصلة|Nothing was done\. Actions/.test(text)));
  }
});

test('opened app and language change say what happened', () => {
  const opened = run('open_app', { name: 'calc' }, { ok: true, type: 'opened', app: { id: 'calc.desktop', name: 'Calculator' } });
  assert.deepStrictEqual(render(opened), [{ tone: 'info', text: 'Opened Calculator.' }]);
  assert.deepStrictEqual(render(opened, 'ar'), [{ tone: 'info', text: 'تم فتح Calculator.' }]);
  const language = run('set_language', { lang: 'ar' }, { ok: true, type: 'language', lang: 'ar' });
  assert.deepStrictEqual(render(language), [{ tone: 'info', text: 'The system language is now العربية.' }]);
  assert.deepStrictEqual(render(language, 'ar'), [{ tone: 'info', text: 'أصبحت لغة النظام الآن العربية.' }]);
});

test('event created shows its summary and time, with repeat and attendees, in the user settings', () => {
  const made = run('create_reminder', REMINDER, { ok: true, type: 'event', event: EVENT });
  assert.deepStrictEqual(texts(made), ['Event created: “call the bank” at 2026-10-31 10:00, repeating every day.']);
  assert.deepStrictEqual(texts(made, 'ar'), ['تم إنشاء الحدث «call the bank» في 2026-10-31 10:00، ويتكرر كل يوم.']);
  assert.match(texts(made, 'en', { digits: 'eastern', calendar: 'both' })[0], /at ٢٠٢٦-١٠-٣١ \(about .*\) ١٠:٠٠,/);
  const meeting = run('schedule_meeting', MEETING, { ok: true, type: 'event', event: { ...EVENT, summary: 'Review', start: '2026-11-01T14:00:00', attendees: ['Sara', 'Omar'], repeat: null } });
  assert.deepStrictEqual(texts(meeting), ['Event created: “Review” at 2026-11-01 14:00, with Sara, Omar.']);
  const allDay = run('create_reminder', REMINDER, { ok: true, type: 'event', event: { ...EVENT, start: '2026-11-02', repeat: null } });
  assert.deepStrictEqual(texts(allDay), ['Event created: “call the bank” at 2026-11-02.']);
});

test('a listing shows names as plain text blocks, with kinds, and says when it is empty', () => {
  const blocks = render(run('list_files', { path: '/home/user/Documents' }, LISTING));
  assert.deepStrictEqual(blocks, [
    { tone: 'info', text: 'The contents of /home/user/Documents:' },
    { tone: 'files', items: [{ name: 'Reports', label: 'folder' }, { name: 'تقرير 2026.txt', label: null }] },
  ]);
  const ar = render(run('list_files', { path: '/home/user/Documents' }, LISTING), 'ar');
  assert.strictEqual(ar[0].text, 'محتويات /home/user/Documents:');
  assert.strictEqual(ar[1].items[0].label, 'مجلد');
  assert.deepStrictEqual(texts(run('list_files', { path: '/e' }, { ok: true, type: 'listing', path: '/e', entries: [], truncated: false })), ['/e is empty.']);
  const kinds = render(run('list_files', { path: '/d' }, { ok: true, type: 'listing', path: '/d', truncated: false, entries: [
    { path: '/d/l', kind: 'symlink' }, { path: '/d/o', kind: 'other' }, { path: '/d/', kind: 'file' }, { path: '/', kind: 'file' }] }));
  assert.deepStrictEqual(kinds[1].items, [
    { name: 'l', label: 'link' }, { name: 'o', label: 'other' }, { name: 'd', label: null }, { name: '/', label: null }]);
});

test('a search shows full paths, and says when nothing matched, results were cut short or folders were unreadable', () => {
  const blocks = render(run('search_files', { query: 'report' }, SEARCH));
  assert.deepStrictEqual(blocks.map((b) => b.tone), ['info', 'files', 'note', 'note']);
  assert.strictEqual(blocks[0].text, 'Files matching “report”:');
  assert.deepStrictEqual(blocks[1].items.map((i) => i.name), ['/home/user/Documents/report-2026.pdf', '/home/user/Downloads/تقرير']);
  assert.match(blocks[2].text, /may be more/);
  assert.strictEqual(blocks[3].text, 'Folders that could not be read: 2.');
  assert.match(texts(run('search_files', { query: 'report' }, SEARCH), 'ar')[1], /قد توجد نتائج أكثر/);
  assert.deepStrictEqual(texts(run('search_files', { query: 'zzz' }, { ok: true, type: 'search', matches: [], truncated: false, unreadable: 0 })), ['No files matched “zzz”.']);
});

test('at most 20 files are shown, and the rest are counted', () => {
  const matches = Array.from({ length: 125 }, (_, i) => ({ path: `/d/file ${i}`, kind: 'file' }));
  const blocks = render(run('search_files', { query: 'file' }, { ok: true, type: 'search', matches, truncated: true, unreadable: 0 }));
  assert.strictEqual(blocks[1].items.length, 20);
  assert.strictEqual(blocks[1].items[19].name, '/d/file 19');
  assert.strictEqual(blocks[2].text, '105 more not shown.');
  assert.strictEqual(texts(run('search_files', { query: 'file' }, { ok: true, type: 'search', matches: matches.slice(0, 21) }), 'en', { digits: 'eastern' })[1], '١ more not shown.');
  assert.strictEqual(render(run('search_files', { query: 'file' }, { ok: true, type: 'search', matches: matches.slice(0, 20) }))[1].items.length, 20);
  assert.strictEqual(render(run('search_files', { query: 'file' }, { ok: true, type: 'search', matches: matches.slice(0, 20) })).length, 2);
});

test('the digits setting never changes a file name, a path or a query', () => {
  const entries = [{ path: '/home/u/report 2026-10.txt', kind: 'file' }, { path: '/home/u/٢٠٢٦ ملف 2026', kind: 'file' }];
  for (const digits of ['western', 'eastern']) {
    const settings = { digits, calendar: 'gregorian' };
    const blocks = render(run('search_files', { query: 'q 2026 ٢٠٢٦' }, { ok: true, type: 'search', matches: entries, truncated: false, unreadable: 0 }), 'en', settings);
    assert.deepStrictEqual(blocks[1].items.map((i) => i.name), entries.map((e) => e.path));
    assert.strictEqual(blocks[0].text, 'Files matching “q 2026 ٢٠٢٦”:');
    const listing = render(run('list_files', { path: '/home/u/2026' }, { ok: true, type: 'listing', path: '/home/u/2026', truncated: false, entries }), 'en', settings);
    assert.strictEqual(listing[0].text, 'The contents of /home/u/2026:');
    assert.deepStrictEqual(listing[1].items.map((i) => i.name), ['report 2026-10.txt', '٢٠٢٦ ملف 2026']);
  }
});

test('errors are told by error.code through locale text, in both languages, and the closest apps are named', () => {
  const english = {
    root_unusable: 'The folder I am allowed to use for files cannot be opened.',
    not_found: 'I could not find that file or folder.',
    outside_root: 'That location is outside the folders I am allowed to use.',
    not_a_directory: 'That is not a folder.',
    unreadable: 'I could not read that folder.',
    empty_query: 'The search was empty. What should I look for?',
    empty_name: 'No app name was given. Which app should I open?',
    launch_failed: 'The app was found but could not be started.',
    unsupported: "I can't do that yet.",
    calendar_io: 'The calendar could not be read or saved.',
    calendar_malformed: 'The calendar file is damaged, so I left it unchanged.',
    not_an_event: 'That calendar entry is not an event.',
    invalid_time: 'That time is not valid.',
    empty_text: 'The text is empty, so nothing was saved.',
  };
  assert.deepStrictEqual(Object.keys(english).sort(), [...ERROR_CODES].sort());
  for (const code of ERROR_CODES) {
    assert.deepStrictEqual(render(run('list_files', {}, failure(code))), [{ tone: 'error', text: english[code] }], code);
    const ar = render(run('list_files', {}, failure(code)), 'ar');
    assert.strictEqual(ar.length, 1);
    assert.match(ar[0].text, /[؀-ۿ]/, code);
    assert.ok(!ar[0].text.includes('core says'), 'the core\'s English message is not shown for a known code');
  }
  const missing = run('open_app', { name: 'calc' }, failure('app_not_found', { closest: ['Calculator', 'Calendar'] }));
  assert.deepStrictEqual(render(missing), [
    { tone: 'error', text: 'I could not find an app called calc.' },
    { tone: 'note', text: 'Closest app names: Calculator, Calendar' },
  ]);
  assert.deepStrictEqual(texts(missing, 'ar'), ['لم أجد تطبيقًا باسم calc.', 'أقرب أسماء التطبيقات: Calculator، Calendar']);
  assert.strictEqual(render(run('open_app', { name: 'zzz' }, failure('app_not_found'))).length, 1);
  assert.deepStrictEqual(render(run('open_app', {}, failure('something_new'))), [{ tone: 'error', text: 'Something went wrong: core says something_new' }]);
  assert.deepStrictEqual(render(run('open_app', {}, { ok: false })), [{ tone: 'error', text: 'Something went wrong: ' }]);
});

test('needs_confirmation and needs_clarification ask, and carry what the Confirm and Cancel buttons need', () => {
  const confirm = render({ kind: 'needs_confirmation', tool: 'set_language', arguments: { lang: 'ar' }, action_id: 'abc' });
  assert.deepStrictEqual(confirm, [
    { tone: 'question', text: 'This changes a setting. Do you want to go ahead: change the system language to العربية?' },
    { tone: 'actions', actionId: 'abc', clarify: null },
  ]);
  const clarify = render({ kind: 'needs_clarification', tool: 'create_reminder', arguments: { text: 'x', time: '2026-10-30T11:00:00', repeat: null }, action_id: 'def' });
  assert.strictEqual(clarify[0].text, 'The time could mean two things. Is this what you meant: create the reminder “x” at 2026-10-30 11:00?');
  assert.deepStrictEqual(clarify[1], { tone: 'actions', actionId: 'def', clarify: { date: '2026-10-30', time: '11:00' } });
  assert.strictEqual(render({ kind: 'needs_clarification', tool: 'create_reminder', arguments: { time: '2026-10-30' }, action_id: 'd' })[1].clarify, null);
  assert.strictEqual(render({ kind: 'needs_clarification', tool: 'create_reminder', arguments: { time: '2026-02-31T11:00' }, action_id: 'd' })[1].clarify, null);
  assert.strictEqual(render({ kind: 'needs_clarification', tool: 'create_reminder', arguments: { time: '2026-10-30T11:00' }, action_id: 'd' })[1].clarify.time, '11:00');
  for (const action_id of [undefined, '', 5]) {
    assert.deepStrictEqual(render({ kind: 'needs_confirmation', tool: 'set_language', arguments: { lang: 'ar' }, action_id }).map((b) => b.tone), ['question']);
  }
});

test('cancelled says that nothing was done', () => {
  assert.deepStrictEqual(render({ kind: 'cancelled' }), [{ tone: 'info', text: 'Cancelled. Nothing was done.' }]);
  assert.deepStrictEqual(render({ kind: 'cancelled' }, 'ar'), [{ tone: 'info', text: 'تم الإلغاء. لم يُنفَّذ أي شيء.' }]);
});

test('render.js works as a classic script in a shared global scope, without require', () => {
  const context = vm.createContext({ console });
  for (const file of ['i18n.js', 'settings.js', 'render.js']) {
    vm.runInContext(read(file), context, { filename: file });
  }
  const t = translator('en');
  const blocks = vm.runInContext('ScarfeeRender.describeResponse', context)(run('open_app', {}, { ok: true, type: 'opened', app: { name: 'Calculator' } }), t, 'en', { digits: 'western' });
  assert.strictEqual(plain(blocks[0].text), 'Opened Calculator.');
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
