import assert from 'node:assert/strict';
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createServer } from 'node:net';
import { once } from 'node:events';

const directory = mkdtempSync(join(tmpdir(), 'alice-smoke-'));
const probe = createServer(); probe.listen(0, '127.0.0.1'); await once(probe, 'listening');
const port = probe.address().port; await new Promise(done => probe.close(done));
const binary = resolve('target/debug', process.platform === 'win32' ? 'alice.exe' : 'alice');
const env = { ...process.env, LC_ALL: 'C', ALICE_DATA_DIR: directory, ALICE_PORT: String(port) };
let daemon;
function start() { daemon = spawn(binary, ['daemon', 'run'], { env, stdio: 'ignore' }); }
async function ready() {
  for (let i = 0; i < 100; i++) {
    try { const token = readFileSync(join(directory, 'daemon.token'), 'utf8').trim(); const r = await fetch(`http://127.0.0.1:${port}/v1/health`, { headers: { Authorization: `Bearer ${token}` } }); if (r.ok) return; } catch { /* waiting for the process */ }
    await new Promise(done => setTimeout(done, 100));
  }
  throw new Error('Daemon did not start');
}
async function api(method, path, body) {
  const token = readFileSync(join(directory, 'daemon.token'), 'utf8').trim();
  const response = await fetch(`http://127.0.0.1:${port}${path}`, { method, headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  return { status: response.status, data: await response.json() };
}
function cli(...args) { return execFileSync(binary, args, { env, encoding: 'utf8' }); }
async function stop(signal = 'SIGINT') { if (daemon && daemon.exitCode === null && daemon.signalCode === null) { const exited = once(daemon, 'exit'); daemon.kill(signal); await exited; } }

try {
  start(); await ready();
  const unauth = await fetch(`http://127.0.0.1:${port}/v1/messages`); assert.equal(unauth.status, 401);
  assert.equal(JSON.parse(cli('list', '--json')).total, 0);
  const invalid = await api('PATCH', '/v1/messages/missing', { folder: 'deleted' }); assert.equal(invalid.status, 400);
  const ai = await api('POST', '/v1/ai', { prompt: 'Summarize this' }); assert.equal(ai.status, 400); assert.match(ai.data.error.message, /Connect an AI provider/);
  const settings = await api('POST', '/v1/ai/settings', { provider: 'compatible', model: 'test', base_url: 'http://untrusted.invalid/v1', api_key: 'not-a-real-key' }); assert.equal(settings.status, 400);
  const mcp = spawn(binary, ['mcp'], { env, stdio: ['pipe', 'pipe', 'inherit'] });
  const replies = []; let buffered = '';
  mcp.stdout.on('data', chunk => { buffered += chunk; let at; while ((at = buffered.indexOf('\n')) >= 0) { replies.push(JSON.parse(buffered.slice(0, at))); buffered = buffered.slice(at + 1); } });
  let rpcId = 0;
  async function rpc(method, params) {
    const id = ++rpcId; mcp.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
    for (let i = 0; i < 100; i++) { const reply = replies.find(r => r.id === id); if (reply) return reply; await new Promise(done => setTimeout(done, 50)); }
    throw new Error(`No MCP reply to ${method}`);
  }
  const tool = async (name, args = {}) => (await rpc('tools/call', { name, arguments: args })).result;
  assert.equal((await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'smoke', version: '0' } })).result.serverInfo.name, 'alice');
  mcp.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n');
  const toolNames = (await rpc('tools/list')).result.tools.map(t => t.name);
  assert.ok(toolNames.includes('draft_email') && toolNames.includes('send_draft'));
  assert.equal((await tool('search_mail')).isError, true);
  cli('mcp', 'read-only');
  assert.equal(JSON.parse((await tool('search_mail', { query: 'from:sophie' })).content[0].text).messages.length, 0);
  assert.equal(JSON.parse((await tool('list_accounts')).content[0].text).length, 0);
  assert.equal((await tool('draft_email', { to: ['sophie@form.studio'], body: 'Not allowed yet' })).isError, true);
  cli('mcp', 'off'); assert.equal((await tool('list_accounts')).isError, true);
  assert.equal((await rpc('nope')).error.code, -32601);
  mcp.stdin.end(); await once(mcp, 'exit'); assert.equal(mcp.exitCode, 0);
  // Signal termination leaves exitCode null; repeated cleanup must not wait for a past exit.
  await stop('SIGKILL'); await stop(); start(); await ready();
  assert.equal(JSON.parse(cli('list', '--json')).total, 0);
  await stop();
  try {
    const launched = spawnSync(binary, ['list', '--json'], { env, encoding: 'utf8', detached: process.platform !== 'win32' });
    assert.equal(launched.status, 0, launched.stderr);
    if (process.platform !== 'win32') {
      try { process.kill(-launched.pid, 'SIGHUP'); }
      catch (error) { if (error.code !== 'ESRCH') throw error; }
    }
    await new Promise(done => setTimeout(done, 100));
    assert.equal((await api('GET', '/v1/health')).status, 200);
    assert.equal(JSON.parse(cli('list', '--json')).total, 0);
  } finally {
    assert.equal((await api('POST', '/v1/shutdown')).status, 200);
  }
} finally { await stop(); rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); }
console.log('Alice smoke passed: auth, request validation, AI settings checks, MCP access modes, restart, terminal isolation, and graceful shutdown.');
