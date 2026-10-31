// Oracle: what `@arkade-os/swap` 0.1.0-rc.34 on `@arkade-os/sdk` 0.5.0-rc.31 computes for a
// throwaway wallet and real mainnet quotes from `beta-solver`. Moves no money: it asks for one
// receive quote and one send quote (for the receive quote's own hold invoice) and pays nothing.
//
//   node src/modules/arkade/intents/oracle/capture.mjs > src/modules/arkade/intents/fixtures/oracle.json
//
// `oracle/node_modules` is a local, gitignored symlink to the Bitkit bridge's node_modules.
//
// Uses ORACLE_MNEMONIC when set (a throwaway), else generates one. The mnemonic lands in the
// output: it is test data for a wallet that never holds funds.
import {
    BITCOIN_EMULATOR_PUBKEY,
    InMemoryContractRepository,
    InMemoryWalletRepository,
    MnemonicIdentity,
    Wallet,
    configureEventSource,
    networkFromArkadeInfo,
    provisionClaimSecret,
    provisionRefundKey,
    toXOnly,
    buildPreimageMessage,
    buildSaltedPreimageMessage,
    isPerArtifactDescriptor,
} from "@arkade-os/sdk";
import {
    deriveLightningReceive,
    lightningReceiveRequest,
    lightningSendContract,
    lightningSendRequest,

    unilateralClaimDelay,
} from "@arkade-os/swap/protocol";
import { decodeBolt11 } from "@arkade-os/swap/advanced";
import { nostrRfqTransport, closeSharedNostrPool } from "@arkade-os/swap/nostr";
import { EventSource } from "eventsource";
import { generateMnemonic, mnemonicToSeedSync } from "@scure/bip39";
import { wordlist } from "@scure/bip39/wordlists/english.js";
import { HDKey } from "@scure/bip32";
import { hex } from "@scure/base";
import { readFileSync } from "node:fs";
const newRfqId = () => hex.encode(crypto.getRandomValues(new Uint8Array(32)));

configureEventSource((url) => new EventSource(url));
const log = (...a) => console.error(...a);

const mnemonic = process.env.ORACLE_MNEMONIC ?? generateMnemonic(wordlist);
const root = HDKey.fromMasterSeed(mnemonicToSeedSync(mnemonic));
const path = "86'/0'/1'";
const fp = root.fingerprint.toString(16).padStart(8, "0");
const descriptor = `tr([${fp}/${path}]${root.derive(`m/${path}`).publicExtendedKey}/0/*)`;
const wallet = await Wallet.create({
    identity: MnemonicIdentity.fromMnemonic(mnemonic, { descriptor }),
    arkServerUrl: "https://arkade.computer",
    storage: {
        walletRepository: new InMemoryWalletRepository(),
        contractRepository: new InMemoryContractRepository(),
    },
});
const info = await wallet.getArkadeInfo({ requireLive: true });
const card = JSON.parse(
    readFileSync(new URL("../../bridge/src/beta-solver.card.json", import.meta.url)),
);
const terms = {
    operatorPubkey: toXOnly(hex.decode(info.signerPubkey), "ark signer key"),
    emulatorPubkey: toXOnly(hex.decode(BITCOIN_EMULATOR_PUBKEY), "emulator"),
    claimDelay: unilateralClaimDelay(Number(info.unilateralExitDelay)),
    hrp: networkFromArkadeInfo(info).hrp,
};
log("server", info.signerPubkey, "exit delay", String(info.unilateralExitDelay), "claimDelay", terms.claimDelay);

const transport = nostrRfqTransport({
    relays: card.transports.nostr.relays,
    solverPubkey: card.discovery_pubkey,
});

// ── receive ──────────────────────────────────────────────────────────────────
const secrets = await provisionClaimSecret(wallet);
const paymentHash = hex.encode(secrets.paymentHash);
const payoutAddress = await wallet.getAddress();
const amount = Number(process.env.ORACLE_AMOUNT ?? 1000);
const receiveRequest = {
    ...lightningReceiveRequest({
        rfqId: newRfqId(),
        paymentHash,
        payoutAddress,
        payoutPubkey: secrets.pubkey,
        amount: 0,
        amountSide: "from",
    }),
    amount: String(amount),
};
const receiveQuote = await transport.requestQuote(receiveRequest);
log("receive quote", JSON.stringify(receiveQuote).slice(0, 300));
const derived = deriveLightningReceive({
    quote: receiveQuote,
    paymentHash,
    payoutPubkey: secrets.pubkey,
    payoutAddress,
    ...terms,
});
const invoiceFacts = decodeBolt11(derived.invoice);

// ── send, quoting the receive quote's own hold invoice; nothing is funded ────
const refund = await provisionRefundKey(wallet);
const sendRequest = lightningSendRequest({
    rfqId: newRfqId(),
    invoice: derived.invoice,
    refundAddress: refund.address,
    senderPubkey: refund.pubkey,
});
let send;
try {
    const sendQuote = await transport.requestQuote(sendRequest);
    log("send quote", JSON.stringify(sendQuote).slice(0, 300));
    const base = {
        solverPubkey: toXOnly(hex.decode(sendQuote.solver_pubkey), "solver key"),
        refundLocktime: sendQuote.refund_locktime,
        operatorPubkey: terms.operatorPubkey,
        paymentHash: invoiceFacts.paymentHash,
        claimDelay: terms.claimDelay,
        refundWithoutReceiverDelay: sendQuote.profile.refund_without_receiver_delay,
        emulatorPubkey: terms.emulatorPubkey,
        refundPkScript: refund.pkScript,
        senderPubkey: refund.pubkey,
        receiverPkScript: hex.decode(sendQuote.profile.receiver_pk_script),
    };
    const addressOf = (legacy) =>
        lightningSendContract({ ...base, ...(legacy ? { legacy } : {}) })
            .address(terms.hrp, terms.operatorPubkey)
            .encode();
    send = {
        request: sendRequest,
        quote: sendQuote,
        refund: { descriptor: refund.descriptor, pubkey: hex.encode(refund.pubkey), address: refund.address },
        lockupAddress: addressOf(undefined),
        lockupAddressLegacy: addressOf("preTimelockedRefund"),
    };
} catch (error) {
    log("send quote failed", error?.name, error?.message);
    send = { request: sendRequest, error: `${error?.name}: ${error?.message}` };
}

await transport.close();
closeSharedNostrPool?.();

console.log(
    JSON.stringify(
        {
            comment:
                "Captured by oracle/capture.mjs from @arkade-os/swap 0.1.0-rc.34 against beta-solver on mainnet. Throwaway wallet, no funds.",
            capturedAt: Math.floor(Date.now() / 1000),
            mnemonic,
            walletDescriptor: descriptor,
            server: {
                signerPubkey: info.signerPubkey,
                unilateralExitDelay: String(info.unilateralExitDelay),
                network: info.network,
            },
            terms: {
                operatorPubkey: hex.encode(terms.operatorPubkey),
                emulatorPubkey: hex.encode(terms.emulatorPubkey),
                claimDelay: terms.claimDelay,
                hrp: terms.hrp,
            },
            receive: {
                claimDescriptor: secrets.descriptor,
                claimPubkey: hex.encode(secrets.pubkey),
                // The bridge's wallet runs in the default `walletMode: "auto"` (static), so the
                // claim key is the identity key and P takes the salted arm.
                perArtifactDescriptor: isPerArtifactDescriptor(secrets.descriptor),
                preimageSalt: secrets.preimageSalt ? hex.encode(secrets.preimageSalt) : null,
                mustPersistPreimage: secrets.mustPersistPreimage,
                preimageMessage: hex.encode(
                    secrets.preimageSalt
                        ? buildSaltedPreimageMessage(secrets.pubkey, secrets.preimageSalt)
                        : buildPreimageMessage(secrets.pubkey, 0),
                ),
                preimage: hex.encode(secrets.preimage),
                paymentHash,
                payoutAddress,
                amount,
                request: receiveRequest,
                quote: receiveQuote,
                lockupAddress: derived.address,
                legacy: derived.contractParams.legacy ?? null,
                invoice: invoiceFacts,
            },
            send,
        },
        null,
        2,
    ),
);
process.exit(0);
