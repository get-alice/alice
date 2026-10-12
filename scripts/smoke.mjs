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
  cli('accounts', 'demo'); cli('accounts', 'demo');
  assert.equal((await api('DELETE', '/v1/accounts/demo')).status, 400);
  const invalid = await api('PATCH', '/v1/messages/demo-1', { folder: 'deleted' }); assert.equal(invalid.status, 400);
  const protectedMail = await api('DELETE', '/v1/messages/demo-1'); assert.equal(protectedMail.status, 400);
  const original = JSON.parse(cli('list', '--json')); assert.equal(original.total, 13);
  const suggested = JSON.parse(cli('contacts', 'search', 'soph', '--json')); assert.equal(suggested[0].email, 'sophie@form.studio');
  assert.equal(JSON.parse(cli('read', 'demo-13', '--json')).invite.method, 'REQUEST');
  cli('invite', 'demo-13', 'decline');
  assert.equal((await api('GET', '/v1/messages/demo-13')).data.invite.response, 'declined');
  const eventFile = await api('POST', '/v1/calendar/ics', { message_id: 'demo-13' }); assert.ok(eventFile.data.ics.includes('BEGIN:VEVENT') && !eventFile.data.ics.includes('METHOD:REQUEST'));
  cli('archive', 'demo-2');
  const archived = await api('GET', '/v1/messages?folder=archive'); assert.equal(archived.data.messages[0].id, 'demo-2');
  const star = await api('PATCH', '/v1/messages/demo-2', { is_starred: true }); assert.equal(star.status, 200);
  const starred = JSON.parse(cli('list', '--folder', 'starred', '--json')); assert.ok(starred.messages.some(m => m.id === 'demo-2'));
  cli('reply', 'demo-1', '--body', 'Thursday works. See you at the studio.');
  const drafts = JSON.parse(cli('list', '--folder', 'drafts', '--json')); assert.equal(drafts.total, 1);
  assert.equal(drafts.messages[0].in_reply_to, '<demo-1@alice.local>');
  const id = drafts.messages[0].id; cli('send', id); cli('send', id);
  assert.equal(JSON.parse(cli('list', '--folder', 'sent', '--json')).total, 1);
  const frozen = await api('POST', '/v1/messages', { id, account_id: 'demo', to: ['sophie@form.studio'], subject: 'Changed', body: 'Cannot change a sent message', in_reply_to: null }); assert.equal(frozen.status, 400);
  cli('reply', 'demo-3', '--body', 'Draft to discard');
  const disposable = JSON.parse(cli('list', '--folder', 'drafts', '--json')).messages[0].id;
  cli('discard', disposable);
  assert.equal(JSON.parse(cli('list', '--folder', 'drafts', '--json')).total, 0);
  const ai = await api('POST', '/v1/ai', { prompt: 'Summarize this', message_id: 'demo-1' }); assert.equal(ai.status, 400); assert.match(ai.data.error.message, /Connect an AI provider/);
  const settings = await api('POST', '/v1/ai/settings', { provider: 'compatible', model: 'test', base_url: 'http://untrusted.invalid/v1', api_key: 'not-a-real-key' }); assert.equal(settings.status, 400);
  const controlId = crypto.randomUUID();
  const unsafeText = '\u001b[2Jmail\u009b31m\u001b]52;c;fixture\u0007\nReadable line';
  const unfinished = { id: controlId, account_id: 'demo', to: ['unfinished@'], subject: unsafeText, body: unsafeText, in_reply_to: null };
  assert.equal((await api('POST', '/v1/messages', unfinished)).status, 200);
  assert.equal((await api('POST', `/v1/messages/${controlId}/send`, {})).status, 400);
  assert.equal((await api('POST', '/v1/messages', { ...unfinished, to: ['support@example.com'] })).status, 200);
  const human = cli('read', controlId);
  assert.ok(!human.includes('\u001b') && !human.includes('\u009b') && !human.includes('\u0007'));
  assert.ok(human.includes('Readable line'));
  assert.equal(JSON.parse(cli('read', controlId, '--json')).body, unsafeText);
  assert.ok(!cli('list', '--folder', 'drafts').includes('\u001b'));
  cli('discard', controlId);
  // Signal termination leaves exitCode null; repeated cleanup must not wait for a past exit.
  await stop('SIGKILL'); await stop(); start(); await ready();
  assert.equal(JSON.parse(cli('list', '--folder', 'archive', '--json')).messages[0].id, 'demo-2');
  assert.equal(JSON.parse(cli('list', '--folder', 'sent', '--json')).total, 1);
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
    assert.equal(JSON.parse(cli('list', '--folder', 'sent', '--json')).total, 1);
  } finally {
    assert.equal((await api('POST', '/v1/shutdown')).status, 200);
  }
} finally { await stop(); rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); }
console.log('Alice smoke passed: auth, draft validation, shared CLI/API state, recipient suggestions, invitation replies and calendar files, reply drafts, send idempotency, terminal control sanitization, restart persistence, terminal isolation, and graceful shutdown.');
