//! What `pubky_swaps_advance` does next for a reverse swap, decided from the HTLC's script history
//! (mempool included) and the chain tip, so the claim path can be checked without a chain.

use bitcoin::{OutPoint, ScriptBuf, Transaction};
use swap_common::timelock::claim_start_is_safe;

use super::types::PubkySwapRouteState;

/// The contract as the wallet rebuilt it from its own keys.
pub(crate) struct Contract<'a> {
    pub htlc_spk: &'a ScriptBuf,
    pub lock_sat: u64,
    pub destination: &'a ScriptBuf,
    /// What the claim pays the contact: the lock less the claim fee.
    pub delivered_sat: u64,
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
    /// The saved claim is not in the history: broadcast it again. Once it was published its
    /// preimage is public, so it goes out whatever the window; before that, only inside it.
    Rebroadcast,
    /// Build, save and broadcast the claim of this lockup.
    Claim(OutPoint),
}

/// The claim a record holds, and whether its preimage ever left the wallet.
pub(crate) struct SavedClaim<'a> {
    pub txid: &'a str,
    pub published: bool,
}

pub(crate) fn next_step(
    contract: &Contract,
    saved_claim: Option<SavedClaim>,
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
        // Ours only if it is the saved claim, or pays the contact exactly what the claim would: a
        // refund with a dust output to the contact is not a payment.
        let txid = tx.compute_txid().to_string();
        let is_saved = saved_claim.as_ref().is_some_and(|claim| claim.txid == txid);
        let pays_contact = tx.output.iter().any(|out| {
            out.script_pubkey == *contract.destination
                && out.value.to_sat() == contract.delivered_sat
        });
        if !is_saved && !pays_contact {
            return Step::Report(PubkySwapRouteState::Failed {
                reason: "The provider refunded its lockup instead of the claim.".into(),
            });
        }
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
            return Step::Rebroadcast;
        }
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
    Step::Claim(outpoint)
}

#[cfg(test)]
mod tests {
    use bitcoin::absolute::LockTime;
    use bitcoin::transaction::Version;
    use bitcoin::{Amount, Sequence, TxIn, TxOut, Witness};

    use super::*;

    const LOCK: u64 = 20_200;
    const TIMEOUT: u32 = 1_144;

    fn spk(byte: u8) -> ScriptBuf {
        ScriptBuf::from_bytes(vec![0x00, 0x14].into_iter().chain([byte; 20]).collect())
    }

    fn tx(inputs: Vec<OutPoint>, outputs: Vec<(ScriptBuf, u64)>) -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: inputs
                .into_iter()
                .map(|previous_output| TxIn {
                    previous_output,
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                    witness: Witness::new(),
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

    struct Fixture {
        htlc: ScriptBuf,
        destination: ScriptBuf,
        lockup: Transaction,
    }

    impl Fixture {
        fn new() -> Self {
            let htlc = spk(1);
            let lockup = tx(
                vec![OutPoint::null()],
                vec![(spk(9), 5_000), (htlc.clone(), LOCK)],
            );
            Self {
                htlc,
                destination: spk(2),
                lockup,
            }
        }

        fn contract(&self) -> Contract<'_> {
            Contract {
                htlc_spk: &self.htlc,
                lock_sat: LOCK,
                destination: &self.destination,
                delivered_sat: 20_000,
                timeout: TIMEOUT,
                required_confirmations: 1,
            }
        }

        fn outpoint(&self) -> OutPoint {
            OutPoint::new(self.lockup.compute_txid(), 1)
        }

        fn claim(&self) -> Transaction {
            tx(
                vec![self.outpoint()],
                vec![(self.destination.clone(), 20_000)],
            )
        }

        fn refund(&self) -> Transaction {
            tx(vec![self.outpoint()], vec![(spk(3), 20_000)])
        }

        /// A saved claim that already reached the server.
        fn step(&self, saved: Option<&str>, tip: u32, history: &[(i32, Transaction)]) -> Step {
            let saved = saved.map(|txid| SavedClaim {
                txid,
                published: true,
            });
            next_step(&self.contract(), saved, tip, history)
        }

        fn step_unpublished(&self, saved: &str, tip: u32, history: &[(i32, Transaction)]) -> Step {
            let saved = SavedClaim {
                txid: saved,
                published: false,
            };
            next_step(&self.contract(), Some(saved), tip, history)
        }
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
        assert!(matches!(
            f.step(None, TIMEOUT, &[]),
            Step::Report(PubkySwapRouteState::Failed { .. })
        ));
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
            Step::Claim(f.outpoint())
        );
    }

    #[test]
    fn leaves_a_lockup_too_close_to_the_timeout_to_the_refund() {
        let f = Fixture::new();
        let history = [(1_000, f.lockup.clone())];
        assert_eq!(
            f.step(None, TIMEOUT - 7, &history),
            Step::Claim(f.outpoint())
        );
        assert!(matches!(
            f.step(None, TIMEOUT - 6, &history),
            Step::Report(PubkySwapRouteState::Failed { .. })
        ));
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
            assert!(matches!(
                f.step(saved, 1_200, &history),
                Step::Report(PubkySwapRouteState::Failed { .. })
            ));
        }
    }

    #[test]
    fn broadcasts_a_saved_claim_again_once_it_left_the_mempool() {
        let f = Fixture::new();
        let saved = f.claim().compute_txid().to_string();
        // Also past the claim window: the preimage is public, so the claim must go out.
        assert_eq!(
            f.step(Some(&saved), TIMEOUT - 1, &[(1_000, f.lockup.clone())]),
            Step::Rebroadcast
        );
    }

    #[test]
    fn records_a_claim_found_on_chain_that_the_record_missed() {
        let f = Fixture::new();
        let claim = f.claim();
        let txid = claim.compute_txid().to_string();
        assert_eq!(
            f.step(None, 1_001, &[(1_000, f.lockup.clone()), (0, claim)]),
            Step::Claimed {
                txid: txid.clone(),
                state: PubkySwapRouteState::ClaimBroadcast { txid }
            }
        );
    }

    #[test]
    fn keeps_an_unpublished_claim_private_once_the_window_closed() {
        let f = Fixture::new();
        let saved = f.claim().compute_txid().to_string();
        let history = [(1_000, f.lockup.clone())];
        assert_eq!(
            f.step_unpublished(&saved, TIMEOUT - 7, &history),
            Step::Rebroadcast
        );
        assert!(matches!(
            f.step_unpublished(&saved, TIMEOUT + 5, &history),
            Step::Report(PubkySwapRouteState::Failed { .. })
        ));
    }

    #[test]
    fn reports_a_refund_with_dust_to_the_contact_as_failed() {
        let f = Fixture::new();
        let refund = tx(
            vec![f.outpoint()],
            vec![(spk(3), 19_000), (f.destination.clone(), 546)],
        );
        assert!(matches!(
            f.step(
                None,
                TIMEOUT + 5,
                &[(1_000, f.lockup.clone()), (TIMEOUT as i32 + 1, refund)]
            ),
            Step::Report(PubkySwapRouteState::Failed { .. })
        ));
    }

    #[test]
    fn fails_an_unconfirmed_lockup_past_the_timeout() {
        let f = Fixture::new();
        assert!(matches!(
            f.step(None, 2_000, &[(0, f.lockup.clone())]),
            Step::Report(PubkySwapRouteState::Failed { .. })
        ));
    }
}
