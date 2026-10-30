// Development bridge, until Tauri is added. Serves shell/ui/ and locales/ on loopback and talks to
// one `scarfeeos-core serve` child over JSON lines (see core/src/ipc.rs).
//
//   node shell/dev-bridge.mjs --core path/to/scarfeeos-core      (or set SCARFEEOS_CORE)
//
// Then open http://127.0.0.1:8770/. Standard library only. Not for production use.

import http from 'node:http';
import fs from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { StringDecoder } from 'node:string_decoder';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HOST = '127.0.0.1';
const DEFAULT_PORT = 8770;
const DEFAULT_TIMEOUT_MS = 5 * 60 * 1000;
const MAX_BODY_BYTES = 64 * 1024;
const LANGUAGES = new Set(['en', 'ar']);
const CONTENT_TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.ftl': 'text/plain; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
};

const shellDir = path.dirname(fileURLToPath(import.meta.url));

class BridgeError extends Error {
  constructor(message, status = 502) {
    super(message);
    this.status = status;
  }
}

// Owns the single core child. Requests are written one at a time, in order, and matched by id.
function createCore({ corePath, timeoutMs }) {
  let current = null; // { child, buffer, decoder }
  let pending = null; // { id, state, finish(error, value) }
  let nextId = 1;
  let queue = Promise.resolve();

  function fail(state, message) {
    if (current === state) {
      current = null;
    }
    if (pending && pending.state === state) {
      pending.finish(new BridgeError(message));
    }
  }

  function onLine(state, line) {
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      console.error(`core: ignoring a line that is not JSON: ${line.slice(0, 200)}`);
      return;
    }
    if (!pending || pending.state !== state || !message || typeof message !== 'object') {
      console.error('core: ignoring a response nobody is waiting for');
      return;
    }
    // The core answers a line it cannot read with id null. It sent exactly one line, so that is ours.
    if (message.id === pending.id || message.id === null) {
      pending.finish(null, message);
    } else {
      console.error(`core: ignoring a response for id ${JSON.stringify(message.id)}, waiting for ${pending.id}`);
    }
  }

  function start() {
    const child = spawn(corePath, ['serve'], { stdio: ['pipe', 'pipe', 'inherit'] });
    const state = { child, buffer: '', decoder: new StringDecoder('utf8') };
    child.stdout.on('data', (chunk) => {
      state.buffer += state.decoder.write(chunk);
      for (let end = state.buffer.indexOf('\n'); end >= 0; end = state.buffer.indexOf('\n')) {
        const line = state.buffer.slice(0, end).trim();
        state.buffer = state.buffer.slice(end + 1);
        if (line) {
          onLine(state, line);
        }
      }
    });
    child.stdin.on('error', () => {});
    child.on('error', (error) => fail(state, `cannot start the core: ${error.message}`));
    child.on('close', (code, signal) => fail(state, `the core stopped (${signal || `exit code ${code}`}); it will be restarted for the next request`));
    return state;
  }

  function exchange(text, language) {
    if (!current) {
      current = start();
    }
    const state = current;
    const id = nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        fail(state, `the core did not answer within ${Math.round(timeoutMs / 1000)} seconds; it was stopped`);
        state.child.kill('SIGKILL');
      }, timeoutMs);
      pending = {
        id,
        state,
        finish(error, value) {
          clearTimeout(timer);
          pending = null;
          if (error) {
            reject(error);
          } else {
            resolve(value);
          }
        },
      };
      state.child.stdin.write(`${JSON.stringify({ id, text, language })}\n`);
    });
  }

  return {
    ask(text, language) {
      const run = () => exchange(text, language);
      const result = queue.then(run, run);
      queue = result.catch(() => {});
      return result;
    },
    stop() {
      const state = current;
      if (state) {
        fail(state, 'the bridge is shutting down');
        state.child.kill('SIGKILL');
      }
    },
  };
}

// Maps a URL path to a file inside `root`. Returns null when it does not exist; throws on anything that
// leaves `root`, including through a symlink.
async function resolveInside(root, urlPath) {
  const segments = urlPath.split('/').filter((s) => s !== '' && s !== '.');
  if (segments.some((s) => s === '..' || s.includes('\0') || s.includes('\\'))) {
    throw new BridgeError('forbidden', 403);
  }
  const rootReal = await fs.realpath(root);
  const target = path.join(rootReal, ...segments);
  let real;
  try {
    real = await fs.realpath(target);
    if ((await fs.stat(real)).isDirectory()) {
      real = await fs.realpath(path.join(real, 'index.html'));
    }
  } catch (error) {
    if (error.code === 'ENOENT' || error.code === 'ENOTDIR') {
      return null;
    }
    throw error;
  }
  if (real !== rootReal && !real.startsWith(rootReal + path.sep)) {
    throw new BridgeError('forbidden', 403);
  }
  return real;
}

function send(res, status, headers, body) {
  res.writeHead(status, { 'X-Content-Type-Options': 'nosniff', 'Cache-Control': 'no-store', ...headers });
  res.end(body);
}

function sendJson(res, status, value) {
  send(res, status, { 'Content-Type': 'application/json; charset=utf-8' }, JSON.stringify(value));
}

function sendError(res, error) {
  const status = error instanceof BridgeError ? error.status : 500;
  sendJson(res, status, { kind: 'error', message: error instanceof BridgeError ? error.message : 'internal error' });
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    req.on('data', (chunk) => {
      size += chunk.length;
      if (size <= MAX_BODY_BYTES) {
        chunks.push(chunk);
      }
    });
    req.on('end', () => {
      if (size > MAX_BODY_BYTES) {
        reject(new BridgeError('request body is too large', 413));
      } else {
        resolve(Buffer.concat(chunks).toString('utf8'));
      }
    });
    req.on('error', reject);
  });
}

export function createBridge({
  corePath,
  port = DEFAULT_PORT,
  timeoutMs = DEFAULT_TIMEOUT_MS,
  uiDir = path.join(shellDir, 'ui'),
  localesDir = path.join(shellDir, '..', 'locales'),
}) {
  const core = createCore({ corePath, timeoutMs });
  let allowedHosts = new Set();

  async function chat(req, res) {
    if (!/^application\/json\s*(;|$)/i.test(req.headers['content-type'] || '')) {
      throw new BridgeError('content type must be application/json', 415);
    }
    let body;
    try {
      body = JSON.parse(await readBody(req));
    } catch (error) {
      if (error instanceof BridgeError) {
        throw error;
      }
      throw new BridgeError('request body is not valid JSON', 400);
    }
    if (!body || typeof body.text !== 'string' || typeof body.language !== 'string' || !LANGUAGES.has(body.language)) {
      throw new BridgeError('expected {"text": string, "language": "en" | "ar"}', 400);
    }
    sendJson(res, 200, await core.ask(body.text, body.language));
  }

  async function serveStatic(req, res, pathname) {
    const isLocales = pathname === '/locales' || pathname.startsWith('/locales/');
    const file = await resolveInside(isLocales ? localesDir : uiDir, isLocales ? pathname.slice('/locales'.length) : pathname);
    if (!file) {
      throw new BridgeError('not found', 404);
    }
    res.writeHead(200, {
      'Content-Type': CONTENT_TYPES[path.extname(file)] || 'application/octet-stream',
      'Content-Length': (await fs.stat(file)).size,
      'Cache-Control': 'no-store',
      'X-Content-Type-Options': 'nosniff',
    });
    if (req.method === 'HEAD') {
      res.end();
    } else {
      createReadStream(file).on('error', () => res.destroy()).pipe(res);
    }
  }

  async function handle(req, res) {
    // Refuses requests that reached loopback by way of another site (DNS rebinding, cross-site forms).
    const origin = req.headers.origin;
    if (!allowedHosts.has(req.headers.host) || (origin !== undefined && !allowedHosts.has(origin.replace(/^http:\/\//, '')))) {
      throw new BridgeError('forbidden', 403);
    }
    let pathname;
    try {
      pathname = decodeURIComponent(req.url.split(/[?#]/)[0]);
    } catch {
      throw new BridgeError('bad request path', 400);
    }
    if (!pathname.startsWith('/')) {
      throw new BridgeError('bad request path', 400);
    }
    if (pathname === '/chat') {
      if (req.method !== 'POST') {
        throw new BridgeError('use POST', 405);
      }
      return chat(req, res);
    }
    if (req.method !== 'GET' && req.method !== 'HEAD') {
      throw new BridgeError('method not allowed', 405);
    }
    return serveStatic(req, res, pathname);
  }

  const server = http.createServer((req, res) => {
    handle(req, res).catch((error) => {
      if (!(error instanceof BridgeError)) {
        console.error(error);
      }
      if (res.headersSent) {
        res.destroy();
      } else {
        sendError(res, error);
      }
    });
  });

  return {
    server,
    start() {
      return new Promise((resolve, reject) => {
        server.once('error', reject);
        server.listen(port, HOST, () => {
          server.off('error', reject);
          const actual = server.address().port;
          allowedHosts = new Set([`${HOST}:${actual}`, `localhost:${actual}`]);
          resolve(actual);
        });
      });
    },
    stop() {
      core.stop();
      return new Promise((resolve) => {
        server.close(() => resolve());
        server.closeAllConnections();
      });
    },
  };
}

function parseArgs(argv) {
  let core = process.env.SCARFEEOS_CORE;
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--core' && i + 1 < argv.length) {
      core = argv[(i += 1)];
    } else if (argv[i].startsWith('--core=')) {
      core = argv[i].slice('--core='.length);
    } else {
      return null;
    }
  }
  return core ? { corePath: core } : null;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!options) {
    console.error('usage: node shell/dev-bridge.mjs --core <path to scarfeeos-core>   (or set SCARFEEOS_CORE)');
    process.exit(2);
  }
  const bridge = createBridge(options);
  for (const signal of ['SIGINT', 'SIGTERM']) {
    process.on(signal, () => bridge.stop().then(() => process.exit(0)));
  }
  try {
    const port = await bridge.start();
    console.log(`ScarfeeOS dev bridge: http://${HOST}:${port}/  (core: ${options.corePath})`);
  } catch (error) {
    console.error(`cannot listen on ${HOST}:${DEFAULT_PORT}: ${error.message}`);
    process.exit(1);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
