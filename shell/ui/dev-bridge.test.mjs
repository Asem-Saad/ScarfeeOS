import test from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { createBridge, parseArgs } from '../dev-bridge.mjs';

// A stand-in for `scarfeeos-core serve`. The text of a request picks what it does.
const FAKE_CORE = `#!/usr/bin/env node
const readline = require('node:readline');
let inflight = 0;
const out = (value) => process.stdout.write(JSON.stringify(value) + '\\n');
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  const request = JSON.parse(line);
  const { id, text } = request;
  if (text === undefined) { out({ id, kind: 'echo', request }); return; }
  inflight += 1;
  const reply = () => {
    out({ id, kind: 'reply', text, language: request.language, pid: process.pid, inflight, foreign_words: [] });
    inflight -= 1;
  };
  if (text === 'die') process.exit(3);
  else if (text === 'hang') inflight -= 1;
  else if (text === 'wrong-id-first') { out({ id: id + 1000, kind: 'reply', text: 'stale' }); reply(); }
  else if (text === 'junk-first') { process.stdout.write('not json\\n'); reply(); }
  else if (text === 'null-id') { out({ id: null, kind: 'error', message: 'unreadable' }); inflight -= 1; }
  else if (text.startsWith('slow')) setTimeout(reply, 150);
  else reply();
});
`;

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'scarfee-bridge-'));
const fakeCore = path.join(tmp, 'fake-core.js');
fs.writeFileSync(fakeCore, FAKE_CORE, { mode: 0o755 });
test.after(() => fs.rmSync(tmp, { recursive: true, force: true }));

// The cookie a page load hands out; withBridge sets it for the bridge it starts.
let cookie = '';
const cookieOf = (res) => /^scarfeeos_dev_token=[0-9a-f]+/.exec(res.headers.get('set-cookie') || '')?.[0];

async function withBridge(options, body) {
  const bridge = createBridge({ corePath: fakeCore, port: 0, ...options });
  const port = await bridge.start();
  try {
    cookie = cookieOf(await fetch(`http://127.0.0.1:${port}/`));
    await body(`http://127.0.0.1:${port}`, port);
  } finally {
    await bridge.stop();
  }
}

const chat = (base, text, language = 'en') =>
  fetch(`${base}/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Cookie: cookie },
    body: JSON.stringify({ text, language }),
  }).then(async (res) => ({ status: res.status, body: await res.json() }));

// Sends a raw request so the client cannot tidy up the path or headers.
function raw(port, requestLine, headers = {}) {
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, '127.0.0.1');
    const all = { Host: `127.0.0.1:${port}`, Connection: 'close', ...headers };
    socket.write(`${requestLine}\r\n${Object.entries(all).map(([k, v]) => `${k}: ${v}`).join('\r\n')}\r\n\r\n`);
    let data = '';
    socket.on('data', (chunk) => { data += chunk; });
    socket.on('end', () => resolve({ status: Number(/^HTTP\/1\.1 (\d+)/.exec(data)[1]), data }));
    socket.on('error', reject);
  });
}

test('a request gets the matching response, and Arabic survives the round trip', async () => {
  await withBridge({}, async (base) => {
    const english = await chat(base, 'hello');
    assert.strictEqual(english.status, 200);
    assert.deepStrictEqual([english.body.kind, english.body.text, english.body.language], ['reply', 'hello', 'en']);
    const arabic = await chat(base, 'مرحبا بك، كيف الحال؟', 'ar');
    assert.deepStrictEqual([arabic.body.text, arabic.body.language], ['مرحبا بك، كيف الحال؟', 'ar']);
    assert.ok(arabic.body.id > english.body.id, 'ids are new for every request');
  });
});

test('confirm, confirm with a time, and cancel reach the core with only their own fields', async () => {
  await withBridge({}, async (base) => {
    const post = (body) =>
      fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': 'application/json', Cookie: cookie }, body: JSON.stringify(body) })
        .then(async (res) => ({ status: res.status, body: await res.json() }));
    const sent = async (body) => {
      const { status, body: answer } = await post(body);
      assert.strictEqual(status, 200);
      assert.strictEqual(answer.kind, 'echo');
      const { id, ...fields } = answer.request;
      assert.strictEqual(typeof id, 'number');
      assert.strictEqual(answer.id, id);
      return fields;
    };
    assert.deepStrictEqual(await sent({ confirm: 'abc123' }), { confirm: 'abc123' });
    assert.deepStrictEqual(await sent({ confirm: 'abc123', time: '2026-10-30T15:30' }), { confirm: 'abc123', time: '2026-10-30T15:30' });
    assert.deepStrictEqual(await sent({ cancel: 'abc123' }), { cancel: 'abc123' });
    const forged = await post({ cancel: 'abc123', id: 99, other: true });
    assert.notStrictEqual(forged.body.request.id, 99);
    assert.deepStrictEqual(Object.keys(forged.body.request).sort(), ['cancel', 'id']);
    assert.strictEqual((await chat(base, 'a text turn still works')).body.kind, 'reply');
  });
});

test('upcoming, delete_event and hardware reach the core with only their own fields', async () => {
  await withBridge({}, async (base) => {
    const sent = async (body) => {
      const res = await fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': 'application/json', Cookie: cookie }, body: JSON.stringify(body) });
      assert.strictEqual(res.status, 200, JSON.stringify(body));
      const { id, ...fields } = (await res.json()).request;
      assert.strictEqual(typeof id, 'number');
      return fields;
    };
    assert.deepStrictEqual(await sent({ upcoming: true }), { upcoming: true });
    assert.deepStrictEqual(await sent({ upcoming: true, from: '2026-10-30' }), { upcoming: true, from: '2026-10-30' });
    assert.deepStrictEqual(await sent({ delete_event: 'uid-1@scarfeeos' }), { delete_event: 'uid-1@scarfeeos' });
    assert.deepStrictEqual(await sent({ hardware: true }), { hardware: true });
    assert.deepStrictEqual(await sent({ hardware: true, id: 5 }), { hardware: true });
    for (const body of [
      { upcoming: false }, { upcoming: 1 }, { upcoming: true, from: 20261030 }, { from: '2026-10-30' }, { delete_event: '' }, { delete_event: 7 },
      { delete_event: 'u', from: '2026-10-30' }, { hardware: 'yes' }, { hardware: false }, { hardware: true, from: '2026-10-30' },
      { upcoming: true, hardware: true }, { upcoming: true, cancel: 'a' }, { delete_event: 'u', confirm: 'a' }, { hardware: true, text: 'hi', language: 'en' },
      { upcoming: true, language: 'en' }, { hardware: true, time: '10:00' },
    ]) {
      const res = await fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': 'application/json', Cookie: cookie }, body: JSON.stringify(body) });
      assert.strictEqual(res.status, 400, JSON.stringify(body));
    }
  });
});

test('confirm and cancel requests that are not well formed are refused before the core', async () => {
  await withBridge({}, async (base) => {
    for (const body of [
      { confirm: '' }, { confirm: 5 }, { cancel: null }, { cancel: '' }, { confirm: 'a', time: 7 }, { cancel: 'a', time: '10:00' },
      { confirm: 'a', cancel: 'a' }, { confirm: 'a', text: 'hi', language: 'en' }, { confirm: 'a', language: 'en' }, { text: 'hi', language: 'en', time: '10:00' },
      { time: '2026-10-30T15:30' }, {},
    ]) {
      const res = await fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': 'application/json', Cookie: cookie }, body: JSON.stringify(body) });
      assert.strictEqual(res.status, 400, JSON.stringify(body));
    }
  });
});

test('concurrent requests reach the core one at a time, in order, each with its own answer', async () => {
  await withBridge({}, async (base) => {
    const results = await Promise.all(['slow-a', 'slow-b', 'slow-c'].map((text) => chat(base, text)));
    assert.deepStrictEqual(results.map((r) => r.body.text), ['slow-a', 'slow-b', 'slow-c']);
    assert.deepStrictEqual(results.map((r) => r.body.inflight), [1, 1, 1]);
    assert.ok(results[0].body.id < results[1].body.id && results[1].body.id < results[2].body.id);
  });
});

test('responses with another id, and lines that are not JSON, are skipped', async () => {
  await withBridge({}, async (base) => {
    const wrongId = await chat(base, 'wrong-id-first');
    assert.strictEqual(wrongId.body.text, 'wrong-id-first');
    const junk = await chat(base, 'junk-first');
    assert.strictEqual(junk.body.text, 'junk-first');
  });
});

test('an id-less error from the core answers the request that caused it', async () => {
  await withBridge({}, async (base) => {
    const result = await chat(base, 'null-id');
    assert.deepStrictEqual([result.status, result.body.kind, result.body.message], [200, 'error', 'unreadable']);
    assert.strictEqual((await chat(base, 'after')).body.text, 'after');
  });
});

test('a core that dies is reported for that request and restarted for the next', async () => {
  await withBridge({}, async (base) => {
    const first = await chat(base, 'one');
    const died = await chat(base, 'die');
    assert.strictEqual(died.status, 502);
    assert.strictEqual(died.body.kind, 'error');
    assert.match(died.body.message, /core stopped/);
    const again = await chat(base, 'two');
    assert.strictEqual(again.status, 200);
    assert.strictEqual(again.body.text, 'two');
    assert.notStrictEqual(again.body.pid, first.body.pid);
  });
});

test('requests queued behind a request that kills the core are still answered', async () => {
  await withBridge({}, async (base) => {
    const [a, b] = await Promise.all([chat(base, 'die'), chat(base, 'queued')]);
    assert.strictEqual(a.status, 502);
    assert.deepStrictEqual([b.status, b.body.text], [200, 'queued']);
  });
});

test('a core that never answers is stopped after the timeout and replaced', async () => {
  await withBridge({ timeoutMs: 300 }, async (base) => {
    const first = await chat(base, 'before');
    const hung = await chat(base, 'hang');
    assert.strictEqual(hung.status, 502);
    assert.match(hung.body.message, /did not answer/);
    const after = await chat(base, 'after');
    assert.strictEqual(after.body.text, 'after');
    assert.notStrictEqual(after.body.pid, first.body.pid);
  });
});

test('a core that cannot be started is an error for the request, not a crash', async () => {
  await withBridge({ corePath: path.join(tmp, 'no-such-core') }, async (base) => {
    const result = await chat(base, 'hello');
    assert.strictEqual(result.status, 502);
    assert.match(result.body.message, /cannot start the core/);
    assert.strictEqual((await chat(base, 'again')).status, 502);
  });
});

test('bad chat requests are refused before they reach the core', async () => {
  await withBridge({}, async (base, port) => {
    const post = (body, type = 'application/json') =>
      fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': type, Cookie: cookie }, body }).then((r) => r.status);
    assert.strictEqual(await post('{nope'), 400);
    assert.strictEqual(await post('null'), 400);
    assert.strictEqual(await post(JSON.stringify({ text: 5, language: 'en' })), 400);
    assert.strictEqual(await post(JSON.stringify({ text: 'hi', language: 'fr' })), 400);
    assert.strictEqual(await post(JSON.stringify({ text: 'hi' })), 400);
    assert.strictEqual(await post(JSON.stringify({ text: 'hi', language: 'en' }), 'text/plain'), 415);
    assert.strictEqual(await post(JSON.stringify({ text: 'x'.repeat(70 * 1024), language: 'en' })), 413);
    assert.strictEqual((await fetch(`${base}/chat`, { headers: { Cookie: cookie } })).status, 405);
    assert.strictEqual((await raw(port, 'OPTIONS /chat HTTP/1.1', { Cookie: cookie })).status, 405);
    assert.strictEqual((await chat(base, 'still fine')).status, 200);
  });
});

test('listens on loopback only, and refuses other Host and Origin headers', async () => {
  const bridge = createBridge({ corePath: fakeCore, port: 0 });
  const port = await bridge.start();
  try {
    assert.strictEqual(bridge.server.address().address, '127.0.0.1');
    assert.strictEqual((await raw(port, 'GET / HTTP/1.1')).status, 200);
    assert.strictEqual((await raw(port, 'GET / HTTP/1.1', { Host: 'evil.example' })).status, 403);
    assert.strictEqual((await raw(port, 'GET / HTTP/1.1', { Host: `evil.example:${port}` })).status, 403);
    assert.strictEqual((await raw(port, 'GET / HTTP/1.1', { Origin: 'http://evil.example' })).status, 403);
    assert.strictEqual((await raw(port, 'GET / HTTP/1.1', { Origin: 'null' })).status, 403);
    assert.strictEqual((await raw(port, 'GET / HTTP/1.1', { Origin: `http://localhost:${port}` })).status, 403);
    assert.strictEqual((await raw(port, 'GET / HTTP/1.1', { Host: `localhost:${port}` })).status, 403);
    assert.strictEqual((await raw(port, 'GET / HTTP/1.1', { Origin: `http://127.0.0.1:${port}` })).status, 200);
  } finally {
    await bridge.stop();
  }
});

test('/chat needs the cookie from a page load, the right Host, and a matching Origin if there is one', async () => {
  await withBridge({}, async (base, port) => {
    const body = JSON.stringify({ text: 'hello', language: 'en' });
    const post = (headers) =>
      fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': 'application/json', ...headers }, body }).then(async (res) => ({ status: res.status, text: await res.text() }));
    const rawWithBody = (headers) => {
      const request = net.connect(port, '127.0.0.1');
      const lines = { Host: `127.0.0.1:${port}`, Connection: 'close', 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(body), ...headers };
      request.write(`POST /chat HTTP/1.1\r\n${Object.entries(lines).map(([k, v]) => `${k}: ${v}`).join('\r\n')}\r\n\r\n${body}`);
      return new Promise((resolve) => {
        let data = '';
        request.on('data', (chunk) => { data += chunk; });
        request.on('end', () => resolve(Number(/^HTTP\/1\.1 (\d+)/.exec(data)[1])));
      });
    };

    // The normal flow: load the page, take its cookie, post with it (and with the page's own Origin).
    const page = await fetch(`${base}/`);
    const set = page.headers.get('set-cookie');
    assert.match(set, /^scarfeeos_dev_token=[0-9a-f]{64}; HttpOnly; SameSite=Strict; Path=\//);
    const good = cookieOf(page);
    assert.strictEqual((await post({ Cookie: good })).status, 200);
    assert.strictEqual((await post({ Cookie: good, Origin: `http://127.0.0.1:${port}` })).status, 200);
    assert.strictEqual((await post({ Cookie: `other=1; ${good}; more=2` })).status, 200);

    // No cookie, a wrong cookie, a cookie that is only a prefix or an extension of the right one: refused.
    const refused = [{}, { Cookie: '' }, { Cookie: 'scarfeeos_dev_token=' }, { Cookie: 'scarfeeos_dev_token=wrong' },
      { Cookie: good.slice(0, -1) }, { Cookie: `${good}0` }, { Cookie: good.replace('scarfeeos_dev_token', 'other') }];
    for (const headers of refused) {
      const result = await post(headers);
      assert.strictEqual(result.status, 403, JSON.stringify(headers));
      assert.strictEqual(JSON.parse(result.text).kind, 'error');
    }
    assert.strictEqual((await post({ Cookie: good, Origin: 'http://evil.example' })).status, 403);
    assert.strictEqual((await post({ Cookie: good, Origin: 'null' })).status, 403);
    assert.strictEqual((await post({ Cookie: good, Origin: `http://127.0.0.1:${port + 1}` })).status, 403);
    assert.strictEqual((await post({ Cookie: good, Origin: `https://127.0.0.1:${port}` })).status, 403);
    assert.strictEqual(await rawWithBody({ Cookie: good, Host: 'evil.example' }), 403);
    assert.strictEqual(await rawWithBody({ Cookie: good, Host: `evil.example:${port}` }), 403);
    assert.strictEqual(await rawWithBody({ Cookie: good, Host: '127.0.0.1' }), 403);
    assert.strictEqual(await rawWithBody({ Cookie: good }), 200);
    // Refusing comes before the method check, so an unauthenticated caller learns nothing about the route.
    assert.strictEqual((await fetch(`${base}/chat`)).status, 403);
    assert.strictEqual((await raw(port, 'OPTIONS /chat HTTP/1.1')).status, 403);
    assert.strictEqual((await post({ Cookie: good })).status, 200);
  });
});

test('the token is new on every start and is not the same across bridges', async () => {
  const tokens = [];
  for (let i = 0; i < 2; i += 1) {
    await withBridge({}, async () => { tokens.push(cookie); });
  }
  assert.match(tokens[0], /^scarfeeos_dev_token=[0-9a-f]{64}$/);
  assert.notStrictEqual(tokens[0], tokens[1]);
  // The first bridge's cookie means nothing to the second.
  await withBridge({}, async (base) => {
    const res = await fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': 'application/json', Cookie: tokens[0] }, body: JSON.stringify({ hardware: true }) });
    assert.strictEqual(res.status, 403);
  });
});

test('static files need no cookie, and only HTML files hand one out', async () => {
  await withBridge({}, async (base, port) => {
    for (const p of ['/', '/panel.js', '/panel.css', '/ui/i18n.js', '/locales/ar/scarfeeos.ftl', '/installer/']) {
      const res = await fetch(`${base}${p}`);
      assert.strictEqual(res.status, 200, p);
      assert.strictEqual(Boolean(res.headers.get('set-cookie')), p === '/' || p === '/installer/', p);
    }
    assert.match((await fetch(`${base}/calendar.html`)).headers.get('set-cookie'), /HttpOnly; SameSite=Strict/);
    assert.strictEqual((await raw(port, 'GET /panel.js HTTP/1.1', { Cookie: 'scarfeeos_dev_token=wrong' })).status, 200);
  });
});

test('static files: the panel and the locale files are served', async () => {
  await withBridge({}, async (base) => {
    const index = await fetch(`${base}/`);
    assert.strictEqual(index.status, 200);
    assert.match(index.headers.get('content-type'), /^text\/html; charset=utf-8/);
    assert.match(await index.text(), /<html lang="en"/);
    const js = await fetch(`${base}/panel.js`);
    assert.match(js.headers.get('content-type'), /^text\/javascript/);
    const ftl = await fetch(`${base}/locales/ar/scarfeeos.ftl`);
    assert.strictEqual(ftl.status, 200);
    assert.match(await ftl.text(), /assistant-send = إرسال/);
    assert.strictEqual((await fetch(`${base}/no-such-file.js`)).status, 404);
    assert.strictEqual((await fetch(`${base}/locales/xx/scarfeeos.ftl`)).status, 404);
    assert.strictEqual((await fetch(`${base}/locales/`)).status, 404);
    assert.strictEqual((await fetch(`${base}/`, { method: 'HEAD' })).status, 200);
    assert.strictEqual((await fetch(`${base}/`, { method: 'DELETE' })).status, 405);
  });
});

test('path traversal is refused in every spelling', async () => {
  await withBridge({}, async (base, port) => {
    const secret = fs.readFileSync(new URL('../../README.md', import.meta.url), 'utf8').slice(0, 40);
    const paths = [
      '/../../README.md',
      '/../README.md',
      '/%2e%2e/%2e%2e/README.md',
      '/%2E%2E/README.md',
      '/..%2f..%2fREADME.md',
      '/..%5c..%5cREADME.md',
      '/locales/../../README.md',
      '/locales/%2e%2e/README.md',
      '/locales/..%2fREADME.md',
      '/locales/../shell/dev-bridge.mjs',
      '/ui/../../README.md',
      '/%00',
      '/panel.js%00.png',
    ];
    for (const p of paths) {
      const result = await raw(port, `GET ${p} HTTP/1.1`);
      assert.ok([400, 403, 404].includes(result.status), `${p} gave ${result.status}`);
      assert.ok(!result.data.includes(secret), `${p} leaked a file`);
    }
    for (const p of ['/..%2fREADME.md', '/%2e%2e/README.md', '/locales/%2e%2e/README.md']) {
      assert.strictEqual((await raw(port, `GET ${p} HTTP/1.1`)).status, 403, p);
    }
    assert.strictEqual((await raw(port, 'GET /%ZZ HTTP/1.1')).status, 400);
    assert.strictEqual((await raw(port, 'GET http://evil.example/ HTTP/1.1')).status, 400);
  });
});

test('a symlink that leaves the served directory is refused', async () => {
  const ui = path.join(tmp, 'ui');
  fs.mkdirSync(ui);
  fs.writeFileSync(path.join(ui, 'index.html'), 'ok');
  fs.writeFileSync(path.join(tmp, 'outside.txt'), 'outside');
  fs.symlinkSync(path.join(tmp, 'outside.txt'), path.join(ui, 'link.txt'));
  fs.symlinkSync(tmp, path.join(ui, 'dir-link'));
  await withBridge({ uiDir: ui }, async (base) => {
    assert.strictEqual(await (await fetch(`${base}/`)).text(), 'ok');
    assert.strictEqual((await fetch(`${base}/link.txt`)).status, 403);
    assert.strictEqual((await fetch(`${base}/dir-link/outside.txt`)).status, 403);
  });
});

test('shell/installer/ is served at /installer/, and a missing folder is a 404', async () => {
  const installer = path.join(tmp, 'installer');
  fs.mkdirSync(installer, { recursive: true });
  fs.writeFileSync(path.join(installer, 'index.html'), '<!doctype html><title>installer</title>');
  fs.writeFileSync(path.join(installer, 'installer.js'), 'void 0;');
  fs.writeFileSync(path.join(tmp, 'secret.txt'), 'secret');
  await withBridge({ installerDir: installer }, async (base, port) => {
    const page = await fetch(`${base}/installer/`);
    assert.strictEqual(page.status, 200);
    assert.match(page.headers.get('content-type'), /^text\/html/);
    assert.match(await page.text(), /<title>installer<\/title>/);
    assert.strictEqual((await fetch(`${base}/installer/installer.js`)).status, 200);
    assert.strictEqual((await fetch(`${base}/installer/nothing.js`)).status, 404);
    assert.strictEqual((await raw(port, 'GET /installer/../secret.txt HTTP/1.1')).status, 403);
    assert.strictEqual((await raw(port, 'GET /installer/%2e%2e/secret.txt HTTP/1.1')).status, 403);
    assert.strictEqual((await fetch(`${base}/installerx`)).status, 404);
  });
  await withBridge({ installerDir: path.join(tmp, 'no-such-folder') }, async (base) => {
    assert.strictEqual((await fetch(`${base}/installer/`)).status, 404);
    assert.strictEqual((await fetch(`${base}/`)).status, 200);
  });
});

test('every script and stylesheet a served page names is served, with the right type', async () => {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const installerDir = path.join(here, '..', 'installer');
  const pages = [
    ...fs.readdirSync(here).filter((f) => f.endsWith('.html')).map((f) => (f === 'index.html' ? '/' : `/${f}`)),
    ...fs.readdirSync(installerDir).filter((f) => f.endsWith('.html')).map((f) => (f === 'index.html' ? '/installer/' : `/installer/${f}`)),
  ];
  assert.ok(pages.includes('/') && pages.includes('/installer/'), pages.join(' '));
  const types = { '.js': /^text\/javascript/, '.css': /^text\/css/ };
  await withBridge({}, async (base) => {
    for (const page of pages) {
      const html = await (await fetch(`${base}${page}`)).text();
      const urls = [
        ...[...html.matchAll(/<script\b[^>]*\bsrc="([^"]+)"/g)].map((m) => m[1]),
        ...[...html.matchAll(/<link\b[^>]*\brel="stylesheet"[^>]*\bhref="([^"]+)"/g)].map((m) => m[1]),
      ];
      assert.ok(urls.length > 0, `${page} names no script or stylesheet`);
      for (const url of urls) {
        const res = await fetch(new URL(url, `${base}${page}`));
        assert.strictEqual(res.status, 200, `${page} names ${url}`);
        const type = types[path.extname(url)];
        assert.ok(type, `${page} names ${url}, which this test has no type for`);
        assert.match(res.headers.get('content-type'), type, `${page} names ${url}`);
        await res.arrayBuffer();
      }
    }
  });
});

test('the locale files installer.js fetches are served from /installer/', async () => {
  const source = fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'installer', 'installer.js'), 'utf8');
  const [, template] = source.match(/fetch\(`([^`]*scarfeeos\.ftl)`\)/);
  await withBridge({}, async (base) => {
    for (const lang of ['en', 'ar']) {
      const res = await fetch(new URL(template.replace('${lang}', lang), `${base}/installer/`));
      assert.strictEqual(res.status, 200, lang);
      assert.match(res.headers.get('content-type'), /^text\/plain/);
      assert.match(await res.text(), /=/);
    }
  });
});

test('--port is read, checked, and used by the program', async () => {
  assert.deepStrictEqual(parseArgs(['--core', 'c'], {}), { corePath: 'c', port: 8770 });
  assert.deepStrictEqual(parseArgs(['--core', 'c', '--port', '8771'], {}), { corePath: 'c', port: 8771 });
  assert.deepStrictEqual(parseArgs(['--port=9000', '--core=c'], {}), { corePath: 'c', port: 9000 });
  assert.deepStrictEqual(parseArgs(['--port', '0'], { SCARFEEOS_CORE: 'env-core' }), { corePath: 'env-core', port: 0 });
  for (const bad of [['--port'], ['--port', 'abc'], ['--port=65536'], ['--port', '-1'], ['--port='], ['--prot', '1']]) {
    assert.strictEqual(parseArgs(['--core', 'c', ...bad], {}), null, bad.join(' '));
  }
  assert.strictEqual(parseArgs(['--port', '8771'], {}), null, 'no core');

  const probe = net.createServer();
  await new Promise((resolve) => probe.listen(0, '127.0.0.1', resolve));
  const { port } = probe.address();
  await new Promise((resolve) => probe.close(resolve));
  const child = spawn(process.execPath, [fileURLToPath(new URL('../dev-bridge.mjs', import.meta.url)), '--core', fakeCore, '--port', String(port)], { stdio: ['ignore', 'pipe', 'inherit'] });
  try {
    const line = await new Promise((resolve, reject) => {
      child.stdout.once('data', (chunk) => resolve(String(chunk)));
      child.once('error', reject);
      child.once('exit', (code) => reject(new Error(`bridge exited with ${code}`)));
    });
    assert.ok(line.includes(`http://127.0.0.1:${port}/`), line);
    assert.strictEqual((await fetch(`http://127.0.0.1:${port}/`)).status, 200);
  } finally {
    child.removeAllListeners('exit');
    child.kill('SIGTERM');
  }
});
