//! VHTLC v2 with the emulator covenant suite: the lockup both Lightning corridors compile.
//!
//! Ported from arkade-os/rust-sdk#287 (`feat/vhtlc-covenant-leaves`, commit `eb8548d`), which
//! mirrors `VHTLC.ScriptV2` with `nonInteractiveParameters` in `@arkade-os/sdk` 0.5.0-rc.31. Kept
//! here rather than taken as a git dependency so the crate links the same `ark-core =0.12.0` the
//! wallet binding does. Drop it for `ark_core::vhtlc::VhtlcScript::new_v2` once that ships.
//!
//! Leaf order fixes the merkle root: the six signature leaves, then the covenant suite
//! (`nonInteractiveClaim`, `nonInteractiveRefund`, and, unless legacy, the timelocked
//! `nonInteractiveRefundWithoutReceiver`). The tree is assembled with btcd's algorithm, as arkd
//! and the TS SDK do.

use ark_core::ArkAddress;
use bitcoin::hashes::ripemd160;
use bitcoin::hashes::Hash;
use bitcoin::key::Secp256k1;
use bitcoin::opcodes::all::*;
use bitcoin::opcodes::Opcode;
use bitcoin::taproot::ControlBlock;
use bitcoin::taproot::LeafVersion;
use bitcoin::taproot::NodeInfo;
use bitcoin::taproot::TaprootSpendInfo;
use bitcoin::Network;
use bitcoin::PublicKey;
use bitcoin::ScriptBuf;
use bitcoin::Sequence;
use bitcoin::XOnlyPublicKey;
use std::collections::VecDeque;
use std::str::FromStr;

#[derive(Debug, thiserror::Error)]
pub enum VhtlcError {
    #[error("invalid P2TR script for {0}: expected `OP_1 <32 bytes>`")]
    InvalidP2trScript(&'static str),
    #[error("invalid timelock: {0}")]
    InvalidTimelock(String),
    #[error("covenant key derivation failed: {0}")]
    Covenant(String),
    #[error("taproot construction failed: {0}")]
    Taproot(String),
}

/// The emulator covenant leaves, all or nothing (`VHTLC.Options.nonInteractiveParameters`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonInteractiveParameters {
    pub emulator_pubkey: XOnlyPublicKey,
    /// Where the claim covenant pays: the receiver's P2TR pkScript.
    pub receiver_pk_script: ScriptBuf,
    /// Where both refund covenants pay: the sender's P2TR pkScript.
    pub sender_pk_script: ScriptBuf,
    /// Rebuild a lockup funded before the timelocked refund leaf shipped (eight leaves).
    pub legacy_pre_timelocked_refund: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VhtlcOptions {
    pub sender: XOnlyPublicKey,
    pub receiver: XOnlyPublicKey,
    pub server: XOnlyPublicKey,
    pub preimage_hash: ripemd160::Hash,
    pub refund_locktime: u32,
    pub unilateral_claim_delay: Sequence,
    pub unilateral_refund_delay: Sequence,
    pub unilateral_refund_without_receiver_delay: Sequence,
}

/// A built nine-leaf (or eight-leaf legacy) VHTLC v2.
#[derive(Debug, Clone)]
pub struct VhtlcV2 {
    pub options: VhtlcOptions,
    pub covenant: NonInteractiveParameters,
    leaves: Leaves,
    spend_info: TaprootSpendInfo,
}

#[derive(Debug, Clone)]
struct Leaves {
    claim: ScriptBuf,
    refund: ScriptBuf,
    refund_without_receiver: ScriptBuf,
    unilateral_claim: ScriptBuf,
    unilateral_refund: ScriptBuf,
    unilateral_refund_without_receiver: ScriptBuf,
    non_interactive_claim: ScriptBuf,
    non_interactive_claim_arkade_script: ScriptBuf,
    non_interactive_refund: ScriptBuf,
    non_interactive_refund_arkade_script: ScriptBuf,
    non_interactive_refund_without_receiver: Option<ScriptBuf>,
}

fn is_p2tr(script: &ScriptBuf) -> bool {
    let b = script.as_bytes();
    b.len() == 34 && b[0] == 0x51 && b[1] == 0x20
}

fn check_delay(name: &str, delay: Sequence) -> Result<(), VhtlcError> {
    if !delay.is_relative_lock_time() || delay.to_consensus_u32() & 0xffff == 0 {
        return Err(VhtlcError::InvalidTimelock(format!(
            "{name} must be a non-zero CSV relative lock time"
        )));
    }
    Ok(())
}

/// `enforcePayTo`: the output at this input's index is P2TR, pays the destination program, and
/// carries at least the input's value. Opcodes 0xc9..0xd1 are Arkade introspection opcodes,
/// executed by the emulator.
fn enforce_pay_to(destination: &ScriptBuf) -> ScriptBuf {
    let program: &[u8; 32] = destination.as_bytes()[2..]
        .try_into()
        .expect("validated 34-byte P2TR script");
    ScriptBuf::builder()
        .push_opcode(Opcode::from(0xcd)) // PUSHCURRENTINPUTINDEX
        .push_opcode(OP_DUP)
        .push_opcode(Opcode::from(0xd1)) // INSPECTOUTPUTSCRIPTPUBKEY
        .push_int(1)
        .push_opcode(OP_EQUALVERIFY)
        .push_slice(program)
        .push_opcode(OP_EQUALVERIFY)
        .push_opcode(Opcode::from(0xcf)) // INSPECTOUTPUTVALUE
        .push_opcode(Opcode::from(0xcd)) // PUSHCURRENTINPUTINDEX
        .push_opcode(Opcode::from(0xc9)) // INSPECTINPUTVALUE
        .push_opcode(OP_GREATERTHANOREQUAL)
        .into_script()
}

impl VhtlcOptions {
    /// `OP_SIZE 32 OP_EQUALVERIFY OP_HASH160 <hash> OP_EQUAL` (the v2 preimage condition).
    fn condition(&self) -> bitcoin::script::Builder {
        ScriptBuf::builder()
            .push_opcode(OP_SIZE)
            .push_int(32)
            .push_opcode(OP_EQUALVERIFY)
            .push_opcode(OP_HASH160)
            .push_slice(self.preimage_hash.as_byte_array())
            .push_opcode(OP_EQUAL)
    }

    fn csv(builder: bitcoin::script::Builder, delay: Sequence) -> bitcoin::script::Builder {
        builder
            .push_int(delay.to_consensus_u32() as i64)
            .push_opcode(OP_CSV)
            .push_opcode(OP_DROP)
    }
}

impl VhtlcV2 {
    pub fn new(
        options: VhtlcOptions,
        covenant: NonInteractiveParameters,
    ) -> Result<Self, VhtlcError> {
        if options.refund_locktime == 0 {
            return Err(VhtlcError::InvalidTimelock(
                "refund locktime must be > 0".into(),
            ));
        }
        check_delay("unilateral claim delay", options.unilateral_claim_delay)?;
        check_delay("unilateral refund delay", options.unilateral_refund_delay)?;
        check_delay(
            "unilateral refund without receiver delay",
            options.unilateral_refund_without_receiver_delay,
        )?;
        if !is_p2tr(&covenant.receiver_pk_script) {
            return Err(VhtlcError::InvalidP2trScript("receiver_pk_script"));
        }
        if !is_p2tr(&covenant.sender_pk_script) {
            return Err(VhtlcError::InvalidP2trScript("sender_pk_script"));
        }
        let o = &options;

        let claim = o
            .condition()
            .push_opcode(OP_VERIFY)
            .push_x_only_key(&o.receiver)
            .push_opcode(OP_CHECKSIGVERIFY)
            .push_x_only_key(&o.server)
            .push_opcode(OP_CHECKSIG)
            .into_script();
        let refund = ScriptBuf::builder()
            .push_x_only_key(&o.sender)
            .push_opcode(OP_CHECKSIGVERIFY)
            .push_x_only_key(&o.receiver)
            .push_opcode(OP_CHECKSIGVERIFY)
            .push_x_only_key(&o.server)
            .push_opcode(OP_CHECKSIG)
            .into_script();
        let refund_without_receiver = ScriptBuf::builder()
            .push_int(o.refund_locktime as i64)
            .push_opcode(OP_CLTV)
            .push_opcode(OP_DROP)
            .push_x_only_key(&o.sender)
            .push_opcode(OP_CHECKSIGVERIFY)
            .push_x_only_key(&o.server)
            .push_opcode(OP_CHECKSIG)
            .into_script();
        let unilateral_claim = VhtlcOptions::csv(
            o.condition().push_opcode(OP_VERIFY),
            o.unilateral_claim_delay,
        )
        .push_x_only_key(&o.receiver)
        .push_opcode(OP_CHECKSIG)
        .into_script();
        let unilateral_refund = VhtlcOptions::csv(ScriptBuf::builder(), o.unilateral_refund_delay)
            .push_x_only_key(&o.sender)
            .push_opcode(OP_CHECKSIGVERIFY)
            .push_x_only_key(&o.receiver)
            .push_opcode(OP_CHECKSIG)
            .into_script();
        let unilateral_refund_without_receiver = VhtlcOptions::csv(
            ScriptBuf::builder(),
            o.unilateral_refund_without_receiver_delay,
        )
        .push_x_only_key(&o.sender)
        .push_opcode(OP_CHECKSIG)
        .into_script();

        let tweak = |script: &ScriptBuf| {
            ark_script::tweak::compute_arkade_script_public_key(&covenant.emulator_pubkey, script)
                .map_err(|e| VhtlcError::Covenant(format!("{e:?}")))
        };
        let nic_script = enforce_pay_to(&covenant.receiver_pk_script);
        let nic_cosigner = tweak(&nic_script)?;
        let non_interactive_claim = o
            .condition()
            .push_opcode(OP_VERIFY)
            .push_x_only_key(&o.server)
            .push_opcode(OP_CHECKSIGVERIFY)
            .push_x_only_key(&nic_cosigner)
            .push_opcode(OP_CHECKSIG)
            .into_script();
        let nir_script = enforce_pay_to(&covenant.sender_pk_script);
        let nir_cosigner = tweak(&nir_script)?;
        let non_interactive_refund = ScriptBuf::builder()
            .push_x_only_key(&o.server)
            .push_opcode(OP_CHECKSIGVERIFY)
            .push_x_only_key(&o.receiver)
            .push_opcode(OP_CHECKSIGVERIFY)
            .push_x_only_key(&nir_cosigner)
            .push_opcode(OP_CHECKSIG)
            .into_script();
        let non_interactive_refund_without_receiver = (!covenant.legacy_pre_timelocked_refund)
            .then(|| {
                ScriptBuf::builder()
                    .push_int(o.refund_locktime as i64)
                    .push_opcode(OP_CLTV)
                    .push_opcode(OP_DROP)
                    .push_x_only_key(&o.server)
                    .push_opcode(OP_CHECKSIGVERIFY)
                    .push_x_only_key(&nir_cosigner)
                    .push_opcode(OP_CHECKSIG)
                    .into_script()
            });

        let leaves = Leaves {
            claim,
            refund,
            refund_without_receiver,
            unilateral_claim,
            unilateral_refund,
            unilateral_refund_without_receiver,
            non_interactive_claim,
            non_interactive_claim_arkade_script: nic_script,
            non_interactive_refund,
            non_interactive_refund_arkade_script: nir_script,
            non_interactive_refund_without_receiver,
        };
        let spend_info = build_taproot(&leaves.ordered())?;
        Ok(Self {
            options,
            covenant,
            leaves,
            spend_info,
        })
    }

    /// Every leaf, in the order the merkle root depends on.
    pub fn tapscripts(&self) -> Vec<ScriptBuf> {
        self.leaves.ordered()
    }

    pub fn spend_info(&self) -> &TaprootSpendInfo {
        &self.spend_info
    }

    pub fn script_pubkey(&self) -> ScriptBuf {
        ScriptBuf::builder()
            .push_opcode(OP_PUSHNUM_1)
            .push_slice(self.spend_info.output_key().serialize())
            .into_script()
    }

    pub fn address(&self, network: Network) -> ArkAddress {
        ArkAddress::new(network, self.options.server, self.spend_info.output_key())
    }

    pub fn claim_script(&self) -> &ScriptBuf {
        &self.leaves.claim
    }

    pub fn refund_without_receiver_script(&self) -> &ScriptBuf {
        &self.leaves.refund_without_receiver
    }

    pub fn control_block(&self, leaf: &ScriptBuf) -> Option<ControlBlock> {
        self.spend_info
            .control_block(&(leaf.clone(), LeafVersion::TapScript))
    }

    /// Named leaves, for fixtures and debugging.
    pub fn named_leaves(&self) -> Vec<(&'static str, ScriptBuf)> {
        let l = &self.leaves;
        let mut out = vec![
            ("claim", l.claim.clone()),
            ("refund", l.refund.clone()),
            ("refundWithoutReceiver", l.refund_without_receiver.clone()),
            ("unilateralClaim", l.unilateral_claim.clone()),
            ("unilateralRefund", l.unilateral_refund.clone()),
            (
                "unilateralRefundWithoutReceiver",
                l.unilateral_refund_without_receiver.clone(),
            ),
            ("nonInteractiveClaim", l.non_interactive_claim.clone()),
            ("nonInteractiveRefund", l.non_interactive_refund.clone()),
        ];
        if let Some(s) = &l.non_interactive_refund_without_receiver {
            out.push(("nonInteractiveRefundWithoutReceiver", s.clone()));
        }
        out
    }

    pub fn arkade_scripts(&self) -> (ScriptBuf, ScriptBuf) {
        (
            self.leaves.non_interactive_claim_arkade_script.clone(),
            self.leaves.non_interactive_refund_arkade_script.clone(),
        )
    }
}

impl Leaves {
    fn ordered(&self) -> Vec<ScriptBuf> {
        let mut v = vec![
            self.claim.clone(),
            self.refund.clone(),
            self.refund_without_receiver.clone(),
            self.unilateral_claim.clone(),
            self.unilateral_refund.clone(),
            self.unilateral_refund_without_receiver.clone(),
            self.non_interactive_claim.clone(),
            self.non_interactive_refund.clone(),
        ];
        v.extend(self.non_interactive_refund_without_receiver.clone());
        v
    }
}

fn build_taproot(scripts: &[ScriptBuf]) -> Result<TaprootSpendInfo, VhtlcError> {
    let internal = XOnlyPublicKey::from(
        PublicKey::from_str(ark_core::UNSPENDABLE_KEY)
            .map_err(|e| VhtlcError::Taproot(e.to_string()))?,
    );
    let tree = assemble_taproot_tree(scripts)?;
    Ok(TaprootSpendInfo::from_node_info(
        &Secp256k1::new(),
        internal,
        tree,
    ))
}

/// btcd's `AssembleTaprootScriptTree`: pair leaves left to right (an odd trailing leaf joins the
/// last pair), then FIFO-merge branches until one root remains. Private in `ark-core` 0.12.0.
fn assemble_taproot_tree(scripts: &[ScriptBuf]) -> Result<NodeInfo, VhtlcError> {
    let leaf = |s: &ScriptBuf| NodeInfo::new_leaf_with_ver(s.clone(), LeafVersion::TapScript);
    let combine = |a: NodeInfo, b: NodeInfo| {
        NodeInfo::combine(a, b).map_err(|e| VhtlcError::Taproot(e.to_string()))
    };
    match scripts.len() {
        0 => return Err(VhtlcError::Taproot("no scripts".into())),
        1 => return Ok(leaf(&scripts[0])),
        _ => {}
    }
    let mut branches: Vec<NodeInfo> = Vec::new();
    let mut i = 0;
    while i < scripts.len() {
        if i == scripts.len() - 1 {
            let last = branches.pop().expect("a branch precedes a trailing leaf");
            branches.push(combine(last, leaf(&scripts[i]))?);
        } else {
            branches.push(combine(leaf(&scripts[i]), leaf(&scripts[i + 1]))?);
        }
        i += 2;
    }
    let mut queue: VecDeque<NodeInfo> = branches.into();
    while queue.len() >= 2 {
        let left = queue.pop_front().expect("two branches");
        let right = queue.pop_front().expect("two branches");
        queue.push_back(combine(left, right)?);
    }
    Ok(queue.pop_front().expect("one root"))
}

/// BIP68 seconds-typed relative lock time; `seconds` must be a multiple of 512.
pub fn seconds_sequence(seconds: u32) -> Result<Sequence, VhtlcError> {
    if seconds == 0 || !seconds.is_multiple_of(512) || seconds / 512 > 0xffff {
        return Err(VhtlcError::InvalidTimelock(format!(
            "{seconds}s is not a positive BIP68 multiple of 512"
        )));
    }
    Sequence::from_seconds_floor(seconds).map_err(|e| VhtlcError::InvalidTimelock(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hex::DisplayHex;
    use bitcoin::secp256k1::Keypair;
    use bitcoin::secp256k1::SecretKey;

    fn key(fill: u8) -> XOnlyPublicKey {
        let sk = SecretKey::from_slice(&[fill; 32]).unwrap();
        Keypair::from_secret_key(&Secp256k1::new(), &sk)
            .x_only_public_key()
            .0
    }

    fn p2tr(fill: u8) -> ScriptBuf {
        ScriptBuf::new_p2tr_tweaked(bitcoin::key::TweakedPublicKey::dangerous_assume_tweaked(
            key(fill),
        ))
    }

    /// The cross-SDK nine-leaf fixture from rust-sdk#287, generated by `VHTLC.ScriptV2` (blocks
    /// delays; the corridor uses seconds, which the oracle test covers).
    #[test]
    fn nine_leaf_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/vhtlc-v2-nine-leaf.json")).unwrap();
        let options = VhtlcOptions {
            sender: key(1),
            receiver: key(2),
            server: key(3),
            preimage_hash: ripemd160::Hash::from_byte_array([9; 20]),
            refund_locktime: 1000,
            unilateral_claim_delay: Sequence::from_height(100),
            unilateral_refund_delay: Sequence::from_height(102),
            unilateral_refund_without_receiver_delay: Sequence::from_height(103),
        };
        let v = VhtlcV2::new(
            options,
            NonInteractiveParameters {
                emulator_pubkey: key(5),
                receiver_pk_script: p2tr(4),
                sender_pk_script: p2tr(6),
                legacy_pre_timelocked_refund: false,
            },
        )
        .unwrap();
        for (name, script) in v.named_leaves() {
            assert_eq!(
                script.as_bytes().to_lower_hex_string(),
                fixture["leaves"][name].as_str().unwrap(),
                "leaf {name}"
            );
        }
        let (nic, nir) = v.arkade_scripts();
        assert_eq!(
            nic.as_bytes().to_lower_hex_string(),
            fixture["arkadeScripts"]["nonInteractiveClaim"]
                .as_str()
                .unwrap()
        );
        assert_eq!(
            nir.as_bytes().to_lower_hex_string(),
            fixture["arkadeScripts"]["nonInteractiveRefund"]
                .as_str()
                .unwrap()
        );
        assert_eq!(
            v.script_pubkey().as_bytes().to_lower_hex_string(),
            fixture["pkScript"].as_str().unwrap()
        );
        assert_eq!(v.tapscripts().len(), 9);
    }
}
