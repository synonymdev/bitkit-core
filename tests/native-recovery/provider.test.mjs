import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import test from 'node:test';

async function start(directory) {
  const child = spawn(process.execPath, ['provider.mjs', directory], {
    cwd: import.meta.dirname, env: { ...process.env, FIXTURE_PORT: '0' }, stdio: ['ignore', 'pipe', 'inherit'],
  });
  const lines = createInterface({ input: child.stdout });
  const [line] = await once(lines, 'line');
  lines.close();
  return { child, base: JSON.parse(line).url };
}
async function stop(child) {
  const exited = once(child, 'exit');
  child.kill('SIGTERM');
  await exited;
}
async function json(base, path, body) {
  const response = await fetch(`${base}/${path}`, body === undefined ? undefined : {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body),
  });
  return { status: response.status, body: await response.json() };
}

test('attempt ledger and prepare oracle survive validation errors and restart', { timeout: 15000 }, async () => {
  const directory = await mkdtemp(join(tmpdir(), 'bitkit-native-protocol-'));
  let server = await start(directory);
  try {
    assert.equal((await json(server.base, 'case/control', { expectedSnapshot: 'original' })).status, 200);
    assert.equal((await json(server.base, 'case/control', { expectedSnapshot: 'replacement' })).status, 400);
    const rpc = await json(server.base, 'case/rpc', { jsonrpc: '2.0', id: 1, method: 'eth_sendUserOperation', params: [{}] });
    assert.ok(rpc.body.error);
    const state = (await json(server.base, 'case/control')).body;
    assert.equal(state.expectedSnapshot, 'original');
    assert.equal(state.operations.length, 0);
    assert.equal(state.events.filter(({ type }) => type === 'broadcast-attempt').length, 1);
    assert.deepEqual(state.events.find(({ type }) => type === 'broadcast-attempt').operation, {});
    const token = await readFile(join(directory, 'public-test-token.txt'), 'utf8');
    await stop(server.child);
    server = await start(directory);
    assert.equal(await readFile(join(directory, 'public-test-token.txt'), 'utf8'), token);
    assert.deepEqual((await json(server.base, 'case/control')).body, state);
  } finally {
    await stop(server.child);
    await rm(directory, { recursive: true });
  }
});
