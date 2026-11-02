//! Spending a lockup offchain: the receive claim (collaborative `claim` leaf: preimage, receiver
//! and server) and the send refund (`refundWithoutReceiver`: sender and server after the CLTV).
//! Built like `ark-client`'s Boltz `claim_vhtlc` / `refund_expired_vhtlc`, on `ark-core`.

use super::error::Error;
use super::error::Result;
use super::vhtlc::VhtlcV2;
use super::wallet::ArkWallet;
use ark_core::send::build_offchain_transactions;
use ark_core::send::sign_ark_transaction;
use ark_core::send::sign_checkpoint_transaction;
use ark_core::send::OffchainTransactions;
use ark_core::send::SendReceiver;
use ark_core::send::VtxoInput;
use ark_core::server::Info;
use ark_core::server::VirtualTxOutPoint;
use ark_core::ArkAddress;
use ark_core::VTXO_CONDITION_KEY;
use bitcoin::absolute::LockTime;
use bitcoin::consensus::Decodable;
use bitcoin::consensus::Encodable;
use bitcoin::hashes::sha256;
use bitcoin::hashes::Hash;
use bitcoin::key::Secp256k1;
use bitcoin::psbt;
use bitcoin::secp256k1::schnorr;
use bitcoin::secp256k1::Keypair;
use bitcoin::secp256k1::Message;
use bitcoin::Amount;
use bitcoin::Psbt;
use bitcoin::Txid;
use bitcoin::VarInt;
use bitcoin::XOnlyPublicKey;

/// PSBT proprietary-ish key type arkd reads the condition witness from.
const CONDITION_KEY_TYPE: u8 = 222;
/// At most this many lockup outputs per spend; the rest go in the next pass.
pub const MAX_LOCKUP_SPEND_INPUTS: usize = 16;

/// Which leaf a spend takes.
pub enum Leaf {
    /// Receive: the trader's claim, revealing `P`.
    Claim { preimage: [u8; 32] },
    /// Send: the trader's refund after `refund_locktime`.
    RefundWithoutReceiver,
}

/// Spend `vtxos` of `lockup` to `destination` in one offchain transaction; returns the Ark txid.
pub async fn spend_lockup<W: ArkWallet + ?Sized>(
    wallet: &W,
    info: &Info,
    lockup: &VhtlcV2,
    vtxos: &[VirtualTxOutPoint],
    keypair: &Keypair,
    leaf: Leaf,
    destination: &ArkAddress,
) -> Result<Txid> {
    if vtxos.is_empty() {
        return Err(Error::Other("nothing to spend".into()));
    }
    let (script, locktime, preimage) = match &leaf {
        Leaf::Claim { preimage } => (lockup.claim_script().clone(), None, Some(*preimage)),
        Leaf::RefundWithoutReceiver => (
            lockup.refund_without_receiver_script().clone(),
            Some(LockTime::from_consensus(lockup.options.refund_locktime)),
            None,
        ),
    };
    let control_block = lockup
        .control_block(&script)
        .ok_or_else(|| Error::Other("leaf missing from the lockup tree".into()))?;
    let inputs: Vec<VtxoInput> = vtxos
        .iter()
        .map(|v| {
            VtxoInput::new(
                script.clone(),
                locktime,
                control_block.clone(),
                lockup.tapscripts(),
                lockup.script_pubkey(),
                v.amount,
                v.outpoint,
                v.assets.clone(),
            )
        })
        .collect();
    let total: Amount = vtxos.iter().map(|v| v.amount).sum();
    let OffchainTransactions {
        mut ark_tx,
        checkpoint_txs,
    } = build_offchain_transactions(
        &[SendReceiver::bitcoin(*destination, total)],
        destination,
        &inputs,
        info,
    )
    .map_err(|e| Error::Other(format!("build offchain tx: {e}")))?;

    let sign = |input: &mut psbt::Input, msg: Message| {
        if let Some(preimage) = preimage {
            attach_condition_witness(input, &preimage);
        }
        let sig = Secp256k1::new().sign_schnorr_no_aux_rand(&msg, keypair);
        Ok::<Vec<(schnorr::Signature, XOnlyPublicKey)>, ark_core::Error>(vec![(
            sig,
            keypair.x_only_public_key().0,
        )])
    };
    for index in 0..ark_tx.inputs.len() {
        sign_ark_transaction(sign, &mut ark_tx, index)
            .map_err(|e| Error::Other(format!("sign ark tx: {e}")))?;
    }
    let ark_txid = ark_tx.unsigned_tx.compute_txid();
    let mut checkpoints = wallet.submit_offchain(ark_tx, checkpoint_txs).await?;
    for checkpoint in &mut checkpoints {
        sign_checkpoint_transaction(sign, checkpoint)
            .map_err(|e| Error::Other(format!("sign checkpoint: {e}")))?;
    }
    wallet.finalize_offchain(ark_txid, checkpoints).await?;
    Ok(ark_txid)
}

/// The preimage as the condition witness: one element, `P`.
fn attach_condition_witness(input: &mut psbt::Input, preimage: &[u8; 32]) {
    let mut bytes = vec![1u8];
    VarInt(preimage.len() as u64)
        .consensus_encode(&mut bytes)
        .expect("vec write");
    bytes.extend_from_slice(preimage);
    input.unknown.insert(
        psbt::raw::Key {
            type_value: CONDITION_KEY_TYPE,
            key: VTXO_CONDITION_KEY.to_vec(),
        },
        bytes,
    );
}

/// Find a 32-byte witness element hashing to `payment_hash` in a spending transaction: in a
/// condition witness or a final witness. This is how a send learns the solver's claim settled.
pub fn preimage_in(tx: &Psbt, payment_hash: &[u8; 32]) -> Option<[u8; 32]> {
    let matches = |item: &[u8]| -> Option<[u8; 32]> {
        let p: [u8; 32] = item.try_into().ok()?;
        (sha256::Hash::hash(&p).to_byte_array() == *payment_hash).then_some(p)
    };
    for input in &tx.inputs {
        for (key, value) in &input.unknown {
            if key.type_value != CONDITION_KEY_TYPE || key.key != VTXO_CONDITION_KEY {
                continue;
            }
            let mut cursor = value.as_slice();
            let Ok(count) = VarInt::consensus_decode(&mut cursor) else {
                continue;
            };
            for _ in 0..count.0 {
                let Ok(len) = VarInt::consensus_decode(&mut cursor) else {
                    break;
                };
                let len = len.0 as usize;
                if cursor.len() < len {
                    break;
                }
                if let Some(p) = matches(&cursor[..len]) {
                    return Some(p);
                }
                cursor = &cursor[len..];
            }
        }
        if let Some(witness) = &input.final_script_witness {
            if let Some(p) = witness.iter().find_map(matches) {
                return Some(p);
            }
        }
    }
    None
}

/// The live outputs of a lockup: unspent and still spendable offchain.
pub fn live(vtxos: &[VirtualTxOutPoint], dust: Amount) -> Vec<VirtualTxOutPoint> {
    vtxos
        .iter()
        .filter(|v| !v.is_spent && !v.is_unrolled && !v.is_recoverable(dust))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_witness_round_trips() {
        let preimage = [7u8; 32];
        let hash = sha256::Hash::hash(&preimage).to_byte_array();
        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![bitcoin::TxIn::default()],
            output: vec![],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        assert_eq!(preimage_in(&psbt, &hash), None);
        attach_condition_witness(&mut psbt.inputs[0], &preimage);
        assert_eq!(preimage_in(&psbt, &hash), Some(preimage));
        assert_eq!(preimage_in(&psbt, &[0; 32]), None);
    }
}
