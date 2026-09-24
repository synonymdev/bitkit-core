import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import {
  AbiCoder,
  Contract,
  HDNodeWallet,
  JsonRpcProvider,
  concat,
  getBytes,
  keccak256,
  toBeHex,
  zeroPadValue,
} from 'ethers';

// A local service fixture using the deployed contracts with a test paymaster signer.
// It does not model Pimlico's pricing, gas estimation, or service acceptance policy.
const rpc = new JsonRpcProvider('http://127.0.0.1:18545', 42161, {
  staticNetwork: true,
  cacheTimeout: -1,
});
assert.match(await rpc.send('web3_clientVersion', []), /anvil/i);
assert.equal(await rpc.send('eth_chainId', []), '0xa4b1');
const signer = HDNodeWallet.fromPhrase(
  'test test test test test test test test test test test junk',
  undefined,
  "m/44'/60'/0'/0/1",
);
const bundler = await rpc.getSigner(signer.address);
const vector = JSON.parse(
  readFileSync(new URL('../../src/modules/usdt/fixtures/eip7702-vectors.json', import.meta.url)),
);
const account = vector.address;
const delegate = '0xe6Cae83BdE06E4c305530e199D7217f42808555B';
const tokenAddress = '0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9';
const pmAddress = '0x888888888888Ec68A58AB8094Cc1AD20Ba3D2402';
const entryAddress = '0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108';
const packedType =
  '(address sender,uint256 nonce,bytes initCode,bytes callData,bytes32 accountGasLimits,uint256 preVerificationGas,bytes32 gasFees,bytes paymasterAndData,bytes signature)';
const token = new Contract(
  tokenAddress,
  ['function balanceOf(address) view returns (uint256)'],
  rpc,
);
const pm = new Contract(
  pmAddress,
  [
    `function getHash(uint8,${packedType}) view returns (bytes32)`,
    'function signers(address) view returns (bool)',
  ],
  rpc,
);
const entry = new Contract(
  entryAddress,
  [
    `function handleOps(${packedType}[],address)`,
    `function getUserOpHash(${packedType}) view returns (bytes32)`,
    'function depositTo(address) payable',
  ],
  bundler,
);
const coder = AbiCoder.defaultAbiCoder();

async function setMapping(contract, account, value, check) {
  for (let slot = 0; slot < 100; slot++) {
    const key = keccak256(coder.encode(['address', 'uint256'], [account, slot]));
    const previous = await rpc.send('eth_getStorageAt', [contract, key, 'latest']);
    await rpc.send('anvil_setStorageAt', [contract, key, toBeHex(value, 32)]);
    if (await check()) return slot;
    await rpc.send('anvil_setStorageAt', [contract, key, previous]);
  }
  throw Error('Could not locate fixture mapping');
}
await rpc.send('anvil_setCode', [account, '0x']);
await rpc.send('anvil_setNonce', [account, '0x0']);
await rpc.send('anvil_setBalance', [account, '0x0']);
const balanceSlot = await setMapping(
  tokenAddress,
  account,
  1_000_000_000n,
  async () => (await token.balanceOf(account)) === 1_000_000_000n,
);
await setMapping(pmAddress, signer.address, 1n, async () => await pm.signers(signer.address));
await (await entry.depositTo(pmAddress, { value: 10n ** 18n })).wait();

const gas = {
  callGasLimit: toBeHex(1_500_000),
  verificationGasLimit: toBeHex(800_000),
  preVerificationGas: toBeHex(100_000),
  paymasterVerificationGasLimit: toBeHex(100_000),
  paymasterPostOpGasLimit: toBeHex(100_000),
};
const rate = 3_000_000_000n;
function pack(op) {
  return {
    sender: op.sender,
    nonce: op.nonce,
    initCode: concat(['0x7702', toBeHex(0, 18)]),
    callData: op.callData,
    accountGasLimits: concat([toBeHex(op.verificationGasLimit, 16), toBeHex(op.callGasLimit, 16)]),
    preVerificationGas: op.preVerificationGas,
    gasFees: concat([toBeHex(op.maxPriorityFeePerGas, 16), toBeHex(op.maxFeePerGas, 16)]),
    paymasterAndData: concat([
      op.paymaster,
      toBeHex(op.paymasterVerificationGasLimit, 16),
      toBeHex(op.paymasterPostOpGasLimit, 16),
      op.paymasterData,
    ]),
    signature: op.signature,
  };
}
async function dispatch(method, params) {
  if (method === 'eth_estimateUserOperationGas' || method === 'eth_sendUserOperation') {
    const op = params[0];
    assert.equal(op.factory, '0x7702');
    assert.ok(op.eip7702Auth, 'The 7702 factory marker requires authorization');
    assert.equal(
      BigInt(op.eip7702Auth.nonce),
      BigInt(await rpc.send('eth_getTransactionCount', [op.sender, 'latest'])),
      'Authorization must use the current account nonce',
    );
  }
  if (method === 'test_fundWallet') {
    const key = keccak256(coder.encode(['address', 'uint256'], [params[0], balanceSlot]));
    await rpc.send('anvil_setStorageAt', [tokenAddress, key, toBeHex(1_000_000_000n, 32)]);
    return true;
  }
  if (method === 'pimlico_getTokenQuotes')
    return {
      quotes: [
        {
          token: tokenAddress,
          paymaster: pmAddress,
          postOpGas: toBeHex(50000),
          exchangeRate: toBeHex(rate),
        },
      ],
    };
  if (method === 'pimlico_getUserOperationGasPrice')
    return {
      fast: { maxFeePerGas: toBeHex(100_000_000), maxPriorityFeePerGas: toBeHex(1_000_000) },
    };
  if (method === 'eth_estimateUserOperationGas') return gas;
  if (method === 'pm_getPaymasterData' || method === 'pm_getPaymasterStubData') {
    const stub = method === 'pm_getPaymasterStubData';
    const limits = stub ? { paymasterPostOpGasLimit: gas.paymasterPostOpGasLimit } : {};
    const op = { ...params[0], paymaster: pmAddress, ...limits };
    const unsigned = concat([
      '0x0300',
      toBeHex(Math.floor(Date.now() / 1000) + 600, 6),
      toBeHex(0, 6),
      tokenAddress,
      toBeHex(50000, 16),
      toBeHex(rate, 32),
      toBeHex(100000, 16),
      signer.address,
    ]);
    op.paymasterData = concat([unsigned, zeroPadValue('0x01', 65)]);
    const signature = await signer.signMessage(getBytes(await pm.getHash(1, pack(op))));
    return { paymaster: pmAddress, paymasterData: concat([unsigned, signature]), ...limits };
  }
  if (method === 'eth_sendUserOperation') {
    const op = pack(params[0]);
    const hash = await rpc.send('eth_call', [
      { to: entryAddress, data: entry.interface.encodeFunctionData('getUserOpHash', [op]) },
      'latest',
      { [op.sender]: { code: concat(['0xef0100', delegate]) } },
    ]);
    const auth = params[0].eip7702Auth;
    const options = {
      gasLimit: 6_000_000,
      type: 4,
      authorizationList: [
        {
          address: auth.address,
          chainId: BigInt(auth.chainId),
          nonce: BigInt(auth.nonce),
          signature: { r: auth.r, s: auth.s, yParity: Number(BigInt(auth.yParity)) },
        },
      ],
    };
    const tx = await entry.handleOps([op], signer.address, options);
    const receipt = await tx.wait();
    await rpc.send('anvil_mine', [3]);
    console.log(
      JSON.stringify({
        operation: hash,
        transaction: receipt.hash,
        gasUsed: receipt.gasUsed.toString(),
      }),
    );
    return hash;
  }
  return rpc.send(method, params);
}
const server = createServer(async (request, response) => {
  let body = '';
  for await (const chunk of request) body += chunk;
  const call = JSON.parse(body);
  let result;
  try {
    result = { jsonrpc: '2.0', id: call.id, result: await dispatch(call.method, call.params) };
  } catch (error) {
    console.error(call.method, error.shortMessage ?? error.message, error.data ?? '');
    result = {
      jsonrpc: '2.0',
      id: call.id,
      error: { code: -32000, message: error.shortMessage ?? error.message },
    };
  }
  response.writeHead(200, { 'content-type': 'application/json' });
  response.end(JSON.stringify(result));
});
server.listen(18546, '127.0.0.1', () =>
  console.log('Fork provider ready on localhost:18546; no mainnet writes.'),
);
