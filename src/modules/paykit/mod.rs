mod implementation;
mod types;
mod errors;
mod session_helper;
mod session_serialization;
mod deeplink_handler;

pub use implementation::{
    PubkyAuthenticatedTransport, PubkyUnauthenticatedTransport,
    set_payment_endpoint, remove_payment_endpoint,
    get_payment_list, get_payment_endpoint, get_known_contacts,
};
pub use types::{MethodId, EndpointData, PublicKey, SupportedPayments};
pub use errors::PaykitError;
// Session helper exports - PKARR functions removed as Bitkit only receives tokens via deeplinks
// pub use session_helper::{SessionConfig, extract_public_key_from_pkarr, create_session_from_pkarr};
pub use session_serialization::{
    SessionToken,
    deserialize_token_to_session,
    create_transport_from_session_token,
};

// Re-export for testing - these are needed by scanner tests
#[cfg(test)]
pub use session_serialization::create_session_token_from_keypair;
pub use deeplink_handler::{
    PaykitDeeplink, parse_paykit_deeplink, handle_paykit_deeplink,
    create_pubky_ring_session_request,
    // Not needed for Bitkit (only receives deeplinks, doesn't create them):
    // create_deeplink_from_token, validate_paykit_deeplink,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod integration_tests;
