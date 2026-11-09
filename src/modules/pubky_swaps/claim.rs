//! What `pubky_swaps_advance` does next for a reverse swap, decided from the HTLC's script history
//! (mempool included) and the chain tip, so the claim path can be checked without a chain.

use bitcoin::{OutPoint, ScriptBuf, Transaction};
use swap_common::onchain::extract_preimage;
use swap_common::timelock::claim_start_is_safe;

use super::types::PubkySwapRouteState;

/// The contract as the wallet rebuilt it from its own keys.
pub(crate) struct Contract<'a> {
    pub htlc_spk: &'a ScriptBuf,
    pub lock_sat: u64,
    /// The hash the claim's witness reveals the preimage of. Only the claim path carries it, so it
    /// tells our claim from the provider's refund whatever either pays.
    pub payment_hash: &'a [u8; 32],
    pub timeout: u32,
    pub required_confirmations: u32,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Step {
    /// Report this state; nothing to do.
    Report(PubkySwapRouteState),
    /// Our claim spends the lockup; record `txid` if the record names another, and report.
    Claimed {
        txid: String,
        state: PubkySwapRouteState,
    },
    /// The saved claim is not in the history: broadcast it again, or a new claim of this lockup
    /// when the record holds none (a restored swap whose claim the provider listed). Once a claim
    /// was published its preimage is public, so it goes out whatever the window; before that, only
    /// inside it.
    Rebroadcast {
        outpoint: OutPoint,
        confirmations: u32,
    },
    /// Build, save and broadcast the claim of this lockup.
    Claim {
        outpoint: OutPoint,
        confirmations: u32,
    },
}

/// A claim the record holds, and whether its preimage ever left the wallet.
pub(crate) struct SavedClaim {
    pub published: bool,
}

/// `listed_claim`: the provider listed a claim of this swap that the history does not show, so it
/// cannot be proven and its preimage may or may not be public.
pub(crate) fn next_step(
    contract: &Contract,
    saved_claim: Option<SavedClaim>,
    listed_claim: bool,
    tip: u32,
    history: &[(i32, Transaction)],
) -> Step {
    let confirmations_at = |height: i32| {
        if height > 0 {
            tip.saturating_sub(height as u32) + 1
        } else {
            0
        }
    };
    let lockup = history.iter().find_map(|(height, tx)| {
        let vout = tx.output.iter().position(|out| {
            out.script_pubkey == *contract.htlc_spk && out.value.to_sat() == contract.lock_sat
        })?;
        Some((
            OutPoint::new(tx.compute_txid(), vout as u32),
            confirmations_at(*height),
        ))
    });
    let Some((outpoint, confirmations)) = lockup else {
        return Step::Report(if tip >= contract.timeout {
            PubkySwapRouteState::Failed {
                reason: "The provider never locked funds before the timeout. Its hold invoice is \
                         cancelled and the payment returns."
                    .into(),
            }
        } else {
            PubkySwapRouteState::AwaitingLockup {
                confirmations: 0,
                required: contract.required_confirmations,
            }
        });
    };

    let spend = history.iter().find(|(_, tx)| {
        tx.input
            .iter()
            .any(|input| input.previous_output == outpoint)
    });
    if let Some((height, tx)) = spend {
        // A claim reveals the preimage in its witness and a refund cannot, so this holds for a
        // claim sent from another device too, whatever it pays.
        if extract_preimage(tx, &outpoint, contract.payment_hash).is_none() {
            return Step::Report(PubkySwapRouteState::Failed {
                reason: "The provider refunded its lockup instead of the claim.".into(),
            });
        }
        let txid = tx.compute_txid().to_string();
        let state = if confirmations_at(*height) > 0 {
            PubkySwapRouteState::Claimed { txid: txid.clone() }
        } else {
            PubkySwapRouteState::ClaimBroadcast { txid: txid.clone() }
        };
        return Step::Claimed { txid, state };
    }

    let safe_to_reveal = claim_start_is_safe(tip, contract.timeout);
    if let Some(claim) = saved_claim {
        if claim.published || safe_to_reveal {
            return Step::Rebroadcast {
                outpoint,
                confirmations,
            };
        }
    }
    // An unproven listed claim gets no early reveal: a provider could list any txid, read the
    // preimage from a claim of an unconfirmed lockup and replace the lockup. Once the lockup has
    // its confirmations, the claim goes out whatever the window, as its preimage may be public.
    if listed_claim {
        if confirmations < contract.required_confirmations {
            return Step::Report(PubkySwapRouteState::AwaitingLockup {
                confirmations,
                required: contract.required_confirmations,
            });
        }
        return Step::Rebroadcast {
            outpoint,
            confirmations,
        };
    }
    if !safe_to_reveal {
        return Step::Report(PubkySwapRouteState::Failed {
            reason: format!(
                "The lockup times out at block {}, too close to claim safely at block {tip}. \
                 The provider's refund returns the payment.",
                contract.timeout
            ),
        });
    }
    if confirmations < contract.required_confirmations {
        return Step::Report(PubkySwapRouteState::AwaitingLockup {
            confirmations,
            required: contract.required_confirmations,
        });
    }
    Step::Claim {
        outpoint,
        confirmations,
    }
}

#[cfg(test)]
mod tests {
    use bitcoin::absolute::LockTime;
    use bitcoin::hashes::{sha256, Hash};
    use bitcoin::transaction::Version;
    use bitcoin::{Amount, Sequence, TxIn, TxOut, Witness};

    use super::*;

    const LOCK: u64 = 20_200;
    const TIMEOUT: u32 = 1_144;
    const PREIMAGE: [u8; 32] = [5; 32];

    fn spk(byte: u8) -> ScriptBuf {
        ScriptBuf::from_bytes(vec![0x00, 0x14].into_iter().chain([byte; 20]).collect())
    }

    fn tx(inputs: Vec<OutPoint>, witness: Witness, outputs: Vec<(ScriptBuf, u64)>) -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: inputs
                .into_iter()
                .map(|previous_output| TxIn {
                    previous_output,
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                    witness: witness.clone(),
                })
                .collect(),
            output: outputs
                .into_iter()
                .map(|(script_pubkey, sats)| TxOut {
                    value: Amount::from_sat(sats),
                    script_pubkey,
                })
                .collect(),
        }
    }

    /// A claim's witness: signature, preimage, script. A refund's: signature, empty, script.
    fn witness(second: &[u8]) -> Witness {
        Witness::from_slice(&[vec![0x30; 71], second.to_vec(), vec![0x63; 90]])
    }

    struct Fixture {
        htlc: ScriptBuf,
        payment_hash: [u8; 32],
        lockup: Transaction,
    }

    impl Fixture {
        fn new() -> Self {
            let htlc = spk(1);
            let lockup = tx(
                vec![OutPoint::null()],
                Witness::new(),
                vec![(spk(9), 5_000), (htlc.clone(), LOCK)],
            );
            Self {
                htlc,
                payment_hash: sha256::Hash::hash(&PREIMAGE).to_byte_array(),
                lockup,
            }
        }

        fn contract(&self) -> Contract<'_> {
            Contract {
                htlc_spk: &self.htlc,
                lock_sat: LOCK,
                payment_hash: &self.payment_hash,
                timeout: TIMEOUT,
                required_confirmations: 1,
            }
        }

        fn outpoint(&self) -> OutPoint {
            OutPoint::new(self.lockup.compute_txid(), 1)
        }

        /// A claim to the contact.
        fn claim(&self) -> Transaction {
            self.claim_to(spk(2))
        }

        fn claim_to(&self, destination: ScriptBuf) -> Transaction {
            tx(
                vec![self.outpoint()],
                witness(&PREIMAGE),
                vec![(destination, 20_000)],
            )
        }

        fn refund_to(&self, outputs: Vec<(ScriptBuf, u64)>) -> Transaction {
            tx(vec![self.outpoint()], witness(&[]), outputs)
        }

        fn refund(&self) -> Transaction {
            self.refund_to(vec![(spk(3), 20_000)])
        }

        /// A saved claim that already reached the server.
        fn step(&self, saved: Option<&str>, tip: u32, history: &[(i32, Transaction)]) -> Step {
            let saved = saved.map(|_| SavedClaim { published: true });
            next_step(&self.contract(), saved, false, tip, history)
        }

        fn step_unpublished(&self, tip: u32, history: &[(i32, Transaction)]) -> Step {
            let saved = SavedClaim { published: false };
            next_step(&self.contract(), Some(saved), false, tip, history)
        }

        /// A restored swap whose claim the provider listed, unproven.
        fn step_listed(&self, tip: u32, history: &[(i32, Transaction)]) -> Step {
            next_step(&self.contract(), None, true, tip, history)
        }

        fn rebroadcast(&self, confirmations: u32) -> Step {
            Step::Rebroadcast {
                outpoint: self.outpoint(),
                confirmations,
            }
        }

        fn claim_step(&self, confirmations: u32) -> Step {
            Step::Claim {
                outpoint: self.outpoint(),
                confirmations,
            }
        }
    }

    fn is_failed(step: Step) -> bool {
        matches!(step, Step::Report(PubkySwapRouteState::Failed { .. }))
    }

    #[test]
    fn waits_for_a_lockup_until_the_timeout_then_fails() {
        let f = Fixture::new();
        assert_eq!(
            f.step(None, TIMEOUT - 1, &[]),
            Step::Report(PubkySwapRouteState::AwaitingLockup {
                confirmations: 0,
                required: 1
            })
        );
        assert!(is_failed(f.step(None, TIMEOUT, &[])));
    }

    #[test]
    fn waits_for_the_lockup_confirmations() {
        let f = Fixture::new();
        assert_eq!(
            f.step(None, 1_000, &[(0, f.lockup.clone())]),
            Step::Report(PubkySwapRouteState::AwaitingLockup {
                confirmations: 0,
                required: 1
            })
        );
        assert_eq!(
            f.step(None, 1_000, &[(1_000, f.lockup.clone())]),
            f.claim_step(1)
        );
    }

    #[test]
    fn leaves_a_lockup_too_close_to_the_timeout_to_the_refund() {
        let f = Fixture::new();
        let history = [(1_000, f.lockup.clone())];
        assert_eq!(f.step(None, TIMEOUT - 7, &history), f.claim_step(138));
        assert!(is_failed(f.step(None, TIMEOUT - 6, &history)));
    }

    #[test]
    fn reports_the_claim_as_broadcast_until_it_confirms() {
        let f = Fixture::new();
        let claim = f.claim();
        let txid = claim.compute_txid().to_string();
        let lockup = (1_000, f.lockup.clone());
        assert_eq!(
            f.step(Some(&txid), 1_001, &[lockup.clone(), (0, claim.clone())]),
            Step::Claimed {
                txid: txid.clone(),
                state: PubkySwapRouteState::ClaimBroadcast { txid: txid.clone() }
            }
        );
        assert_eq!(
            f.step(Some(&txid), 1_002, &[lockup, (1_002, claim)]),
            Step::Claimed {
                txid: txid.clone(),
                state: PubkySwapRouteState::Claimed { txid }
            }
        );
    }

    #[test]
    fn fails_when_the_provider_refunds_even_after_the_claim_went_out() {
        let f = Fixture::new();
        let history = [(1_000, f.lockup.clone()), (1_200, f.refund())];
        let saved = f.claim().compute_txid().to_string();
        for saved in [None, Some(saved.as_str())] {
            assert!(is_failed(f.step(saved, 1_200, &history)));
        }
    }

    #[test]
    fn broadcasts_a_saved_claim_again_once_it_left_the_mempool() {
        let f = Fixture::new();
        let saved = f.claim().compute_txid().to_string();
        // Also past the claim window: the preimage is public, so the claim must go out.
        assert_eq!(
            f.step(Some(&saved), TIMEOUT - 1, &[(1_000, f.lockup.clone())]),
            f.rebroadcast(144)
        );
    }

    #[test]
    fn claims_a_listed_claim_again_only_once_the_lockup_confirmed() {
        let f = Fixture::new();
        // Unconfirmed: a provider could have listed any txid to get the preimage revealed early.
        assert_eq!(
            f.step_listed(1_000, &[(0, f.lockup.clone())]),
            Step::Report(PubkySwapRouteState::AwaitingLockup {
                confirmations: 0,
                required: 1
            })
        );
        // Confirmed: the listed claim may have made the preimage public, so claim past the window.
        assert_eq!(
            f.step_listed(TIMEOUT + 5, &[(1_000, f.lockup.clone())]),
            f.rebroadcast(150)
        );
    }

    #[test]
    fn records_a_claim_from_another_device_whatever_it_pays() {
        let f = Fixture::new();
        // The original device claimed to the contact, whose address a restored record lacks.
        let claim = f.claim_to(spk(7));
        let txid = claim.compute_txid().to_string();
        for listed in [false, true] {
            assert_eq!(
                next_step(
                    &f.contract(),
                    None,
                    listed,
                    1_001,
                    &[(1_000, f.lockup.clone()), (0, claim.clone())]
                ),
                Step::Claimed {
                    txid: txid.clone(),
                    state: PubkySwapRouteState::ClaimBroadcast { txid: txid.clone() }
                }
            );
        }
    }

    #[test]
    fn keeps_an_unpublished_claim_private_once_the_window_closed() {
        let f = Fixture::new();
        let history = [(1_000, f.lockup.clone())];
        assert_eq!(
            f.step_unpublished(TIMEOUT - 7, &history),
            f.rebroadcast(138)
        );
        assert!(is_failed(f.step_unpublished(TIMEOUT + 5, &history)));
    }

    #[test]
    fn reports_a_refund_that_pays_the_contact_as_failed() {
        let f = Fixture::new();
        let history = |refund| [(1_000, f.lockup.clone()), (TIMEOUT as i32 + 1, refund)];
        // Dust to the contact, or exactly what the claim would have paid: neither is our claim.
        let dust = f.refund_to(vec![(spk(3), 19_000), (spk(2), 546)]);
        let exact = f.refund_to(vec![(spk(2), 20_000)]);
        let saved = exact.compute_txid().to_string();
        assert!(is_failed(f.step(None, TIMEOUT + 5, &history(dust))));
        assert!(is_failed(f.step(
            Some(&saved),
            TIMEOUT + 5,
            &history(exact)
        )));
    }

    #[test]
    fn fails_an_unconfirmed_lockup_past_the_timeout() {
        let f = Fixture::new();
        assert!(is_failed(f.step(None, 2_000, &[(0, f.lockup.clone())])));
    }
}
