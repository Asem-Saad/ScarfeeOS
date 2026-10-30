// Panel settings (language, digits, calendar, week start), digit shaping and the tabular Hijri calendar.
// Pure functions plus a small store; no DOM. The Hijri arithmetic is the same method as core/src/hijri.rs.
'use strict';

const STORAGE_KEY = 'scarfeeos.panel.settings';

// Eastern Arabic digits are off until the user chooses them, in both languages (docs/i18n-and-arabic.md rule 6).
const DEFAULTS = Object.freeze({ language: 'en', digits: 'western', calendar: 'gregorian', weekStart: 'sunday' });
const CHOICES = Object.freeze({
  language: ['en', 'ar'],
  digits: ['western', 'eastern'],
  calendar: ['gregorian', 'both'],
  weekStart: ['saturday', 'sunday', 'monday'],
});

const EASTERN_ZERO = 0x0660;

// Only digits that the panel itself formats may go through these. Never pass model text, URLs or file names.
function toEasternDigits(text) {
  return String(text).replace(/[0-9]/g, (digit) => String.fromCharCode(EASTERN_ZERO + Number(digit)));
}

function toWesternDigits(text) {
  return String(text).replace(/[٠-٩]/g, (digit) => String(digit.charCodeAt(0) - EASTERN_ZERO));
}

function shapeDigits(text, digits) {
  return digits === 'eastern' ? toEasternDigits(text) : toWesternDigits(text);
}

const EPOCH_DAYS = 1948440 - 2440588;
const MAX_HIJRI_YEAR = 1000000;

function mod(a, b) {
  return ((a % b) + b) % b;
}

function isLeapHijriYear(year) {
  return mod(11 * year + 14, 30) < 11;
}

function hijriMonthLength(year, month) {
  if (!Number.isInteger(month) || month < 1 || month > 12) {
    return null;
  }
  if (month < 12) {
    return month % 2 === 1 ? 30 : 29;
  }
  return isLeapHijriYear(year) ? 30 : 29;
}

function daysBeforeYear(year) {
  return 354 * (year - 1) + Math.floor((3 + 11 * year) / 30);
}

function daysBeforeMonth(month) {
  let days = 0;
  for (let m = 1; m < month; m += 1) {
    days += hijriMonthLength(0, m);
  }
  return days;
}

function daysFromCivil(year, month, day) {
  const y = year - (month <= 2 ? 1 : 0);
  const era = Math.floor(y / 400);
  const yoe = y - era * 400;
  const doy = Math.floor((153 * ((month + 9) % 12) + 2) / 5) + day - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146097 + doe - 719468;
}

function civilFromDays(days) {
  const z = days + 719468;
  const era = Math.floor(z / 146097);
  const doe = z - era * 146097;
  const yoe = Math.floor((doe - Math.floor(doe / 1460) + Math.floor(doe / 36524) - Math.floor(doe / 146096)) / 365);
  const doy = doe - (365 * yoe + Math.floor(yoe / 4) - Math.floor(yoe / 100));
  const mp = Math.floor((5 * doy + 2) / 153);
  const day = doy - Math.floor((153 * mp + 2) / 5) + 1;
  const month = mp < 10 ? mp + 3 : mp - 9;
  return { year: yoe + era * 400 + (month <= 2 ? 1 : 0), month, day };
}

function isGregorianDate(year, month, day) {
  if (![year, month, day].every(Number.isInteger) || month < 1 || month > 12 || day < 1) {
    return false;
  }
  const leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
  const lengths = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return day <= lengths[month - 1];
}

// Approximate Hijri date of a valid Gregorian date, by the tabular calendar. It never looks at the moon and can
// differ from the official calendar by a day or two, so every caller must show it as approximate.
function toHijriApprox(year, month, day) {
  const n = daysFromCivil(year, month, day) - EPOCH_DAYS;
  let hijriYear = Math.floor((30 * n) / 10631) + 1;
  while (daysBeforeYear(hijriYear + 1) <= n) {
    hijriYear += 1;
  }
  while (daysBeforeYear(hijriYear) > n) {
    hijriYear -= 1;
  }
  let rest = n - daysBeforeYear(hijriYear);
  let hijriMonth = 1;
  while (hijriMonth < 12 && rest >= hijriMonthLength(hijriYear, hijriMonth)) {
    rest -= hijriMonthLength(hijriYear, hijriMonth);
    hijriMonth += 1;
  }
  return { year: hijriYear, month: hijriMonth, day: rest + 1 };
}

// The Gregorian date of a Hijri date, or null when the Hijri date does not exist.
function fromHijriApprox(year, month, day) {
  const length = hijriMonthLength(year, month);
  if (!Number.isInteger(year) || year < 1 || year > MAX_HIJRI_YEAR || length === null) {
    return null;
  }
  if (!Number.isInteger(day) || day < 1 || day > length) {
    return null;
  }
  return civilFromDays(daysBeforeYear(year) + daysBeforeMonth(month) + day - 1 + EPOCH_DAYS);
}

function normalize(raw) {
  const settings = { ...DEFAULTS };
  if (raw && typeof raw === 'object' && !Array.isArray(raw)) {
    for (const key of Object.keys(CHOICES)) {
      if (typeof raw[key] === 'string' && CHOICES[key].includes(raw[key])) {
        settings[key] = raw[key];
      }
    }
  }
  return settings;
}

// localStorage can be missing, blocked, or throw on access, so even looking it up is guarded.
function browserStorage() {
  try {
    return globalThis.localStorage || null;
  } catch (error) {
    return null;
  }
}

// `getStorage` returns a Storage-like object (getItem/setItem) or null. Settings always work in memory;
// saving is best effort.
function createStore(getStorage) {
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
    const saved = store ? store.getItem(STORAGE_KEY) : null;
    current = normalize(saved ? JSON.parse(saved) : null);
  } catch (error) {
    current = normalize(null);
  }

  return {
    get: () => ({ ...current }),
    // Applies the value in memory at once and returns true. False when the key or value is not a known choice.
    set(key, value) {
      if (!Object.prototype.hasOwnProperty.call(CHOICES, key) || !CHOICES[key].includes(value)) {
        return false;
      }
      current = { ...current, [key]: value };
      try {
        const store = storage();
        if (store) {
          store.setItem(STORAGE_KEY, JSON.stringify(current));
        }
      } catch (error) {
        // Storage is full, blocked or gone: the choice still holds until the panel closes.
      }
      return true;
    },
  };
}

const settingsApi = {
  STORAGE_KEY,
  DEFAULTS,
  CHOICES,
  toEasternDigits,
  toWesternDigits,
  shapeDigits,
  isLeapHijriYear,
  hijriMonthLength,
  isGregorianDate,
  toHijriApprox,
  fromHijriApprox,
  normalize,
  browserStorage,
  createStore,
};

if (typeof module !== 'undefined' && module.exports) {
  module.exports = settingsApi;
} else {
  globalThis.ScarfeeSettings = settingsApi;
}
