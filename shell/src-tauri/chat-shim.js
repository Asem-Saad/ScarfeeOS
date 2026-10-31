// Runs before the panel. The panel posts to /chat; here that goes to the `ask`
// command instead, and the answer comes back shaped like the dev bridge's HTTP
// response. Everything else still reaches the real fetch. The window has no browser
// toolbar, so Alt+Left goes back from the calendar page, as in a browser.
(() => {
  const realFetch = window.fetch.bind(window);
  let nextId = 1;

  function json(status, value) {
    return new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } });
  }

  // The requests the panel may send (see src/request.rs): exactly one kind, with only
  // its own extra fields. Only these fields reach the core, and the id is always the shim's own.
  const KINDS = ['text', 'confirm', 'cancel', 'upcoming', 'delete_event'];
  const EXTRAS = { text: ['language'], confirm: ['time'], upcoming: ['from'] };

  function requestFields(body) {
    if (!body || typeof body !== 'object' || Array.isArray(body)) {
      return null;
    }
    const given = (key) => Object.hasOwn(body, key);
    const kinds = KINDS.filter(given);
    if (kinds.length !== 1) {
      return null;
    }
    const [kind] = kinds;
    const allowed = EXTRAS[kind] || [];
    if (['language', 'time', 'from'].some((key) => given(key) && !allowed.includes(key))) {
      return null;
    }
    const text = (key) => typeof body[key] === 'string';
    switch (kind) {
      case 'text':
        return text('text') && (body.language === 'en' || body.language === 'ar')
          ? { text: body.text, language: body.language }
          : null;
      case 'upcoming':
        return body.upcoming === true && (!given('from') || text('from'))
          ? given('from') ? { upcoming: true, from: body.from } : { upcoming: true }
          : null;
      case 'confirm':
        return text('confirm') && body.confirm && (!given('time') || text('time'))
          ? given('time') ? { confirm: body.confirm, time: body.time } : { confirm: body.confirm }
          : null;
      default:
        return text(kind) && body[kind] ? { [kind]: body[kind] } : null;
    }
  }

  async function chat(init) {
    let body;
    try {
      body = JSON.parse(init.body);
    } catch {
      body = null;
    }
    const fields = requestFields(body);
    if (!fields) {
      return json(400, {
        kind: 'error',
        message:
          'expected {"text": string, "language": "en" | "ar"}, {"confirm": action id, "time"?: string}, {"cancel": action id}, ' +
          '{"upcoming": true, "from"?: string} or {"delete_event": uid}',
      });
    }
    const id = nextId++;
    try {
      const line = await window.__TAURI__.core.invoke('ask', { line: JSON.stringify({ id, ...fields }) });
      return new Response(line, { status: 200, headers: { 'Content-Type': 'application/json' } });
    } catch (error) {
      return json(502, { kind: 'error', message: String(error) });
    }
  }

  window.addEventListener('keydown', (event) => {
    if (event.altKey && event.key === 'ArrowLeft' && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
      event.preventDefault();
      window.history.back();
    }
  });

  window.fetch = (input, init) =>
    input === '/chat' && init && init.method === 'POST' ? chat(init) : realFetch(input, init);
})();
