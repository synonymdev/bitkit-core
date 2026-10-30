//! Pubky Swaps as Paykit routes: when no rail the wallet holds reaches an endpoint a Paykit
//! contact published, route the payment through a Pubky Swaps provider (`swap/1`).
//! See README.md.

mod client;
mod errors;
mod keys;
mod router;
mod store;
mod transport;
mod types;

pub use client::{advance_swap, commit_route, provider_edges, route_paykit_payment};
pub use errors::PubkySwapError;
pub(crate) use store::init;
pub use types::*;
