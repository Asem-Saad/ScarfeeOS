// Meetings and reminders screen: upcoming events from the core (see core/src/ipc.rs), a month grid,
// delete with a confirmation. Everything sits in one function so that, loaded as a classic script beside
// i18n.js and settings.js, it adds no top-level names. Event text only ever reaches the page through textContent.
(function () {
  'use strict';

  const Settings = typeof require === 'function' ? require('./settings.js') : globalThis.ScarfeeSettings;

  const WEEKEND_KEY = 'scarfeeos.calendar.weekend';
  // Day numbers follow Date#getUTCDay: 0 is Sunday. The default weekend is Saturday and Sunday.
  const DEFAULT_WEEKEND = [0, 6];
  const WEEK_START_DAY = { saturday: 6, sunday: 0, monday: 1 };
  const FSI = '\u2068';
  const PDI = '\u2069';
  const PLACEHOLDER = /\{\s*\$([A-Za-z][A-Za-z0-9_-]*)\s*\}/g;
  const ISO_DATE = /^(\d{4})-(\d{2})-(\d{2})$/;
  const ISO_TIME = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})(?::\d{2})?$/;
  const DAY_MS = 86400000;

  // Fills `{ $name }` the way Fluent does; each value is isolated so Arabic text cannot reorder what is around it.
  function fill(template, values) {
    return String(template).replace(PLACEHOLDER, (_, name) => {
      const value = values && Object.prototype.hasOwnProperty.call(values, name) ? values[name] : '';
      return FSI + String(value === null || value === undefined ? '' : value) + PDI;
    });
  }

  function weekdayName(day, t) {
    switch (day) {
      case 0: return t('assistant-weekday-sunday');
      case 1: return t('assistant-weekday-monday');
      case 2: return t('assistant-weekday-tuesday');
      case 3: return t('assistant-weekday-wednesday');
      case 4: return t('assistant-weekday-thursday');
      case 5: return t('assistant-weekday-friday');
      default: return t('assistant-weekday-saturday');
    }
  }

  function monthName(month, t) {
    switch (month) {
      case 1: return t('calendar-month-1');
      case 2: return t('calendar-month-2');
      case 3: return t('calendar-month-3');
      case 4: return t('calendar-month-4');
      case 5: return t('calendar-month-5');
      case 6: return t('calendar-month-6');
      case 7: return t('calendar-month-7');
      case 8: return t('calendar-month-8');
      case 9: return t('calendar-month-9');
      case 10: return t('calendar-month-10');
      case 11: return t('calendar-month-11');
      default: return t('calendar-month-12');
    }
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

  function repeatName(repeat, t) {
    switch (repeat) {
      case 'daily': return t('assistant-repeat-daily');
      case 'weekly': return t('assistant-repeat-weekly');
      default: return null;
    }
  }

  function digitsOf(settings) {
    return settings && settings.digits === 'eastern' ? 'eastern' : 'western';
  }

  function pad2(number) {
    return String(number).padStart(2, '0');
  }

  function isoOf(date) {
    return `${String(date.year).padStart(4, '0')}-${pad2(date.month)}-${pad2(date.day)}`;
  }

  function parseDate(text) {
    const match = ISO_DATE.exec(String(text));
    if (!match) {
      return null;
    }
    const date = { year: Number(match[1]), month: Number(match[2]), day: Number(match[3]) };
    return Settings.isGregorianDate(date.year, date.month, date.day) ? date : null;
  }

  // Days since 1970-01-01 (setUTCFullYear because Date.UTC reads years 0 to 99 as 1900 to 1999).
  function dayNumber(date) {
    const moment = new Date(0);
    moment.setUTCFullYear(date.year, date.month - 1, date.day);
    return Math.round(moment.getTime() / DAY_MS);
  }

  function dateOfDayNumber(number) {
    const moment = new Date(number * DAY_MS);
    return { year: moment.getUTCFullYear(), month: moment.getUTCMonth() + 1, day: moment.getUTCDate() };
  }

  function weekdayOf(date) {
    return ((dayNumber(date) % 7) + 11) % 7;
  }

  // The Gregorian date, then the approximate Hijri one when the user asked for both. The Hijri part is
  // always worded as "about" and "AH" by the locale string.
  function formatDate(date, settings, t) {
    const digits = digitsOf(settings);
    const gregorian = Settings.shapeDigits(isoOf(date), digits);
    if (!settings || settings.calendar !== 'both') {
      return gregorian;
    }
    const hijri = Settings.toHijriApprox(date.year, date.month, date.day);
    return fill(t('settings-date-both'), {
      gregorian,
      day: Settings.shapeDigits(hijri.day, digits),
      month: hijriMonthName(hijri.month, t),
      year: Settings.shapeDigits(hijri.year, digits),
    });
  }

  function eventTime(start, settings) {
    const match = ISO_TIME.exec(String(start));
    return match ? Settings.shapeDigits(match[2], digitsOf(settings)) : null;
  }

  // Reads the core's `events` list. Entries that are not usable events are dropped, and the rest are put
  // in date order whatever order they came in.
  function readEvents(list) {
    const events = [];
    for (const item of Array.isArray(list) ? list : []) {
      const date = item && typeof item === 'object' ? parseDate(item.date) : null;
      if (!date || typeof item.uid !== 'string' || !item.uid || typeof item.summary !== 'string') {
        continue;
      }
      const timeMatch = ISO_TIME.exec(String(item.start));
      events.push({
        uid: item.uid,
        summary: item.summary,
        date,
        day: dayNumber(date),
        start: typeof item.start === 'string' ? item.start : '',
        time: timeMatch ? timeMatch[2] : '',
        attendees: Array.isArray(item.attendees) ? item.attendees.filter((name) => typeof name === 'string') : [],
        repeat: item.repeat === 'daily' || item.repeat === 'weekly' ? item.repeat : null,
      });
    }
    events.sort((a, b) => a.day - b.day || (a.time < b.time ? -1 : a.time > b.time ? 1 : 0) || (a.uid < b.uid ? -1 : a.uid > b.uid ? 1 : 0));
    return events;
  }

  // The core lists a repeating event once, at its next occurrence; later ones are worked out here.
  function occursOn(event, day) {
    if (day < event.day) {
      return false;
    }
    if (event.repeat === 'daily') {
      return true;
    }
    if (event.repeat === 'weekly') {
      return (day - event.day) % 7 === 0;
    }
    return day === event.day;
  }

  // Weeks of the month as rows of seven cells, starting on `weekStart` ('saturday' | 'sunday' | 'monday').
  function monthWeeks(year, month, weekStart) {
    const first = dayNumber({ year, month, day: 1 });
    const lastDay = dayNumber({ year: month === 12 ? year + 1 : year, month: month === 12 ? 1 : month + 1, day: 1 }) - 1;
    const startDay = Object.prototype.hasOwnProperty.call(WEEK_START_DAY, weekStart) ? WEEK_START_DAY[weekStart] : 0;
    const lead = (weekdayOf(dateOfDayNumber(first)) - startDay + 7) % 7;
    const weeks = [];
    for (let day = first - lead; day <= lastDay; day += 7) {
      const week = [];
      for (let i = 0; i < 7; i += 1) {
        const date = dateOfDayNumber(day + i);
        week.push({ ...date, number: day + i, inMonth: date.month === month, weekday: (startDay + i) % 7 });
      }
      weeks.push(week);
    }
    return weeks;
  }

  function normalizeWeekend(raw) {
    if (!Array.isArray(raw)) {
      return DEFAULT_WEEKEND.slice();
    }
    return [...new Set(raw.filter((day) => Number.isInteger(day) && day >= 0 && day <= 6))].sort((a, b) => a - b);
  }

  // Weekend days are their own setting, saved apart from the panel settings. Like the panel store, it
  // always works in memory and saving is best effort.
  function createWeekendStore(getStorage) {
    const storage = () => {
      try {
        return getStorage() || null;
      } catch (error) {
        return null;
      }
    };
    let current;
    try {
      const store = storage();
      const saved = store ? store.getItem(WEEKEND_KEY) : null;
      current = normalizeWeekend(saved ? JSON.parse(saved) : null);
    } catch (error) {
      current = DEFAULT_WEEKEND.slice();
    }
    return {
      get: () => current.slice(),
      // Replaces the weekend. False when the value is not a list of day numbers from 0 to 6.
      set(days) {
        if (!Array.isArray(days) || !days.every((day) => Number.isInteger(day) && day >= 0 && day <= 6)) {
          return false;
        }
        current = normalizeWeekend(days);
        try {
          const store = storage();
          if (store) {
            store.setItem(WEEKEND_KEY, JSON.stringify(current));
          }
        } catch (error) {
          // Storage is full, blocked or gone: the choice still holds until the page closes.
        }
        return true;
      },
    };
  }

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

  // `env`: document, ask(body) -> Promise of a core response, the translator, settings() -> panel settings,
  // weekendStore, today() -> { year, month, day }. Returns the controls the page and the tests use.
  function createCalendar(env) {
    const { document, ask, weekendStore, t } = env;
    const byId = (id) => document.getElementById(id);
    const nodes = {
      list: byId('events-list'),
      status: byId('events-status'),
      notice: byId('events-notice'),
      monthTitle: byId('month-title'),
      grid: byId('month-grid'),
      hijriNote: byId('month-hijri-note'),
      weekendOptions: byId('weekend-options'),
    };
    const shown = env.today();
    const state = {
      events: [],
      phase: 'loading',
      message: '',
      notice: null,
      confirming: null,
      deleting: null,
      year: shown.year,
      month: shown.month,
    };
    let loadId = 0;

    function el(tag, className, text) {
      const node = document.createElement(tag);
      if (className) {
        node.className = className;
      }
      if (text !== undefined) {
        node.textContent = text;
      }
      return node;
    }

    function button(label, className, onClick) {
      const node = el('button', `button ${className}`.trim(), label);
      node.type = 'button';
      node.addEventListener('click', onClick);
      return node;
    }

    function renderStatus() {
      const status = nodes.status;
      status.className = 'status';
      status.setAttribute('role', 'status');
      if (state.phase === 'loading') {
        status.textContent = t('calendar-loading');
      } else if (state.phase === 'unreachable') {
        status.textContent = t('assistant-unreachable');
        status.className = 'status status-error';
        status.setAttribute('role', 'alert');
      } else if (state.phase === 'error') {
        status.textContent = fill(t('assistant-error'), { message: state.message });
        status.className = 'status status-error';
        status.setAttribute('role', 'alert');
      } else if (state.events.length === 0) {
        status.textContent = t('calendar-empty');
      } else {
        status.textContent = '';
      }
      status.hidden = status.textContent === '';
    }

    function renderNotice() {
      const notice = nodes.notice;
      notice.hidden = !state.notice;
      notice.textContent = state.notice ? state.notice.text : '';
      notice.className = state.notice && state.notice.error ? 'notice notice-error' : 'notice';
      notice.setAttribute('role', state.notice && state.notice.error ? 'alert' : 'status');
    }

    function renderEvent(event, settings) {
      const item = el('li', 'event');
      const main = el('div', 'event-main');
      const title = el('p', 'event-title', event.summary);
      title.dir = 'auto';
      const time = eventTime(event.start, settings);
      const date = formatDate(event.date, settings, t);
      const when = el('p', 'event-meta', time === null ? date : fill(t('calendar-when'), { date, time }));
      when.dir = 'auto';
      main.append(title, when);
      if (time === null && event.start.length === 10) {
        main.append(el('p', 'event-meta', t('calendar-all-day')));
      }
      const repeat = repeatName(event.repeat, t);
      if (repeat) {
        main.append(el('p', 'event-meta', fill(t('calendar-repeats'), { repeat })));
      }
      if (event.attendees.length > 0) {
        const names = el('p', 'event-meta', fill(t('calendar-attendees'), { names: event.attendees.join(`${t('assistant-list-separator')} `) }));
        names.dir = 'auto';
        main.append(names);
      }
      item.append(main);

      const actions = el('div', 'event-actions');
      if (state.deleting === event.uid) {
        actions.append(el('p', 'event-meta', t('calendar-deleting')));
      } else if (state.confirming === event.uid) {
        const question = el('p', 'event-meta', fill(event.repeat ? t('calendar-delete-confirm-repeat') : t('calendar-delete-confirm'), { title: event.summary }));
        question.dir = 'auto';
        question.setAttribute('role', 'alert');
        actions.append(
          question,
          button(t('calendar-delete-yes'), 'button-danger', () => remove(event.uid)),
          button(t('calendar-delete-keep'), '', () => {
            state.confirming = null;
            render();
          })
        );
      } else {
        const del = button(t('calendar-delete'), 'button-danger', () => {
          state.confirming = event.uid;
          render();
        });
        del.setAttribute('aria-label', fill(t('calendar-delete-label'), { title: event.summary }));
        actions.append(del);
      }
      item.append(actions);
      return item;
    }

    function renderList() {
      const settings = env.settings();
      nodes.list.replaceChildren(...state.events.map((event) => renderEvent(event, settings)));
    }

    function renderGrid() {
      const settings = env.settings();
      const digits = digitsOf(settings);
      const both = settings.calendar === 'both';
      const weekend = new Set(weekendStore.get());
      const today = dayNumber(env.today());
      const weeks = monthWeeks(state.year, state.month, settings.weekStart);

      nodes.monthTitle.textContent = fill(t('calendar-month-title'), {
        month: monthName(state.month, t),
        year: Settings.shapeDigits(state.year, digits),
      });

      const head = el('tr');
      for (const cell of weeks[0]) {
        const th = el('th', weekend.has(cell.weekday) ? 'day-weekend' : '', weekdayName(cell.weekday, t));
        th.scope = 'col';
        head.append(th);
      }
      const body = el('tbody');
      for (const week of weeks) {
        const row = el('tr');
        for (const cell of week) {
          const classes = ['day'];
          if (!cell.inMonth) classes.push('day-outside');
          if (weekend.has(cell.weekday)) classes.push('day-weekend');
          if (cell.number === today) classes.push('day-today');
          const td = el('td', classes.join(' '));
          if (cell.number === today) {
            td.setAttribute('aria-current', 'date');
          }
          const dayHead = el('div', 'day-head', Settings.shapeDigits(cell.day, digits));
          if (both) {
            const hijri = Settings.toHijriApprox(cell.year, cell.month, cell.day);
            const approx = el('span', 'day-hijri', Settings.shapeDigits(hijri.day, digits));
            approx.title = t('settings-hijri-note');
            dayHead.append(approx);
          }
          td.append(dayHead);
          const here = state.events.filter((event) => occursOn(event, cell.number));
          if (here.length > 0) {
            const marks = el('ul', 'day-events');
            for (const event of here) {
              const mark = el('li', '', event.summary);
              mark.dir = 'auto';
              marks.append(mark);
            }
            td.append(marks);
          }
          row.append(td);
        }
        body.append(row);
      }
      const table = el('table');
      table.setAttribute('aria-labelledby', 'month-title');
      const thead = el('thead');
      thead.append(head);
      table.append(thead, body);
      nodes.grid.replaceChildren(table);
      nodes.hijriNote.hidden = !both;
    }

    function renderWeekend() {
      const settings = env.settings();
      const startDay = Object.prototype.hasOwnProperty.call(WEEK_START_DAY, settings.weekStart) ? WEEK_START_DAY[settings.weekStart] : 0;
      const weekend = new Set(weekendStore.get());
      const options = [];
      for (let i = 0; i < 7; i += 1) {
        const day = (startDay + i) % 7;
        const label = el('label', 'weekend-option');
        const box = el('input');
        box.type = 'checkbox';
        box.checked = weekend.has(day);
        box.addEventListener('change', () => {
          const chosen = new Set(weekendStore.get());
          if (box.checked) {
            chosen.add(day);
          } else {
            chosen.delete(day);
          }
          weekendStore.set([...chosen]);
          renderGrid();
        });
        label.append(box, el('span', '', weekdayName(day, t)));
        options.push(label);
      }
      nodes.weekendOptions.replaceChildren(...options);
    }

    function render() {
      renderStatus();
      renderNotice();
      renderList();
      renderGrid();
      renderWeekend();
    }

    function refresh(keepNotice) {
      loadId += 1;
      const mine = loadId;
      if (!keepNotice) {
        state.notice = null;
      }
      state.phase = 'loading';
      state.confirming = null;
      render();
      return ask({ upcoming: true, from: isoOf(env.today()) }).then((response) => {
        if (mine !== loadId) {
          return;
        }
        if (response && response.kind === 'events' && Array.isArray(response.events)) {
          state.events = readEvents(response.events);
          state.phase = 'ready';
        } else if (response && response.kind === 'unreachable') {
          state.phase = 'unreachable';
        } else {
          state.phase = 'error';
          state.message = response && response.kind === 'error' && typeof response.message === 'string' ? response.message : '';
        }
        render();
      });
    }

    function remove(uid) {
      const event = state.events.find((candidate) => candidate.uid === uid);
      const title = event ? event.summary : '';
      state.deleting = uid;
      state.confirming = null;
      render();
      return ask({ delete_event: uid }).then((response) => {
        state.deleting = null;
        if (response && response.kind === 'event_deleted') {
          state.notice = { error: false, text: fill(t('calendar-deleted'), { title }) };
        } else if (response && response.kind === 'unreachable') {
          state.notice = { error: true, text: t('assistant-unreachable') };
        } else {
          const message = response && response.kind === 'error' && typeof response.message === 'string' ? response.message : '';
          state.notice = { error: true, text: fill(t('assistant-error'), { message }) };
        }
        return refresh(true);
      });
    }

    function moveMonth(step) {
      const index = state.year * 12 + (state.month - 1) + step;
      state.year = Math.floor(index / 12);
      state.month = (index % 12) + 1;
      renderGrid();
    }

    function showToday() {
      const now = env.today();
      state.year = now.year;
      state.month = now.month;
      renderGrid();
    }

    return { render, refresh, remove, moveMonth, showToday, state };
  }

  function start() {
    const root = document.documentElement;
    const catalogs = { en: new Map(), ar: new Map() };
    const i18n = ScarfeeI18n.createI18n(catalogs, root, 'en');
    let settings = ScarfeeSettings.createStore(ScarfeeSettings.browserStorage);
    const weekendStore = createWeekendStore(ScarfeeSettings.browserStorage);

    const calendar = createCalendar({
      document,
      t: i18n.t,
      settings: () => settings.get(),
      weekendStore,
      today() {
        const now = new Date();
        return { year: now.getFullYear(), month: now.getMonth() + 1, day: now.getDate() };
      },
      ask(body) {
        return fetch('/chat', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        })
          .then((response) => response.text().then((text) => responseFromHttp(response.status, text)))
          .catch(() => ({ kind: 'unreachable' }));
      },
    });

    function applyLanguage() {
      i18n.setLanguage(settings.get().language);
      i18n.translate(document);
    }

    document.getElementById('refresh').addEventListener('click', () => calendar.refresh(false));
    document.getElementById('month-previous').addEventListener('click', () => calendar.moveMonth(-1));
    document.getElementById('month-next').addEventListener('click', () => calendar.moveMonth(1));
    document.getElementById('month-today').addEventListener('click', () => calendar.showToday());

    // The panel keeps the settings; a change made there reaches this page through the storage event.
    window.addEventListener('storage', (event) => {
      if (event.key === ScarfeeSettings.STORAGE_KEY || event.key === null) {
        settings = ScarfeeSettings.createStore(ScarfeeSettings.browserStorage);
        applyLanguage();
        calendar.render();
      }
    });

    const loads = Object.keys(catalogs).map((lang) =>
      fetch(`../../locales/${lang}/scarfeeos.ftl`)
        .then((response) => response.text())
        .then((source) => {
          catalogs[lang] = ScarfeeI18n.parseFtl(source);
        })
        .catch((error) => console.warn(lang, error))
    );
    Promise.all(loads).then(() => {
      applyLanguage();
      calendar.refresh(false);
    });
  }

  const calendarApi = {
    WEEKEND_KEY,
    DEFAULT_WEEKEND,
    fill,
    parseDate,
    formatDate,
    eventTime,
    readEvents,
    occursOn,
    monthWeeks,
    createWeekendStore,
    createCalendar,
    responseFromHttp,
  };

  if (typeof document === 'undefined') {
    if (typeof module !== 'undefined' && module.exports) {
      module.exports = calendarApi;
    }
  } else {
    start();
  }
})();
