//! Pubky Swaps as Paykit routes: when no rail the wallet holds reaches an endpoint a Paykit
//! contact published, route the payment through a Pubky Swaps provider (`swap/1`).
//! See README.md.

mod cap;
mod claim;
mod client;
mod delegation;
mod errors;
mod homeserver;
mod keys;
mod router;
mod store;
mod transport;
mod types;

pub use client::{advance_swap, commit_route, provider_edges, restore_swaps, route_paykit_payment};
pub use errors::PubkySwapError;
pub(crate) use store::init;
pub use types::*;
