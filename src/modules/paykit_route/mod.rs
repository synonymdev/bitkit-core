//! Wallet-local route selection for paying a Paykit contact.

mod errors;
mod router;
mod types;

pub use errors::PaykitRouteError;
pub use router::route_payment;
pub(crate) use router::{network_label, onchain_endpoint};
pub use types::{PaykitEndpoint, PaykitRoute, PaykitRouteRequest, ServiceRouteLeg};
