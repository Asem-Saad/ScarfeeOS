// Installer screens: language, AI tier, and a marked placeholder for disk and account setup
// (docs/installer.md). Everything sits in one function so that, loaded as a classic script beside
// ../ui/i18n.js, it adds no top-level names. Text reaches the page only through textContent.
(function () {
  'use strict';

  const I18n = typeof require === 'function' ? require('../ui/i18n.js') : globalThis.ScarfeeI18n;

  // The only text that is not in locales/: the prompt from docs/installer.md and the two language names,
  // shown before the user has chosen a language.
  const LANGUAGE_PROMPT = 'Select your language / اختر لغتك';
  const LANGUAGES = Object.freeze([
    Object.freeze({ code: 'en', name: 'English' }),
    Object.freeze({ code: 'ar', name: 'العربية' }),
  ]);

  // Weakest first: a tier above the recommended one is more than the hardware can handle.
  const TIERS = Object.freeze(['lite', 'full', 'dual']);
  const TIER_KEYS = Object.freeze({
    lite: Object.freeze({ name: 'installer-tier-lite-name', text: 'installer-tier-lite-text' }),
    full: Object.freeze({ name: 'installer-tier-full-name', text: 'installer-tier-full-text' }),
    dual: Object.freeze({ name: 'installer-tier-dual-name', text: 'installer-tier-dual-text' }),
  });

  const STEPS = Object.freeze(['language', 'tier', 'disk']);

  // The only request the installer ever makes. Nothing here can name a disk, a file or an account.
  const HARDWARE_REQUEST = Object.freeze({ hardware: true });

  const FSI = '\u2068';
  const PDI = '\u2069';
  const PLACEHOLDER = /\{\s*\$([A-Za-z][A-Za-z0-9_-]*)\s*\}/g;

  // Fills `{ $name }` the way Fluent does; each value is isolated so Arabic text cannot reorder what is around it.
  function fill(template, values) {
    return String(template).replace(PLACEHOLDER, (_, name) => {
      const value = values && Object.prototype.hasOwnProperty.call(values, name) ? values[name] : '';
      return FSI + String(value === null || value === undefined ? '' : value) + PDI;
    });
  }

  function exceedsHardware(tier, recommended) {
    return TIERS.indexOf(tier) > TIERS.indexOf(recommended);
  }

  function gigabytes(mib) {
    return String(Math.round((mib / 1024) * 10) / 10);
  }

  const isObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
  const isCount = (value) => Number.isFinite(value) && value >= 0;

  function readPlan(plan) {
    if (!isObject(plan) || !isObject(plan.command) || typeof plan.command.name !== 'string') {
      return null;
    }
    if (plan.writer !== null && !(isObject(plan.writer) && typeof plan.writer.name === 'string')) {
      return null;
    }
    return { models: plan.writer === null ? [plan.command.name] : [plan.command.name, plan.writer.name] };
  }

  // The core's `hardware` response (core/src/ipc.rs), or null when it is not one. The core's own `reason` text is
  // English and is not shown; the page words the basis itself from the numbers.
  function readHardware(response) {
    if (!isObject(response) || response.kind !== 'hardware' || !isObject(response.recommended)) {
      return null;
    }
    const recommended = response.recommended.tier;
    const { cpu, gpu, plans } = response;
    const ram = response.ram_kib;
    if (!TIERS.includes(recommended) || !isObject(cpu) || !isObject(plans) || !(ram === null || isCount(ram))) {
      return null;
    }
    if (gpu !== null && !(isObject(gpu) && typeof gpu.name === 'string' && isCount(gpu.memory_mib))) {
      return null;
    }
    const read = {};
    for (const tier of TIERS) {
      read[tier] = readPlan(plans[tier]);
      if (!read[tier]) {
        return null;
      }
    }
    return {
      ramKib: ram,
      avx2: cpu.avx2 === true,
      avx512f: cpu.avx512f === true,
      gpu: gpu === null ? null : { name: gpu.name, memoryMib: gpu.memory_mib },
      recommended,
      plans: read,
    };
  }

  // STUB. Nothing is written yet: the installer has no way to write a file outside the browser, and the disk
  // and account step is not built. When it is, this is the only place that may write the JSON file.
  function saveChoicesToJsonFile(choices) {
    return { saved: false, json: JSON.stringify(choices, null, 2) };
  }

  function responseFromHttp(status, bodyText) {
    let body;
    try {
      body = JSON.parse(bodyText);
    } catch (error) {
      body = null;
    }
    return isObject(body) && typeof body.kind === 'string' ? body : { kind: 'error' };
  }

  function isHardwareRequest(body) {
    return isObject(body) && Object.keys(body).length === 1 && body.hardware === true;
  }

  // `env`: document, container, i18n (from ScarfeeI18n.createI18n), catalogs (the Maps that i18n reads), ask(body) -> Promise
  // of a core response, save(choices) -> the result of saveChoicesToJsonFile, and optionally history (window.history), so
  // that the browser's Back and Forward move between screens. Returns the controls the page and tests use.
  function createInstaller(env) {
    const { document, i18n, catalogs, ask } = env;
    const save = env.save || saveChoicesToJsonFile;
    const history = env.history || null;
    const t = i18n.t;
    const state = {
      step: 'language',
      language: null,
      tier: null,
      hardware: { phase: 'idle', data: null },
      saveResult: null,
      problem: null,
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

    // `focusKey` names a control so that focus survives the page being drawn again.
    function button(label, className, onClick, focusKey) {
      const node = el('button', `button ${className}`.trim(), label);
      node.type = 'button';
      node.setAttribute('data-focus', focusKey);
      node.addEventListener('click', onClick);
      return node;
    }

    function heading(text) {
      const node = el('h1', 'title', text);
      node.setAttribute('tabindex', '-1');
      node.setAttribute('data-focus', 'heading');
      return node;
    }

    function radio(name, value, checked, onChoose) {
      const input = el('input');
      input.type = 'radio';
      input.name = name;
      input.value = value;
      input.checked = checked;
      input.setAttribute('data-focus', `${name}-${value}`);
      input.addEventListener('change', onChoose);
      input.addEventListener('keydown', (event) => {
        if (event && event.key === 'Enter') {
          next();
        }
      });
      return input;
    }

    function alertNotice(text) {
      const node = el('p', 'notice notice-warning', text);
      node.setAttribute('role', 'alert');
      return node;
    }

    // Both languages side by side, for the one button that is shown before a language is chosen.
    function bilingual(key) {
      return LANGUAGES.map((language) => (catalogs[language.code].has(key) ? catalogs[language.code].get(key) : key)).join(' / ');
    }

    function answer(yes) {
      return t(yes ? 'installer-yes' : 'installer-no');
    }

    function size(mib) {
      return fill(t('installer-size-gb'), { value: gigabytes(mib) });
    }

    // A Continue that cannot proceed stays focusable and says why when it is used, instead of being disabled.
    function nav(onBack, forward) {
      const row = el('div', 'nav');
      if (onBack) {
        row.append(button(t('installer-back'), '', onBack, 'back'));
      }
      if (forward) {
        const onward = button(forward.label, 'button-primary nav-forward', forward.onClick, 'forward');
        if (forward.blocked) {
          onward.setAttribute('aria-disabled', 'true');
        }
        row.append(onward);
      }
      return row;
    }

    function chooseLanguage(code) {
      if (!LANGUAGES.some((language) => language.code === code) || !i18n.setLanguage(code)) {
        return false;
      }
      state.language = code;
      state.problem = null;
      render();
      return true;
    }

    function chooseTier(tier) {
      if (!TIERS.includes(tier)) {
        return false;
      }
      state.tier = tier;
      state.problem = null;
      render();
      return true;
    }

    // `how`: 'push' or 'replace' also records the screen in the browser history; null only shows it.
    function goTo(step, how) {
      state.step = step;
      state.problem = null;
      if (history && how) {
        history[how === 'push' ? 'pushState' : 'replaceState']({ installerStep: step }, '');
      }
      render({ focusHeading: true });
      if (step === 'tier' && (state.hardware.phase === 'idle' || state.hardware.phase === 'unavailable')) {
        loadHardware();
      }
    }

    function next() {
      if (state.step === 'language') {
        if (state.language === null) {
          state.problem = 'language';
          render();
          return false;
        }
        goTo('tier', 'push');
        return true;
      }
      if (state.step === 'tier') {
        if (state.tier === null) {
          state.problem = 'tier';
          render();
          return false;
        }
        state.saveResult = save({ language: state.language, tier: state.tier });
        goTo('disk', 'push');
        return true;
      }
      return false;
    }

    function back() {
      const previous = STEPS[STEPS.indexOf(state.step) - 1];
      if (!previous) {
        return false;
      }
      if (history && history.state && history.state.installerStep === state.step) {
        history.back();
      } else {
        goTo(previous, 'replace');
      }
      return true;
    }

    // The browser's Back or Forward. A screen whose choices were not made (a reload, a stale entry) is not shown.
    function popState(event) {
      const wanted = event && event.state && STEPS.includes(event.state.installerStep) ? event.state.installerStep : 'language';
      const limit = state.language === null ? 0 : state.tier === null ? 1 : 2;
      const step = STEPS[Math.min(STEPS.indexOf(wanted), limit)];
      goTo(step, step === wanted ? null : 'replace');
    }

    function loadHardware() {
      loadId += 1;
      const mine = loadId;
      const hadFocus = focusedKey();
      state.hardware = { phase: 'loading', data: null };
      render();
      return Promise.resolve()
        .then(() => ask({ ...HARDWARE_REQUEST }))
        .catch(() => null)
        .then((response) => {
          if (mine !== loadId) {
            return;
          }
          const data = readHardware(response);
          state.hardware = data ? { phase: 'ready', data } : { phase: 'unavailable', data: null };
          if (data && state.tier === null) {
            state.tier = data.recommended;
          }
          render({ focusKey: hadFocus });
        });
    }

    function renderLanguage() {
      const chosen = state.language !== null;
      const title = chosen ? t('language-select-title') : LANGUAGE_PROMPT;
      document.title = title;
      const group = el('div', 'choices');
      group.setAttribute('role', 'radiogroup');
      group.setAttribute('aria-label', title);
      for (const language of LANGUAGES) {
        const label = el('label', 'choice');
        const input = radio('language', language.code, state.language === language.code, () => chooseLanguage(language.code));
        const name = el('span', 'choice-name', language.name);
        name.lang = language.code;
        name.dir = I18n.directionOf(language.code);
        label.append(input, name);
        group.append(label);
      }
      const parts = [heading(title), group];
      if (state.problem === 'language') {
        parts.push(alertNotice(bilingual('installer-choose-language')));
      }
      const label = chosen ? t('installer-continue') : bilingual('installer-continue');
      parts.push(nav(null, { label, blocked: !chosen, onClick: next }));
      return parts;
    }

    function renderHardware() {
      const box = el('section', 'hardware');
      box.append(el('h2', 'section-title', t('installer-hardware-title')));
      const { phase, data } = state.hardware;
      if (phase === 'idle' || phase === 'loading') {
        const status = el('p', 'status', t('installer-hardware-checking'));
        status.setAttribute('role', 'status');
        box.append(status);
      } else if (phase === 'unavailable') {
        box.append(alertNotice(t('installer-hardware-unavailable')), button(t('installer-hardware-retry'), '', loadHardware, 'retry'));
      } else {
        const list = el('ul', 'facts');
        const ram = data.ramKib === null
          ? t('installer-hardware-memory-unknown')
          : fill(t('installer-hardware-memory'), { size: size(data.ramKib / 1024) });
        const gpu = data.gpu === null
          ? t('installer-hardware-gpu-none')
          : fill(t('installer-hardware-gpu'), { name: data.gpu.name, memory: size(data.gpu.memoryMib) });
        const facts = [
          ram,
          fill(t('installer-hardware-avx2'), { answer: answer(data.avx2) }),
          fill(t('installer-hardware-avx512'), { answer: answer(data.avx512f) }),
          gpu,
        ];
        for (const fact of facts) {
          list.append(el('li', '', fact));
        }
        const basis = data.gpu === null
          ? t('installer-tier-basis-no-gpu')
          : fill(t('installer-tier-basis-gpu'), { name: data.gpu.name, memory: size(data.gpu.memoryMib) });
        box.append(list, el('p', 'note', basis));
      }
      return box;
    }

    function renderTier() {
      const title = t('installer-tier-title');
      document.title = title;
      const { phase, data } = state.hardware;
      const ready = phase === 'ready';
      const parts = [heading(title), el('p', 'lead', t('installer-tier-intro')), renderHardware()];

      const group = el('fieldset', 'tiers');
      group.setAttribute('aria-label', title);
      for (const tier of TIERS) {
        const recommended = ready && data.recommended === tier;
        const label = el('label', `tier${recommended ? ' tier-recommended' : ''}`);
        const input = radio('tier', tier, state.tier === tier, () => chooseTier(tier));
        const body = el('span', 'tier-body');
        const head = el('span', 'tier-head');
        head.append(el('span', 'tier-name', t(TIER_KEYS[tier].name)));
        if (recommended) {
          head.append(el('span', 'badge', t('installer-tier-recommended')));
        }
        body.append(head, el('span', 'tier-text', t(TIER_KEYS[tier].text)));
        if (ready) {
          const models = data.plans[tier].models.join(`${t('assistant-list-separator')} `);
          body.append(el('span', 'tier-models', fill(t('installer-tier-models'), { models })));
        }
        label.append(input, body);
        group.append(label);
      }
      parts.push(group);
      if (ready && state.tier !== null && exceedsHardware(state.tier, data.recommended)) {
        parts.push(alertNotice(fill(t('installer-tier-warning'), { tier: t(TIER_KEYS[state.tier].name) })));
      }
      if (state.problem === 'tier') {
        parts.push(alertNotice(t('installer-choose-tier')));
      }
      parts.push(el('p', 'note', t('installer-tier-models-note')), el('p', 'note', t('installer-tier-code-note')));
      parts.push(nav(back, { label: t('installer-continue'), blocked: state.tier === null, onClick: next }));
      return parts;
    }

    function renderDisk() {
      const title = t('installer-disk-title');
      document.title = title;
      const notice = el('section', 'notice notice-not-built');
      notice.setAttribute('role', 'note');
      notice.append(el('p', 'badge', t('installer-not-built')), el('p', '', t('installer-disk-placeholder')));
      return [heading(title), notice, nav(back, null)];
    }

    // Drawing the page again replaces every control, so focus is put back on the control that had it, or on the heading
    // when the screen changes.
    function focusedKey() {
      const active = document.activeElement;
      return active && env.container.contains(active) ? active.getAttribute('data-focus') : null;
    }

    function render(options) {
      const { container } = env;
      const kept = focusedKey() || (options && options.focusKey) || null;
      const parts = state.step === 'language' ? renderLanguage() : state.step === 'tier' ? renderTier() : renderDisk();
      container.replaceChildren(...parts);
      const target = options && options.focusHeading ? container.querySelector('h1') : kept && container.querySelector(`[data-focus="${kept}"]`);
      if (target) {
        target.focus();
      }
    }

    return { render, chooseLanguage, chooseTier, next, back, popState, loadHardware, state };
  }

  function start() {
    const root = document.documentElement;
    const catalogs = { en: new Map(), ar: new Map() };
    const i18n = I18n.createI18n(catalogs, root, 'en');
    const installer = createInstaller({
      document,
      container: document.getElementById('installer'),
      i18n,
      catalogs,
      history: window.history,
      ask(body) {
        if (!isHardwareRequest(body)) {
          return Promise.resolve({ kind: 'error' });
        }
        return fetch('/chat', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        })
          .then((response) => response.text().then((text) => responseFromHttp(response.status, text)))
          .catch(() => ({ kind: 'unreachable' }));
      },
    });
    const loads = Object.keys(catalogs).map((lang) =>
      fetch(`../../locales/${lang}/scarfeeos.ftl`)
        .then((response) => response.text())
        .then((source) => {
          catalogs[lang] = I18n.parseFtl(source);
        })
        .catch((error) => console.warn(lang, error))
    );
    window.history.replaceState({ installerStep: 'language' }, '');
    window.addEventListener('popstate', installer.popState);
    Promise.all(loads).then(() => installer.render());
  }

  const installerApi = {
    LANGUAGE_PROMPT,
    LANGUAGES,
    TIERS,
    STEPS,
    fill,
    exceedsHardware,
    isHardwareRequest,
    readHardware,
    saveChoicesToJsonFile,
    responseFromHttp,
    createInstaller,
  };

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = installerApi;
  } else {
    globalThis.ScarfeeInstaller = installerApi;
  }
  if (typeof document !== 'undefined') {
    start();
  }
})();
