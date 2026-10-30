// Runs before the panel. The panel posts to /chat; here that goes to the `ask`
// command instead, and the answer comes back shaped like the dev bridge's HTTP
// response. Everything else still reaches the real fetch.
(() => {
  const realFetch = window.fetch.bind(window);
  let nextId = 1;

  function json(status, value) {
    return new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } });
  }

  async function chat(init) {
    let body;
    try {
      body = JSON.parse(init.body);
    } catch {
      body = null;
    }
    if (!body || typeof body.text !== 'string' || (body.language !== 'en' && body.language !== 'ar')) {
      return json(400, { kind: 'error', message: 'expected {"text": string, "language": "en" | "ar"}' });
    }
    const id = nextId++;
    try {
      const line = await window.__TAURI__.core.invoke('ask', {
        line: JSON.stringify({ id, text: body.text, language: body.language }),
      });
      return new Response(line, { status: 200, headers: { 'Content-Type': 'application/json' } });
    } catch (error) {
      return json(502, { kind: 'error', message: String(error) });
    }
  }

  window.fetch = (input, init) =>
    input === '/chat' && init && init.method === 'POST' ? chat(init) : realFetch(input, init);
})();
