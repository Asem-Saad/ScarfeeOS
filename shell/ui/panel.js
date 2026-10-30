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
    const settingControls = [
      ['language', languageSelect],
      ['digits', document.getElementById('setting-digits')],
      ['calendar', document.getElementById('setting-calendar')],
      ['weekStart', document.getElementById('setting-week-start')],
    ];
    const settings = ScarfeeSettings.createStore(ScarfeeSettings.browserStorage);
    for (const [key, control] of settingControls) {
      control.value = settings.get()[key];
    }
    const sendButton = document.querySelector('#composer button[type="submit"]');
    const { describeResponse, fill, formatElapsed, responseFromHttp } = ScarfeeRender;

    let nextId = 1;
    const conversations = [];
    let active = null;
    let waiting = false;
    let elapsedNode = null;
    let elapsedTimer = null;

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

    function waitingSeconds(message) {
      return (Date.now() - message.startedAt) / 1000;
    }

    function updateElapsed(message) {
      if (elapsedNode) {
        elapsedNode.textContent = fill(t('assistant-waiting-elapsed'), {
          time: formatElapsed(waitingSeconds(message), settings.get()),
        });
      }
    }

    function renderFiles(block) {
      const list = el('ul', 'message-files');
      for (const file of block.items) {
        const item = el('li', 'message-file');
        const name = el('bdi', null, file.name);
        name.dir = 'auto';
        item.append(name);
        if (file.label) {
          item.append(el('span', 'message-file-kind', file.label));
        }
        list.append(item);
      }
      return list;
    }

    // Confirm and Cancel for a call the core is holding. A clarification also asks for the time.
    function renderActions(block, message) {
      const row = el('div', 'message-actions');
      const locked = message.answered === true || waiting;
      let timeInput = null;
      if (block.clarify) {
        timeInput = el('input', 'message-time');
        timeInput.type = 'time';
        timeInput.value = message.chosenTime || block.clarify.time;
        timeInput.disabled = locked;
        timeInput.setAttribute('aria-label', t('assistant-clarify-time-label'));
        timeInput.addEventListener('input', () => {
          message.chosenTime = timeInput.value;
        });
        row.append(timeInput);
      }
      const confirm = el('button', 'button button-primary', t('assistant-confirm'));
      const cancel = el('button', 'button', t('assistant-cancel'));
      for (const button of [confirm, cancel]) {
        button.type = 'button';
        button.disabled = locked;
        row.append(button);
      }
      confirm.addEventListener('click', () => {
        const chosen = timeInput ? timeInput.value : '';
        answer(message, chosen ? { confirm: block.actionId, time: `${block.clarify.date}T${chosen}` } : { confirm: block.actionId });
      });
      cancel.addEventListener('click', () => answer(message, { cancel: block.actionId }));
      return row;
    }

    function renderMessage(isUser, blocks, pendingMessage, message) {
      const item = el('li', isUser ? 'message message-user' : 'message message-assistant');
      const meta = el('div', 'message-meta');
      meta.append(el('span', null, isUser ? t('assistant-role-user') : t('assistant-role-assistant')));
      item.append(meta);
      for (const block of blocks) {
        if (block.tone === 'files') {
          item.append(renderFiles(block));
        } else if (block.tone === 'actions') {
          item.append(renderActions(block, message));
        } else {
          const body = el('p', `message-text message-${block.tone}`);
          body.dir = 'auto';
          renderText(body, block.text);
          item.append(body);
        }
      }
      if (pendingMessage) {
        item.setAttribute('aria-busy', 'true');
        elapsedNode = el('p', 'message-text message-elapsed');
        elapsedNode.dir = 'auto';
        elapsedNode.setAttribute('aria-hidden', 'true');
        updateElapsed(pendingMessage);
        item.append(elapsedNode);
      }
      return item;
    }

    function renderMessages() {
      messageList.replaceChildren();
      elapsedNode = null;
      if (active.messages.length === 0) {
        messageList.append(renderMessage(false, [{ tone: 'reply', text: t('assistant-greeting') }]));
        return;
      }
      for (const message of active.messages) {
        if (message.role === 'user') {
          messageList.append(renderMessage(true, [{ tone: 'reply', text: message.text }]));
        } else if (message.response) {
          messageList.append(renderMessage(false, describeResponse(message.response, t, i18n.getLanguage(), settings.get()), null, message));
        } else {
          messageList.append(renderMessage(false, [{ tone: 'waiting', text: t('assistant-waiting') }], message));
        }
      }
      messageList.scrollTop = messageList.scrollHeight;
    }

    function render() {
      renderConversations();
      renderMessages();
    }

    function post(body) {
      return fetch('/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })
        .then((response) => response.text().then((text) => responseFromHttp(response.status, text)))
        .catch(() => ({ kind: 'unreachable' }));
    }

    function setWaiting(value) {
      waiting = value;
      sendButton.disabled = value;
    }

    // A language change the core reports goes through the same switch as the language setting.
    function followLanguageResult(response) {
      const result = response.kind === 'run' && response.result;
      if (result && result.ok === true && result.type === 'language' && Object.keys(catalogs).includes(result.lang) && languageSelect.value !== result.lang) {
        languageSelect.value = result.lang;
        changeSetting('language', languageSelect);
      }
    }

    function converse(body) {
      const reply = { role: 'assistant', startedAt: Date.now(), response: null };
      active.messages.push(reply);
      setWaiting(true);
      elapsedTimer = setInterval(() => updateElapsed(reply), 1000);
      render();
      post(body).then((response) => {
        clearInterval(elapsedTimer);
        reply.response = response;
        setWaiting(false);
        followLanguageResult(response);
        render();
      });
    }

    function send() {
      const text = input.value.trim();
      if (!text || waiting) {
        return;
      }
      active.messages.push({ role: 'user', text });
      input.value = '';
      converse({ text, language: languageSelect.value });
    }

    // Each held call can be answered once, so its buttons stay disabled after the first click.
    function answer(message, body) {
      if (message.answered === true || waiting) {
        return;
      }
      message.answered = true;
      converse(body);
    }

    function setLanguage(lang) {
      if (i18n.setLanguage(lang)) {
        i18n.translate(document);
        render();
      }
    }

    // A setting applies at once: messages are drawn again from the stored responses with the new choice.
    function changeSetting(key, control) {
      if (settings.set(key, control.value)) {
        if (key === 'language') {
          setLanguage(control.value);
        } else {
          render();
        }
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
    for (const [key, control] of settingControls) {
      control.addEventListener('change', () => changeSetting(key, control));
    }

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
