// Pure functions that turn a core response (see core/src/ipc.rs) into blocks of locale text for the panel.
// No DOM here. Every block is plain text: the panel must show it with textContent, never as HTML.
'use strict';

const Settings = typeof require === 'function' ? require('./settings.js') : globalThis.ScarfeeSettings;

const FSI = '\u2068';
const PDI = '\u2069';
const PLACEHOLDER = /\{\s*\$([A-Za-z][A-Za-z0-9_-]*)\s*\}/g;
const ISO_TIME = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})(?::\d{2})?$/;
const ISO_DATE = /^(\d{4})-(\d{2})-(\d{2})$/;
const MAX_LISTED = 20;

// Fills `{ $name }` the way Fluent does. Each value is isolated so Arabic text, names and numbers
// cannot reorder the words around them.
function fill(template, values) {
  return String(template).replace(PLACEHOLDER, (_, name) => {
    const value = values && Object.prototype.hasOwnProperty.call(values, name) ? values[name] : '';
    return FSI + String(value === null || value === undefined ? '' : value) + PDI;
  });
}

// Digits and calendar come from the user's settings. Only numbers made here are shaped: model text, names,
// paths and URLs reach the screen through fill() untouched.
function digitsOf(settings) {
  return settings && settings.digits === 'eastern' ? 'eastern' : 'western';
}

function shapeNumber(value, settings) {
  return typeof value === 'number' ? Settings.shapeDigits(value, digitsOf(settings)) : value;
}

function formatElapsed(seconds, settings) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0));
  return Settings.shapeDigits(`${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`, digitsOf(settings));
}

function hijriMonthName(month, t) {
  switch (month) {
    case 1: return t('hijri-month-muharram');
    case 2: return t('hijri-month-safar');
    case 3: return t('hijri-month-rabi-al-awwal');
    case 4: return t('hijri-month-rabi-al-thani');
    case 5: return t('hijri-month-jumada-al-awwal');
    case 6: return t('hijri-month-jumada-al-thani');
    case 7: return t('hijri-month-rajab');
    case 8: return t('hijri-month-shaban');
    case 9: return t('hijri-month-ramadan');
    case 10: return t('hijri-month-shawwal');
    case 11: return t('hijri-month-dhu-al-qidah');
    default: return t('hijri-month-dhu-al-hijjah');
  }
}

// A Gregorian ISO date (YYYY-MM-DD), with the approximate Hijri date after it when the user asked for both.
// Anything that is not a real date is returned as it came.
function formatDate(iso, settings, t) {
  const match = ISO_DATE.exec(String(iso));
  if (!match || !Settings.isGregorianDate(Number(match[1]), Number(match[2]), Number(match[3]))) {
    return String(iso);
  }
  const digits = digitsOf(settings);
  const gregorian = Settings.shapeDigits(match[0], digits);
  if (!settings || settings.calendar !== 'both' || typeof t !== 'function') {
    return gregorian;
  }
  const hijri = Settings.toHijriApprox(Number(match[1]), Number(match[2]), Number(match[3]));
  return fill(t('settings-date-both'), {
    gregorian,
    day: Settings.shapeDigits(hijri.day, digits),
    month: hijriMonthName(hijri.month, t),
    year: Settings.shapeDigits(hijri.year, digits),
  });
}

function formatTime(iso, settings, t) {
  const match = ISO_TIME.exec(String(iso));
  return match ? `${formatDate(match[1], settings, t)} ${Settings.shapeDigits(match[2], digitsOf(settings))}` : String(iso);
}

function joinList(items, t) {
  return items.map(String).join(`${t('assistant-list-separator')} `);
}

function weekdayName(weekday, t) {
  switch (weekday) {
    case 'Monday': return t('assistant-weekday-monday');
    case 'Tuesday': return t('assistant-weekday-tuesday');
    case 'Wednesday': return t('assistant-weekday-wednesday');
    case 'Thursday': return t('assistant-weekday-thursday');
    case 'Friday': return t('assistant-weekday-friday');
    case 'Saturday': return t('assistant-weekday-saturday');
    case 'Sunday': return t('assistant-weekday-sunday');
    default: return String(weekday);
  }
}

function languageName(code, t) {
  switch (code) {
    case 'en': return t('language-english');
    case 'ar': return t('language-arabic');
    default: return String(code);
  }
}

function repeatName(repeat, t) {
  switch (repeat) {
    case 'daily': return t('assistant-repeat-daily');
    case 'weekly': return t('assistant-repeat-weekly');
    default: return null;
  }
}

// What a tool call would do, as a phrase that fits after "I understood this request as: ".
function describeTool(tool, args, t, settings) {
  const a = args && typeof args === 'object' ? args : {};
  let action;
  switch (tool) {
    case 'open_app':
      action = fill(t('assistant-tool-open-app'), { name: a.name });
      break;
    case 'list_files':
      action = fill(t('assistant-tool-list-files'), { path: a.path });
      break;
    case 'search_files':
      action = fill(t('assistant-tool-search-files'), { query: a.query });
      break;
    case 'create_reminder':
      action = fill(t('assistant-tool-create-reminder'), { text: a.text, time: formatTime(a.time, settings, t) });
      break;
    case 'schedule_meeting':
      action = fill(t('assistant-tool-schedule-meeting'), { title: a.title, time: formatTime(a.time, settings, t) });
      if (Array.isArray(a.attendees) && a.attendees.length > 0) {
        action = fill(t('assistant-tool-with-attendees'), { action, attendees: joinList(a.attendees, t) });
      }
      break;
    case 'set_language':
      action = fill(t('assistant-tool-set-language'), { language: languageName(a.lang, t) });
      break;
    case 'current_time':
      action = t('assistant-tool-current-time');
      break;
    default:
      return fill(t('assistant-tool-unknown'), { tool });
  }
  const repeat = repeatName(a.repeat, t);
  return repeat ? fill(t('assistant-tool-with-repeat'), { action, repeat }) : action;
}

function entryLabel(kind, t) {
  switch (kind) {
    case 'file': return null;
    case 'dir': return t('assistant-entry-dir');
    case 'symlink': return t('assistant-entry-symlink');
    default: return t('assistant-entry-other');
  }
}

function lastPart(path) {
  const parts = String(path).split('/').filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : String(path);
}

// File names are never digit-shaped. They go to the panel as plain strings in a `files` block.
function describeFiles(heading, none, entries, result, nameOf, t, settings) {
  const list = Array.isArray(entries) ? entries : [];
  const blocks = [];
  if (list.length === 0) {
    blocks.push({ tone: 'info', text: none });
  } else {
    blocks.push({ tone: 'info', text: heading });
    blocks.push({
      tone: 'files',
      items: list.slice(0, MAX_LISTED).map((entry) => {
        const e = entry && typeof entry === 'object' ? entry : {};
        return { name: nameOf(e.path === undefined || e.path === null ? '' : String(e.path)), label: entryLabel(e.kind, t) };
      }),
    });
    if (list.length > MAX_LISTED) {
      blocks.push({ tone: 'note', text: fill(t('assistant-result-more'), { count: shapeNumber(list.length - MAX_LISTED, settings) }) });
    }
  }
  if (result.truncated === true) {
    blocks.push({ tone: 'note', text: t('assistant-result-truncated') });
  }
  if (typeof result.unreadable === 'number' && result.unreadable > 0) {
    blocks.push({ tone: 'note', text: fill(t('assistant-result-unreadable'), { count: shapeNumber(result.unreadable, settings) }) });
  }
  return blocks;
}

function formatStart(start, settings, t) {
  return ISO_DATE.test(String(start)) ? formatDate(start, settings, t) : formatTime(start, settings, t);
}

function describeEvent(event, t, settings) {
  const e = event && typeof event === 'object' ? event : {};
  let text = fill(t('assistant-result-event'), { summary: e.summary, time: formatStart(e.start, settings, t) });
  if (Array.isArray(e.attendees) && e.attendees.length > 0) {
    text = fill(t('assistant-tool-with-attendees'), { action: text, attendees: joinList(e.attendees, t) });
  }
  const repeat = repeatName(e.repeat, t);
  return fill(t('assistant-result-sentence'), { text: repeat ? fill(t('assistant-tool-with-repeat'), { action: text, repeat }) : text });
}

function describeFailure(error, args, t) {
  const e = error && typeof error === 'object' ? error : {};
  let text;
  switch (e.code) {
    case 'root_unusable': text = t('assistant-error-root-unusable'); break;
    case 'not_found': text = t('assistant-error-not-found'); break;
    case 'outside_root': text = t('assistant-error-outside-root'); break;
    case 'not_a_directory': text = t('assistant-error-not-a-directory'); break;
    case 'unreadable': text = t('assistant-error-unreadable'); break;
    case 'empty_query': text = t('assistant-error-empty-query'); break;
    case 'empty_name': text = t('assistant-error-empty-name'); break;
    case 'app_not_found': text = fill(t('assistant-error-app-not-found'), { name: args.name }); break;
    case 'launch_failed': text = t('assistant-error-launch-failed'); break;
    case 'unsupported': text = t('assistant-error-unsupported'); break;
    case 'calendar_io': text = t('assistant-error-calendar-io'); break;
    case 'calendar_malformed': text = t('assistant-error-calendar-malformed'); break;
    case 'not_an_event': text = t('assistant-error-not-an-event'); break;
    case 'invalid_time': text = t('assistant-error-invalid-time'); break;
    case 'empty_text': text = t('assistant-error-empty-text'); break;
    default: text = fill(t('assistant-error'), { message: e.message }); break;
  }
  const blocks = [{ tone: 'error', text }];
  if (e.code === 'app_not_found' && Array.isArray(e.closest) && e.closest.length > 0) {
    blocks.push({ tone: 'note', text: fill(t('assistant-error-closest'), { names: joinList(e.closest.map((name) => FSI + name + PDI), t) }) });
  }
  return blocks;
}

// What really happened, from the `result` the core sends with a `run`. Without a usable result nothing is claimed.
function describeResult(r, t, settings) {
  const args = r.arguments && typeof r.arguments === 'object' ? r.arguments : {};
  const result = r.result && typeof r.result === 'object' ? r.result : {};
  if (result.ok === false) {
    return describeFailure(result.error, args, t);
  }
  if (result.ok === true) {
    switch (result.type) {
      case 'listing':
        return describeFiles(
          fill(t('assistant-result-listing'), { path: result.path }),
          fill(t('assistant-result-listing-empty'), { path: result.path }),
          result.entries, result, lastPart, t, settings);
      case 'search':
        return describeFiles(
          fill(t('assistant-result-search'), { query: args.query }),
          fill(t('assistant-result-search-none'), { query: args.query }),
          result.matches, result, String, t, settings);
      case 'opened': {
        const app = result.app && typeof result.app === 'object' ? result.app : {};
        return [{ tone: 'info', text: fill(t('assistant-result-opened'), { name: app.name }) }];
      }
      case 'event':
        return [{ tone: 'info', text: describeEvent(result.event, t, settings) }];
      case 'language':
        return [{ tone: 'info', text: fill(t('assistant-result-language'), { language: languageName(result.lang, t) }) }];
      default:
        break;
    }
  }
  return [{ tone: 'info', text: fill(t('assistant-run-intro'), { action: describeTool(r.tool, r.arguments, t, settings) }) }];
}

// The date the assistant guessed and its time of day, for the time input of a clarification.
// Null when the guess is not a local date and time the core would take back.
function clarifyTime(args) {
  const match = ISO_TIME.exec(String(args && typeof args === 'object' ? args.time : ''));
  const date = match && ISO_DATE.exec(match[1]);
  return date && Settings.isGregorianDate(Number(date[1]), Number(date[2]), Number(date[3])) ? { date: match[1], time: match[2] } : null;
}

function describeResponse(response, t, lang, settings) {
  const r = response && typeof response === 'object' ? response : {};
  const block = (tone, text) => ({ tone, text });
  const action = () => describeTool(r.tool, r.arguments, t, settings);
  // Buttons for a held call. Without an id from the core there is nothing the buttons could name.
  const actions = (clarify) => (typeof r.action_id === 'string' && r.action_id ? [{ tone: 'actions', actionId: r.action_id, clarify }] : []);
  switch (r.kind) {
    case 'reply': {
      const blocks = [block('reply', String(r.text === undefined || r.text === null ? '' : r.text))];
      const words = Array.isArray(r.foreign_words) ? r.foreign_words.map((w) => (w && w.word !== undefined ? w.word : w)) : [];
      if (words.length > 0) {
        blocks.push(block('warning', fill(t('assistant-foreign-warning'), { words: joinList(words.map((w) => FSI + w + PDI), t) })));
      }
      return blocks;
    }
    case 'time':
      return [block('info', fill(t('assistant-time'), {
        time: Settings.shapeDigits(`${String(r.hour).padStart(2, '0')}:${String(r.minute).padStart(2, '0')}`, digitsOf(settings)),
        weekday: weekdayName(r.weekday, t),
        date: formatDate(r.date, settings, t),
      }))];
    case 'unsupported_time_zone':
      return [block('info', fill(t('assistant-unsupported-time-zone'), { zone: r.zone }))];
    case 'cannot_do_offline': {
      switch (r.what) {
        case 'news': return [block('info', t('assistant-offline-news'))];
        case 'weather': return [block('info', t('assistant-offline-weather'))];
        case 'time': return [block('info', t('assistant-offline-time'))];
        default: return [block('info', t('assistant-offline-other'))];
      }
    }
    case 'declined':
      return [block('info', t('assistant-declined'))];
    case 'hijri_approx': {
      const month = lang === 'ar' ? r.month_name_ar : r.month_name_en;
      const blocks = [block('info', fill(t('assistant-hijri'), {
        gregorian: formatDate(r.gregorian, { digits: digitsOf(settings) }),
        day: shapeNumber(r.day, settings),
        month: month === undefined ? '' : month,
        year: shapeNumber(r.year, settings),
      }))];
      // Only an exact date may go without the note, and the core says so explicitly.
      if (r.approximate !== false) {
        blocks.push(block('warning', t('assistant-hijri-approximate')));
      }
      return blocks;
    }
    case 'missing_date':
      return [block('question', t('assistant-missing-date'))];
    case 'run':
      return describeResult(r, t, settings);
    case 'needs_clarification':
      return [block('question', fill(t('assistant-clarify-question'), { action: action() })), ...actions(clarifyTime(r.arguments))];
    case 'needs_confirmation':
      return [block('question', fill(t('assistant-confirm-question'), { action: action() })), ...actions(null)];
    case 'cancelled':
      return [block('info', t('assistant-cancelled'))];
    case 'missing_time':
      return [block('question', t('assistant-missing-time'))];
    case 'error':
      return [block('error', fill(t('assistant-error'), { message: r.message }))];
    case 'unreachable':
      return [block('error', t('assistant-unreachable'))];
    default: {
      const blocks = [block('info', t('assistant-other'))];
      if (typeof r.debug === 'string' && r.debug) {
        blocks.push(block('note', fill(t('assistant-other-detail'), { debug: r.debug })));
      }
      return blocks;
    }
  }
}

// The bridge answers with the core's JSON line, or with {"kind":"error"} and a 5xx status when it fails itself.
function responseFromHttp(status, bodyText) {
  let body;
  try {
    body = JSON.parse(bodyText);
  } catch (error) {
    body = null;
  }
  if (body && typeof body === 'object' && !Array.isArray(body) && typeof body.kind === 'string') {
    return body;
  }
  return { kind: 'error', message: `HTTP ${status}` };
}

const renderApi = { fill, formatElapsed, formatDate, formatTime, describeTool, describeResponse, responseFromHttp };

if (typeof module !== 'undefined' && module.exports) {
  module.exports = renderApi;
} else {
  globalThis.ScarfeeRender = renderApi;
}
