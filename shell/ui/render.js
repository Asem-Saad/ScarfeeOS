// Pure functions that turn a core response (see core/src/ipc.rs) into blocks of locale text for the panel.
// No DOM here. Every block is plain text: the panel must show it with textContent, never as HTML.
'use strict';

const FSI = '\u2068';
const PDI = '\u2069';
const PLACEHOLDER = /\{\s*\$([A-Za-z][A-Za-z0-9_-]*)\s*\}/g;
const ISO_TIME = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})(?::\d{2})?$/;

// Fills `{ $name }` the way Fluent does. Each value is isolated so Arabic text, names and numbers
// cannot reorder the words around them.
function fill(template, values) {
  return String(template).replace(PLACEHOLDER, (_, name) => {
    const value = values && Object.prototype.hasOwnProperty.call(values, name) ? values[name] : '';
    return FSI + String(value === null || value === undefined ? '' : value) + PDI;
  });
}

function formatElapsed(seconds) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

function formatTime(iso) {
  const match = ISO_TIME.exec(String(iso));
  return match ? `${match[1]} ${match[2]}` : String(iso);
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
function describeTool(tool, args, t) {
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
      action = fill(t('assistant-tool-create-reminder'), { text: a.text, time: formatTime(a.time) });
      break;
    case 'schedule_meeting':
      action = fill(t('assistant-tool-schedule-meeting'), { title: a.title, time: formatTime(a.time) });
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

function describeResponse(response, t, lang) {
  const r = response && typeof response === 'object' ? response : {};
  const block = (tone, text) => ({ tone, text });
  const action = () => describeTool(r.tool, r.arguments, t);
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
        time: `${String(r.hour).padStart(2, '0')}:${String(r.minute).padStart(2, '0')}`,
        weekday: weekdayName(r.weekday, t),
        date: r.date,
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
        gregorian: r.gregorian,
        day: r.day,
        month: month === undefined ? '' : month,
        year: r.year,
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
      return [
        block('info', fill(t('assistant-run-intro'), { action: action() })),
        block('note', t('assistant-not-connected')),
      ];
    case 'needs_clarification':
      return [
        block('question', fill(t('assistant-clarify-question'), { action: action() })),
        block('note', t('assistant-not-connected')),
      ];
    case 'needs_confirmation':
      return [
        block('question', fill(t('assistant-confirm-question'), { action: action() })),
        block('note', t('assistant-not-connected')),
      ];
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

const api = { fill, formatElapsed, formatTime, describeTool, describeResponse, responseFromHttp };

if (typeof module !== 'undefined' && module.exports) {
  module.exports = api;
} else {
  globalThis.ScarfeeRender = api;
}
