import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import test from 'node:test';
import { Wallet, concat, toBeHex } from 'ethers';

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

async function signedOperation(nonce) {
  // This is the documented public, unfunded fixture account, never an app wallet.
  const wallet = Wallet.fromPhrase('test test test test test test test test test test test junk');
  const delegate = '0xe6Cae83BdE06E4c305530e199D7217f42808555B';
  const authorization = await wallet.authorize({ chainId: 42161, address: delegate, nonce: 0 });
  const op = {
    sender: wallet.address, nonce: toBeHex(nonce), factory: '0x7702', callData: '0x',
    verificationGasLimit: '0x3d090', callGasLimit: '0x249f0', preVerificationGas: '0x13880',
    maxPriorityFeePerGas: '0x01', maxFeePerGas: '0x64',
    paymaster: '0x888888888888Ec68A58AB8094Cc1AD20Ba3D2402',
    paymasterVerificationGasLimit: '0xc350', paymasterPostOpGasLimit: '0xc350', paymasterData: '0x',
    eip7702Auth: {
      chainId: toBeHex(authorization.chainId), address: authorization.address, nonce: toBeHex(authorization.nonce),
      r: authorization.signature.r, s: authorization.signature.s, yParity: toBeHex(authorization.signature.yParity),
    },
  };
  const types = { PackedUserOperation: [
    ['sender', 'address'], ['nonce', 'uint256'], ['initCode', 'bytes'], ['callData', 'bytes'],
    ['accountGasLimits', 'bytes32'], ['preVerificationGas', 'uint256'], ['gasFees', 'bytes32'], ['paymasterAndData', 'bytes'],
  ].map(([name, type]) => ({ name, type })) };
  op.signature = await wallet.signTypedData({
    name: 'ERC4337', version: '1', chainId: 42161, verifyingContract: '0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108',
  }, types, {
    sender: op.sender, nonce: op.nonce, initCode: delegate, callData: op.callData,
    accountGasLimits: concat([toBeHex(op.verificationGasLimit, 16), toBeHex(op.callGasLimit, 16)]),
    preVerificationGas: op.preVerificationGas,
    gasFees: concat([toBeHex(op.maxPriorityFeePerGas, 16), toBeHex(op.maxFeePerGas, 16)]),
    paymasterAndData: concat([op.paymaster, toBeHex(op.paymasterVerificationGasLimit, 16), toBeHex(op.paymasterPostOpGasLimit, 16), op.paymasterData]),
  });
  return op;
}

function varint(value) {
  let n = BigInt(value); const bytes = [];
  do { let byte = Number(n & 127n); n >>= 7n; if (n) byte |= 128; bytes.push(byte); } while (n);
  return Buffer.from(bytes);
}
function field(number, value) {
  if (typeof value === 'number') return Buffer.concat([varint(number << 3), varint(value)]);
  const bytes = Buffer.from(value); return Buffer.concat([varint((number << 3) | 2), varint(bytes.length), bytes]);
}
async function put(base, token, items) {
  return fetch(`${base}/case/putObjects`, {
    method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/octet-stream' },
    body: Buffer.concat([field(1, 'store'), ...items.map(([key, version, value]) =>
      field(3, Buffer.concat([field(1, key), field(2, version), field(3, value)])))]),
  });
}

test('a second distinct valid operation is refused while identical retries survive restart', { timeout: 15000 }, async () => {
  const directory = await mkdtemp(join(tmpdir(), 'bitkit-native-single-operation-'));
  let server = await start(directory);
  try {
    const original = await signedOperation(0); const replacement = await signedOperation(1);
    const send = (op) => json(server.base, 'case/rpc', { jsonrpc: '2.0', id: 1, method: 'eth_sendUserOperation', params: [op] });
    const accepted = await send(original); assert.match(accepted.body.result, /^0x[0-9a-f]{64}$/);
    assert.equal((await send(original)).body.result, accepted.body.result);
    const rejected = await send(replacement); assert.match(rejected.body.error?.message ?? '', /second distinct/);
    let state = (await json(server.base, 'case/control')).body;
    assert.deepEqual(state.operations, [{ hash: accepted.body.result, operation: original }]);
    assert.equal(state.events.filter(({ type }) => type === 'broadcast').length, 2);
    assert.equal(state.events.filter(({ type }) => type === 'broadcast-rejected').length, 1);
    await stop(server.child); server = await start(directory);
    assert.ok((await send(replacement)).body.error);
    assert.equal((await send(original)).body.result, accepted.body.result);
    state = (await json(server.base, 'case/control')).body;
    assert.deepEqual(state.operations, [{ hash: accepted.body.result, operation: original }]);
    assert.equal(state.events.filter(({ type }) => type === 'broadcast-rejected').length, 2);
  } finally {
    await stop(server.child); await rm(directory, { recursive: true });
  }
});

test('plaintext rejection preserves every record and version in a batch across restart', { timeout: 15000 }, async () => {
  const directory = await mkdtemp(join(tmpdir(), 'bitkit-native-atomic-backup-'));
  let server = await start(directory);
  try {
    const token = await readFile(join(directory, 'public-test-token.txt'), 'utf8');
    assert.equal((await put(server.base, token, [['A', 0, 'encrypted-new'], ['B', 0, 'usdtWallet']])).status, 400);
    assert.deepEqual((await json(server.base, 'case/control')).body.stores, {});
    assert.equal((await put(server.base, token, [['A', 0, 'encrypted-original']])).status, 200);
    const original = (await json(server.base, 'case/control')).body.stores;
    for (const plaintext of ['usdtWallet', 'signed-operation']) {
      assert.equal((await put(server.base, token, [['A', 1, 'encrypted-replacement'], ['B', 0, plaintext]])).status, 400);
      const state = (await json(server.base, 'case/control')).body;
      assert.deepEqual(state.stores, original);
      assert.equal(state.events.at(-1).type, 'backup-invalid');
      assert.deepEqual(JSON.parse(await readFile(join(directory, 'case.json'), 'utf8')).stores, original);
    }
    await stop(server.child); server = await start(directory);
    assert.deepEqual((await json(server.base, 'case/control')).body.stores, original);
    assert.equal((await put(server.base, token, [['A', 1, 'encrypted-replacement'], ['B', 0, 'encrypted-new']])).status, 200);
    const stored = (await json(server.base, 'case/control')).body.stores.store;
    assert.equal(stored.version, 2); assert.equal(stored.records.A.version, 2); assert.equal(stored.records.B.version, 1);
    assert.equal(Buffer.from(stored.records.A.value, 'base64').toString(), 'encrypted-replacement');
    assert.equal(Buffer.from(stored.records.B.value, 'base64').toString(), 'encrypted-new');
  } finally {
    await stop(server.child); await rm(directory, { recursive: true });
  }
});

test('attempt ledger and prepare oracle survive validation errors and restart', { timeout: 15000 }, async () => {
  const directory = await mkdtemp(join(tmpdir(), 'bitkit-native-protocol-'));
  let server = await start(directory);
  try {
    assert.equal((await json(server.base, 'case/control', {
      mode: 'reject', phase: 'prepare', scenario: 'original', faultMode: 'reject', expectedSnapshot: 'original',
    })).status, 200);
    const original = (await json(server.base, 'case/control')).body;
    for (const invalid of [
      { mode: 'accept', phase: 'restore', scenario: 'replacement', expectedSnapshot: 'replacement' },
      { mode: 'commit-drop', faultMode: 'commit-drop' },
      { mode: 'accept', advance: 'invalid' },
      { mode: 'accept', advance: Number.MAX_SAFE_INTEGER },
      { mode: 'accept', type: 'broadcast' },
    ]) {
      assert.equal((await json(server.base, 'case/control', invalid)).status, 400);
      assert.deepEqual((await json(server.base, 'case/control')).body, original);
      assert.deepEqual(JSON.parse(await readFile(join(directory, 'case.json'), 'utf8')), original);
    }
    const blockedWrite = join(directory, 'case.json.tmp');
    await mkdir(blockedWrite);
    assert.equal((await json(server.base, 'case/control', { mode: 'accept', advance: 3 })).status, 400);
    assert.deepEqual((await json(server.base, 'case/control')).body, original);
    assert.deepEqual(JSON.parse(await readFile(join(directory, 'case.json'), 'utf8')), original);
    await rm(blockedWrite, { recursive: true });
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
