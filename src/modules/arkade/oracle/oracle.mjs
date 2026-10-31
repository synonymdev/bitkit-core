// Oracle for the crate's unit tests: runs Ark Labs' TypeScript SDK (the same packages the Bitkit bridge
// bundles) for one throwaway mnemonic on mainnet and prints the values the Rust crate must reproduce.
//
//   ARKADE_WORKSPACE=<dir> ORACLE_MNEMONIC="..." node src/modules/arkade/oracle/oracle.mjs \
//       > src/modules/arkade/fixtures/oracle.json
//
// ARKADE_WORKSPACE holds `bridge/` (the built Bitkit bridge with its node_modules) and `pwtest/` (playwright-core);
// the script drives the bridge bundle in headless Chromium. Moves no money: it starts the wallet, reads `info` and
// claims or finds the identity's Lightning address.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

if (!process.env.ARKADE_WORKSPACE) throw new Error("set ARKADE_WORKSPACE");
const root = new URL(`file://${process.env.ARKADE_WORKSPACE.replace(/\/?$/, "/")}`);
const mod = (p) => import(fileURLToPath(new URL(p, root)));
const { MnemonicIdentity } = await mod("bridge/node_modules/@arkade-os/sdk/dist/index.js");
const { deriveSessionTokenForIdentity, claimPublicKeyOf, encodeLnurl } = await mod(
    "bridge/node_modules/@arkade-os/lnurl-client/dist/arkade.js",
);
const { HDKey } = await mod("bridge/node_modules/@scure/bip32/index.js");
const { mnemonicToSeedSync, generateMnemonic } = await mod("bridge/node_modules/@scure/bip39/index.js");
const { wordlist } = await mod("bridge/node_modules/@scure/bip39/wordlists/english.js");
const { chromium } = await mod("pwtest/node_modules/playwright-core/index.mjs");

const ARK = "https://arkade.computer";
const DELEGATE = "https://delegate.arkade.money";
const LNURL = "https://lnurl.arkade.sh";
const hex = (b) => Buffer.from(b).toString("hex");

const mnemonic = process.env.ORACLE_MNEMONIC ?? generateMnemonic(wordlist);

// The bridge's descriptorFor(): BIP86 account 1, receive keys /0/*.
const root32 = HDKey.fromMasterSeed(mnemonicToSeedSync(mnemonic));
const path = "86'/0'/1'";
const fingerprint = root32.fingerprint.toString(16).padStart(8, "0");
const descriptor = `tr([${fingerprint}/${path}]${root32.derive(`m/${path}`).publicExtendedKey}/0/*)`;
const identity = MnemonicIdentity.fromMnemonic(mnemonic, { descriptor });

const token = await deriveSessionTokenForIdentity(identity, new URL(LNURL).hostname);
const claimPublicKey = await claimPublicKeyOf(identity);
const xOnlyPublicKey = hex(await identity.xOnlyPublicKey());

const serverInfo = await (await fetch(`${ARK}/v1/info`)).json();
const delegateInfo = await (await fetch(`${DELEGATE}/v1/delegator/info`)).json();

// The bridge itself, as the apps run it.
const dist = new URL("bridge/dist/", root);
const origin = "https://appassets.androidplatform.net";
const browser = await chromium.launch({
    executablePath:
        process.env.HOME +
        "/Library/Caches/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-mac-arm64/chrome-headless-shell",
});
const page = await (await browser.newContext()).newPage();
await page.route(`${origin}/**`, (route) => {
    const name = new URL(route.request().url()).pathname.split("/").pop() || "index.html";
    route.fulfill({ body: readFileSync(new URL(name, dist)), contentType: name.endsWith(".js") ? "text/javascript" : "text/html" });
});
const pending = new Map();
let seq = 0;
await page.exposeFunction("__native", (json) => {
    const msg = JSON.parse(json);
    if (msg.id && pending.has(msg.id)) {
        pending.get(msg.id)(msg);
        pending.delete(msg.id);
    }
});
await page.addInitScript(() => {
    window.BitkitArkNative = { postMessage: (s) => window.__native(s) };
});
const call = async (method, params = {}) => {
    const msg = await new Promise((resolve) => {
        const id = `o${++seq}`;
        pending.set(id, resolve);
        page.evaluate(([id, m, p]) => window.bitkitArk.call(id, m, p), [id, method, JSON.stringify(params)]);
    });
    if (!msg.ok) throw new Error(`${method}: ${msg.error}`);
    return msg.result;
};
await page.goto(`${origin}/arkade/index.html`);
const info = await call("start", { mnemonic, network: "bitcoin", arkServerUrl: ARK, delegateUrl: DELEGATE, lnurlUrl: LNURL });
const address = await call("lightningAddress");
await browser.close();

console.log(
    JSON.stringify(
        {
            mnemonic,
            network: "bitcoin",
            descriptor,
            xOnlyPublicKey,
            claimPublicKey,
            lnurlDomain: new URL(LNURL).hostname,
            token,
            lnurlOfHandle: encodeLnurl(`${LNURL}/.well-known/lnurlp/${address.handle}`),
            server: {
                signerPubkey: serverInfo.signerPubkey,
                network: serverInfo.network,
                unilateralExitDelay: Number(serverInfo.unilateralExitDelay),
                boardingExitDelay: Number(serverInfo.boardingExitDelay),
                dust: Number(serverInfo.dust),
            },
            delegatePubkey: delegateInfo.pubkey,
            info,
            lightningAddress: address,
        },
        null,
        2,
    ),
);
