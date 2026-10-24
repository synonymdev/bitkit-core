import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, existsSync, writeFileSync, renameSync } from 'node:fs';
import { resolve } from 'node:path';
import { TypedDataEncoder, concat, toBeHex, recoverAddress, hashAuthorization } from 'ethers';

// Protocol fixture only: signs with real native keys but does not execute deployed contracts.
const owner = '0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266';
const token = '0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9';
const paymaster = '0x888888888888Ec68A58AB8094Cc1AD20Ba3D2402';
const delegate = '0xe6Cae83BdE06E4c305530e199D7217f42808555B';
const entryPoint = '0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108';
const types = { PackedUserOperation: [
  ['sender', 'address'], ['nonce', 'uint256'], ['initCode', 'bytes'], ['callData', 'bytes'],
  ['accountGasLimits', 'bytes32'], ['preVerificationGas', 'uint256'], ['gasFees', 'bytes32'], ['paymasterAndData', 'bytes'],
].map(([name, type]) => ({ name, type })) };
function operationHash(op) {
  return TypedDataEncoder.hash({ name: 'ERC4337', version: '1', chainId: 42161, verifyingContract: entryPoint }, types, {
    sender: op.sender, nonce: op.nonce, initCode: delegate, callData: op.callData,
    accountGasLimits: concat([toBeHex(op.verificationGasLimit, 16), toBeHex(op.callGasLimit, 16)]),
    preVerificationGas: op.preVerificationGas,
    gasFees: concat([toBeHex(op.maxPriorityFeePerGas, 16), toBeHex(op.maxFeePerGas, 16)]),
    paymasterAndData: concat([op.paymaster, toBeHex(op.paymasterVerificationGasLimit, 16), toBeHex(op.paymasterPostOpGasLimit, 16), op.paymasterData]),
  });
}

function varint(value) {
  let n = BigInt.asUintN(64, BigInt(value)); const bytes = [];
  do { let b = Number(n & 127n); n >>= 7n; if (n) b |= 128; bytes.push(b); } while (n);
  return Buffer.from(bytes);
}
function field(number, value) {
  if (typeof value === 'number' || typeof value === 'bigint') return Buffer.concat([varint(number << 3), varint(value)]);
  const bytes = Buffer.from(value); return Buffer.concat([varint((number << 3) | 2), varint(bytes.length), bytes]);
}
function decode(bytes) {
  let offset = 0; const values = new Map();
  function readVarint() {
    let n = 0n;
    for (let shift = 0n; shift < 70n; shift += 7n) {
      assert.ok(offset < bytes.length, 'truncated varint'); const b = bytes[offset++]; n |= BigInt(b & 127) << shift;
      if (!(b & 128)) return n;
    }
    throw Error('invalid varint');
  }
  while (offset < bytes.length) {
    const tag = Number(readVarint()); const wire = tag & 7; const key = tag >> 3; let value;
    if (wire === 0) value = BigInt.asIntN(64, readVarint());
    else if (wire === 2) { const length = Number(readVarint()); assert.ok(offset + length <= bytes.length); value = bytes.subarray(offset, offset + length); offset += length; }
    else throw Error(`unsupported wire type ${wire}`);
    values.set(key, [...(values.get(key) ?? []), value]);
  }
  return values;
}
const first = (fields, key) => fields.get(key)?.[0];
const text = (fields, key) => first(fields, key)?.toString() ?? '';
const keyValue = (key, version, value = Buffer.alloc(0)) => Buffer.concat([field(1, key), field(2, version), field(3, value)]);
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const base64url = (value) => Buffer.from(JSON.stringify(value)).toString('base64url');
const directory = resolve(process.argv[2] ?? 'fixture-state');
mkdirSync(directory, { recursive: true });
const tokenFile = `${directory}/public-test-token.txt`;
const jwt = existsSync(tokenFile) ? readFileSync(tokenFile, 'utf8') :
  `${base64url({ alg: 'HS256' })}.${base64url({ exp: Math.floor(Date.now() / 1000) + 86400 })}.${Buffer.from('public-test-token').toString('base64url')}`;
if (!existsSync(tokenFile)) writeFileSync(tokenFile, jwt);
const cases = new Map();
function load(id) {
  assert.match(id, /^[a-z0-9-]{1,64}$/);
  if (!cases.has(id)) {
    const file = `${directory}/${id}.json`;
    cases.set(id, existsSync(file) ? JSON.parse(readFileSync(file)) : {
      id, mode: 'accept', tip: 20000, stores: {}, operations: [], events: [], startedAt: new Date().toISOString(),
    });
  }
  return cases.get(id);
}
function save(state) {
  const file = `${directory}/${state.id}.json`; writeFileSync(`${file}.tmp`, JSON.stringify(state, null, 2)); renameSync(`${file}.tmp`, file);
}
function event(state, type, details = {}) {
  state.events.push({ sequence: state.events.length + 1, type, ...details }); save(state);
}
function rpc(state, method, params) {
  switch (method) {
    case 'eth_chainId': return '0xa4b1';
    case 'eth_blockNumber': return toBeHex(state.tip);
    case 'eth_getBlockByNumber': return { hash: toBeHex(params[0] === 'latest' ? state.tip : BigInt(params[0]), 32), timestamp: toBeHex(Math.floor(Date.now() / 1000)), transactions: [] };
    case 'eth_getCode': return '0x';
    case 'eth_getTransactionCount': return '0x0';
    case 'eth_getBalance': return '0x0';
    case 'eth_call': return toBeHex(params[0].data.startsWith('0x35567e1a') ? 0 : 10_000_000, 32);
    case 'eth_getLogs': return [];
    case 'eth_getTransactionReceipt':
    case 'eth_getUserOperationReceipt': return null;
    case 'pimlico_getTokenQuotes': return { quotes: [{ token, paymaster, postOpGas: '0xc350', balanceSlot: '0x33', exchangeRate: toBeHex(3_000_000_000) }] };
    case 'pimlico_getUserOperationGasPrice': return { slow: { maxFeePerGas: toBeHex(45_000_000), maxPriorityFeePerGas: toBeHex(100000) }, fast: { maxFeePerGas: toBeHex(50_000_000), maxPriorityFeePerGas: toBeHex(100000) } };
    case 'pm_getPaymasterStubData':
    case 'pm_getPaymasterData': {
      const data = Buffer.alloc(118); data[0] = 3;
      if (method === 'pm_getPaymasterData') Buffer.from(toBeHex(Math.floor(Date.now() / 1000) + 600, 6).slice(2), 'hex').copy(data, 2);
      Buffer.from(token.slice(2), 'hex').copy(data, 14);
      Buffer.from(toBeHex(50000, 16).slice(2), 'hex').copy(data, 34);
      Buffer.from(toBeHex(3_000_000_000, 32).slice(2), 'hex').copy(data, 50);
      return { paymaster, paymasterData: `0x${Buffer.concat([data, Buffer.alloc(65, 1)]).toString('hex')}`,
        ...(method === 'pm_getPaymasterStubData' ? { paymasterPostOpGasLimit: '0x186a0' } : { paymasterVerificationGasLimit: '0xc350' }) };
    }
    case 'eth_estimateUserOperationGas': return { callGasLimit: '0x249f0', verificationGasLimit: '0x3d090', preVerificationGas: '0x13880', paymasterVerificationGasLimit: params[0].paymasterData.slice(6, 18) !== '000000000000' ? '0x13880' : '0xc350', paymasterPostOpGasLimit: '0xc350' };
    case 'eth_sendUserOperation': {
      event(state, 'broadcast-attempt', { operation: params[0] });
      const op = params[0]; assert.equal(op.sender.toLowerCase(), owner.toLowerCase(), 'only the public fixture wallet may submit');
      assert.equal(op.factory, '0x7702'); assert.equal(BigInt(op.eip7702Auth.nonce), 0n);
      const hash = operationHash(op); assert.equal(recoverAddress(hash, op.signature).toLowerCase(), owner.toLowerCase());
      const auth = op.eip7702Auth;
      assert.equal(recoverAddress(hashAuthorization({ chainId: auth.chainId, address: auth.address, nonce: auth.nonce }), { r: auth.r, s: auth.s, yParity: Number(BigInt(auth.yParity)) }).toLowerCase(), owner.toLowerCase());
      const existing = state.operations.find((value) => value.hash === hash);
      if (existing) assert.deepEqual(existing.operation, op, 'resubmission must preserve the entire signed payload');
      else state.operations.push({ hash, operation: op });
      event(state, 'broadcast', { hash, payloadSha256: digest(Buffer.from(JSON.stringify(op))) });
      return hash;
    }
    default: throw Error(`unsupported RPC method ${method}`);
  }
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  const [id, endpoint] = url.pathname.split('/').filter(Boolean);
  const json = (value, status = 200) => { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(value)); };
  const protobuf = (body, status = 200) => { res.writeHead(status, { 'Content-Type': 'application/octet-stream' }); res.end(body); };
  const failure = (code, message, status) => protobuf(Buffer.concat([field(1, code), field(2, message)]), status);
  try {
    const state = load(id);
    const chunks = []; let size = 0;
    for await (const chunk of req) { size += chunk.length; assert.ok(size <= 16 * 1024 * 1024); chunks.push(chunk); }
    const body = Buffer.concat(chunks);
    if (endpoint === 'control') {
      if (req.method === 'GET') return json(state);
      const control = JSON.parse(body);
      if (control.mode) { assert.ok(['accept', 'reject', 'commit-drop'].includes(control.mode)); state.mode = control.mode; }
      if (control.phase) { assert.ok(['prepare', 'resume', 'restore'].includes(control.phase)); state.phase = control.phase; }
      if (control.scenario) { assert.match(control.scenario, /^[a-z0-9-]{1,64}$/); state.scenario = control.scenario; }
      if (control.faultMode) {
        assert.ok(['reject', 'commit-drop'].includes(control.faultMode));
        if (state.faultMode) assert.equal(state.faultMode, control.faultMode);
        state.faultMode = control.faultMode;
      }
      if (control.expectedSnapshot) {
        assert.equal(typeof control.expectedSnapshot, 'string');
        if (state.expectedSnapshot) assert.equal(state.expectedSnapshot, control.expectedSnapshot, 'prepare oracle is immutable');
        state.expectedSnapshot = control.expectedSnapshot;
      }
      if (control.advance) state.tip += control.advance;
      event(state, 'control', control); return json({ ok: true });
    }
    if (endpoint === 'auth') {
      const target = new URL(`http://localhost:${server.address().port}/${id}/verify`);
      target.search = new URLSearchParams({ tag: 'login', k1: '11'.repeat(32) }).toString();
      res.writeHead(200, { 'Content-Type': 'text/plain' }); return res.end(target.toString());
    }
    if (endpoint === 'verify') {
      assert.match(url.searchParams.get('key'), /^(02|03)[a-f0-9]{64}$/i);
      assert.match(url.searchParams.get('sig'), /^[a-f0-9]+$/i);
      event(state, 'fixture-auth'); return json({ status: 'OK', token: jwt });
    }
    if (endpoint === 'pubky') { event(state, 'pubky-unavailable'); return json({ error: 'fixture Pubky unavailable' }, 503); }
    if (endpoint === 'rpc') {
      const request = JSON.parse(body);
      try { return json({ jsonrpc: '2.0', id: request.id, result: rpc(state, request.method, request.params) }); }
      catch (error) { event(state, 'rpc-error', { method: request.method, message: error.message }); return json({ jsonrpc: '2.0', id: request.id, error: { code: -32603, message: error.message } }); }
    }
    if (req.headers.authorization !== `Bearer ${jwt}`) return failure(5, 'fixture authentication required', 401);
    const request = decode(body); const storeId = text(request, 1);
    const store = state.stores[storeId] ??= { version: 0, records: {} };
    if (endpoint === 'putObjects') {
      if (state.mode === 'reject') { event(state, 'backup-rejected'); return failure(3, 'injected backup failure', 500); }
      const items = (request.get(3) ?? []).map((item) => decode(item));
      assert.ok(items.length > 0 && !request.has(4), 'fixture supports nonempty put transactions');
      if (request.has(2) && Number(first(request, 2)) !== store.version) return failure(1, 'global version conflict', 409);
      const keys = items.map((item) => text(item, 1)); assert.equal(new Set(keys).size, keys.length);
      for (const item of items) {
        const key = text(item, 1); const version = Number(first(item, 2) ?? 0n);
        if (version !== -1 && version !== (store.records[key]?.version ?? 0)) return failure(1, 'key version conflict', 409);
      }
      for (const item of items) {
        const key = text(item, 1); const value = first(item, 3) ?? Buffer.alloc(0); const version = Number(first(item, 2) ?? 0n);
        assert.ok(!value.includes('usdtWallet') && !value.includes('signed-operation'), 'expected encrypted recovery bytes');
        store.records[key] = { version: version === -1 ? 1 : version + 1, value: value.toString('base64') };
      }
      store.version++; event(state, 'backup-committed', { keys, ciphertextSha256: digest(body) });
      if (state.mode === 'commit-drop') { event(state, 'backup-ack-dropped'); return res.destroy(); }
      event(state, 'backup-acknowledged', { keys }); return protobuf(Buffer.alloc(0));
    }
    if (endpoint === 'getObject') {
      const key = text(request, 2); const item = store.records[key];
      if (!item) return failure(4, 'missing key', 404);
      return protobuf(field(2, keyValue(key, item.version, Buffer.from(item.value, 'base64'))));
    }
    if (endpoint === 'listKeyVersions') {
      const offset = Number(text(request, 4) || 0); const limit = Number(first(request, 3) ?? 100n);
      const keys = Object.keys(store.records).filter((key) => key.startsWith(text(request, 2))).sort();
      const page = keys.slice(offset, offset + limit);
      const encoded = page.map((key) => field(1, keyValue(key, store.records[key].version)));
      if (offset + limit < keys.length) encoded.push(field(2, String(offset + limit)));
      encoded.push(field(3, store.version)); return protobuf(Buffer.concat(encoded));
    }
    throw Error(`unsupported endpoint ${endpoint}`);
  } catch (error) { json({ error: error.message }, 400); }
});

server.listen(Number(process.env.FIXTURE_PORT ?? 0), '127.0.0.1', () => {
  console.log(JSON.stringify({ url: `http://127.0.0.1:${server.address().port}`, directory, evidence: 'native client protocol; no deployed-contract or LNURL signature verification claim' }));
});
