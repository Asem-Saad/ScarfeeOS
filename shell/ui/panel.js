'use strict';

(function () {
  // Matches runs that must stay left-to-right inside any paragraph: URLs, paths, file names, numbers.
  const LTR_RUN = new RegExp(
    [
      "https?:\\/\\/[A-Za-z0-9\\-._~:/?#\\[\\]@!$&'()*+,;=%]+",
      '(?<![A-Za-z0-9_.~-])(?:[A-Za-z0-9_.~-]*\\/)+[A-Za-z0-9_.-]+',
      '(?<![A-Za-z0-9_.~-])[A-Za-z0-9_-]+(?:\\.[A-Za-z0-9_-]+)*\\.[A-Za-z][A-Za-z0-9]{0,4}\\b',
      '[0-9\\u0660-\\u0669]+(?:[.,:/\\-][0-9\\u0660-\\u0669]+)*',
    ].join('|'),
    'g'
  );
  const TRAILING_PUNCTUATION = /[.,;:!?)\]'"-]+$/;

  function segmentText(text) {
    const segments = [];
    let last = 0;
    LTR_RUN.lastIndex = 0;
    for (let match = LTR_RUN.exec(text); match; match = LTR_RUN.exec(text)) {
      const run = match[0].replace(TRAILING_PUNCTUATION, '');
      if (!run) {
        continue;
      }
      if (match.index > last) {
        segments.push({ text: text.slice(last, match.index), ltr: false });
      }
      segments.push({ text: run, ltr: true });
      last = match.index + run.length;
      LTR_RUN.lastIndex = last;
    }
    if (last < text.length) {
      segments.push({ text: text.slice(last), ltr: false });
    }
    return segments;
  }

  function start() {
    const root = document.documentElement;
    const catalogs = { en: new Map(), ar: new Map() };
    const i18n = ScarfeeI18n.createI18n(catalogs, root, 'en');
    const t = i18n.t;

    const conversationList = document.getElementById('conversation-list');
    const messageList = document.getElementById('message-list');
    const input = document.getElementById('message-input');
    const languageSelect = document.getElementById('language');

    let nextId = 1;
    const conversations = [];
    let active = null;

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

    function newConversation() {
      active = { id: nextId++, messages: [] };
      conversations.unshift(active);
      render();
      input.focus();
    }

    function conversationTitle(conversation) {
      const first = conversation.messages.find((m) => m.role === 'user');
      return first ? first.text : t('assistant-new-conversation');
    }

    function renderConversations() {
      conversationList.replaceChildren();
      for (const conversation of conversations) {
        const item = el('li');
        const button = el('button', 'conversation-item', conversationTitle(conversation));
        button.type = 'button';
        button.dir = 'auto';
        button.setAttribute('aria-current', String(conversation === active));
        button.addEventListener('click', () => {
          active = conversation;
          render();
        });
        item.append(button);
        conversationList.append(item);
      }
    }

    function renderText(container, text) {
      for (const segment of segmentText(text)) {
        if (segment.ltr) {
          const run = el('bdi', null, segment.text);
          run.dir = 'ltr';
          container.append(run);
        } else {
          container.append(document.createTextNode(segment.text));
        }
      }
    }

    function renderMessage(isUser, text) {
      const item = el('li', isUser ? 'message message-user' : 'message message-assistant');
      const meta = el('div', 'message-meta');
      const role = isUser ? t('assistant-role-user') : t('assistant-role-assistant');
      meta.append(el('span', null, role));
      if (!isUser) {
        meta.append(el('span', 'stub-badge', t('assistant-stub-badge')));
      }
      const body = el('p', 'message-text');
      body.dir = 'auto';
      renderText(body, text);
      item.append(meta, body);
      return item;
    }

    function renderMessages() {
      messageList.replaceChildren();
      if (active.messages.length === 0) {
        messageList.append(renderMessage(false, t('assistant-greeting')));
        return;
      }
      for (const message of active.messages) {
        const isUser = message.role === 'user';
        messageList.append(renderMessage(isUser, isUser ? message.text : t('assistant-stub-reply')));
      }
      messageList.scrollTop = messageList.scrollHeight;
    }

    function render() {
      renderConversations();
      renderMessages();
    }

    function send() {
      const text = input.value.trim();
      if (!text) {
        return;
      }
      active.messages.push({ role: 'user', text }, { role: 'stub' });
      input.value = '';
      render();
    }

    function setLanguage(lang) {
      if (i18n.setLanguage(lang)) {
        i18n.translate(document);
        render();
      }
    }

    document.getElementById('composer').addEventListener('submit', (event) => {
      event.preventDefault();
      send();
    });
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
        event.preventDefault();
        send();
      }
    });
    document.getElementById('new-conversation').addEventListener('click', newConversation);
    languageSelect.addEventListener('change', () => setLanguage(languageSelect.value));

    const loads = Object.keys(catalogs).map((lang) =>
      fetch(`../../locales/${lang}/scarfeeos.ftl`)
        .then((response) => response.text())
        .then((source) => {
          catalogs[lang] = ScarfeeI18n.parseFtl(source);
        })
        .catch((error) => console.warn(lang, error))
    );
    Promise.all(loads).then(() => {
      newConversation();
      setLanguage(languageSelect.value);
    });
  }

  if (typeof document === 'undefined') {
    module.exports = { segmentText };
  } else {
    start();
  }
})();
