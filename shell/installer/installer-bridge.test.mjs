import test from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createBridge } from '../dev-bridge.mjs';

// The installer is served by the dev bridge, which is the one process that talks to the core. These tests start the
// real bridge (never the real core) with a stand-in core that writes every request line it receives to a log, and check
// that nothing the installer page can send, or that anyone can send to the bridge, reaches a disk.

const FAKE_CORE = `#!/usr/bin/env node
const fs = require('node:fs');
const readline = require('node:readline');
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  fs.appendFileSync(process.env.FAKE_CORE_LOG, line + '\\n');
  const { id } = JSON.parse(line);
  process.stdout.write(JSON.stringify({ id, kind: 'echo' }) + '\\n');
});
`;

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'scarfee-installer-bridge-'));
const fakeCore = path.join(tmp, 'fake-core.js');
const log = path.join(tmp, 'requests.log');
fs.writeFileSync(fakeCore, FAKE_CORE, { mode: 0o755 });
process.env.FAKE_CORE_LOG = log;
test.after(() => fs.rmSync(tmp, { recursive: true, force: true }));

const received = () => (fs.existsSync(log) ? fs.readFileSync(log, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);

// The cookie the installer page hands out when it loads; withBridge takes it for the bridge it starts, as the page does.
let cookie = '';
const cookieOf = (res) => /^scarfeeos_dev_token=[0-9a-f]+/.exec(res.headers.get('set-cookie') || '')?.[0];

async function withBridge(body) {
  fs.rmSync(log, { force: true });
  const bridge = createBridge({ corePath: fakeCore, port: 0 });
  const port = await bridge.start();
  try {
    const page = await fetch(`http://127.0.0.1:${port}/installer/`);
    cookie = cookieOf(page);
    assert.ok(cookie, 'the installer page must hand out the cookie');
    await body(`http://127.0.0.1:${port}`);
  } finally {
    await bridge.stop();
  }
}

const post = (base, body) =>
  fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': 'application/json', Cookie: cookie }, body: JSON.stringify(body) });

test('the installer page and its files are served read-only, from shell/installer/', async () => {
  await withBridge(async (base) => {
    const page = await fetch(`${base}/installer/`);
    assert.strictEqual(page.status, 200);
    assert.match(await page.text(), /installer\.js/);
    for (const file of ['installer.js', 'installer.css', '../ui/i18n.js']) {
      assert.strictEqual((await fetch(new URL(file, `${base}/installer/`))).status, 200, file);
    }
    for (const method of ['PUT', 'DELETE', 'PATCH', 'POST', 'OPTIONS']) {
      for (const url of ['/installer/', '/installer/installer.js', '/installer/new-file.txt', '/locales/en/scarfeeos.ftl', '/']) {
        const res = await fetch(`${base}${url}`, { method, body: ['PUT', 'POST', 'PATCH'].includes(method) ? 'x' : undefined });
        assert.strictEqual(res.status, 405, `${method} ${url}`);
      }
    }
    assert.deepStrictEqual(received(), []);
  });
  assert.ok(!fs.existsSync(path.join(import.meta.dirname, 'new-file.txt')));
});

test('the only request the installer makes reaches the core, and it is the read-only hardware check', async () => {
  await withBridge(async (base) => {
    const res = await post(base, { hardware: true });
    assert.strictEqual(res.status, 200);
    assert.deepStrictEqual(received().map(({ id, ...rest }) => rest), [{ hardware: true }]);
  });
});

test('a disk, install or file request is refused by the bridge and never reaches the core', async () => {
  const requests = [
    { install: true },
    { install: true, disk: '/dev/sda' },
    { format: '/dev/sda' },
    { disk: '/dev/sda' },
    { partition: '/dev/sda', size: 1 },
    { write_file: '/etc/passwd', contents: 'x' },
    { delete_file: '/home' },
    { create_account: 'someone' },
    { save: { language: 'en', tier: 'lite' } },
    { hardware: true, text: 'format the disk', language: 'en' },
    { hardware: 'yes' },
    { hardware: false },
    {},
    [],
    'hardware',
    null,
  ];
  await withBridge(async (base) => {
    for (const body of requests) {
      const res = await post(base, body);
      assert.strictEqual(res.status, 400, JSON.stringify(body));
    }
    assert.deepStrictEqual(received(), []);
  });
});

test('unknown extra fields are dropped before a request reaches the core', async () => {
  await withBridge(async (base) => {
    assert.strictEqual((await post(base, { text: 'hello', language: 'en', disk: '/dev/sda', install: true })).status, 200);
    assert.strictEqual((await post(base, { hardware: true, disk: '/dev/sda', install: true })).status, 200);
    assert.deepStrictEqual(received().map(({ id, ...rest }) => rest), [{ text: 'hello', language: 'en' }, { hardware: true }]);
  });
});

test('/chat takes only POST with a JSON body, so a page or a form on another site cannot send it', async () => {
  await withBridge(async (base) => {
    assert.strictEqual((await fetch(`${base}/chat`, { headers: { Cookie: cookie } })).status, 405);
    const plain = await fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': 'text/plain', Cookie: cookie }, body: '{"hardware":true}' });
    assert.strictEqual(plain.status, 415);
    const form = await fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded', Cookie: cookie }, body: 'hardware=true' });
    assert.strictEqual(form.status, 415);
    const foreign = await fetch(`${base}/chat`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Cookie: cookie, Origin: 'http://evil.example' },
      body: '{"hardware":true}',
    });
    assert.strictEqual(foreign.status, 403);
    assert.deepStrictEqual(received(), []);
  });
});

test('/chat refuses a request without the page cookie, even a well-formed hardware check', async () => {
  await withBridge(async (base) => {
    for (const headers of [{}, { Cookie: 'scarfeeos_dev_token=wrong' }]) {
      const res = await fetch(`${base}/chat`, { method: 'POST', headers: { 'Content-Type': 'application/json', ...headers }, body: '{"hardware":true}' });
      assert.strictEqual(res.status, 403);
    }
    assert.deepStrictEqual(received(), []);
  });
});
