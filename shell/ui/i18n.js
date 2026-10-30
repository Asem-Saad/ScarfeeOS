// Minimal Fluent reader: only `key = value` lines and `#` comments, as used in locales/*/scarfeeos.ftl.
'use strict';

const RTL_LANGUAGES = new Set(['ar']);
const ATTRIBUTES = ['placeholder', 'aria-label', 'title'];

function parseFtl(source) {
  const messages = new Map();
  for (const line of String(source).split(/\r?\n/)) {
    const match = /^([A-Za-z][A-Za-z0-9_-]*)[ \t]*=[ \t]*(.*)$/.exec(line);
    if (match) {
      messages.set(match[1], match[2].trim());
    }
  }
  return messages;
}

function directionOf(lang) {
  return RTL_LANGUAGES.has(lang) ? 'rtl' : 'ltr';
}

function createI18n(catalogs, root, initialLang) {
  let current = initialLang;

  function t(key) {
    const messages = catalogs[current];
    return messages && messages.has(key) ? messages.get(key) : key;
  }

  function translate(container) {
    for (const node of container.querySelectorAll('[data-i18n]')) {
      node.textContent = t(node.getAttribute('data-i18n'));
    }
    for (const attribute of ATTRIBUTES) {
      for (const node of container.querySelectorAll(`[data-i18n-${attribute}]`)) {
        node.setAttribute(attribute, t(node.getAttribute(`data-i18n-${attribute}`)));
      }
    }
  }

  function setLanguage(lang) {
    if (!Object.prototype.hasOwnProperty.call(catalogs, lang)) {
      return false;
    }
    current = lang;
    root.lang = lang;
    root.dir = directionOf(lang);
    return true;
  }

  return { t, translate, setLanguage, getLanguage: () => current };
}

const i18nApi = { parseFtl, directionOf, createI18n };

if (typeof module !== 'undefined' && module.exports) {
  module.exports = i18nApi;
} else {
  globalThis.ScarfeeI18n = i18nApi;
}
