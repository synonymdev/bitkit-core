use paykit_lib::{
    PubkyAuthenticatedTransport as ExternalPubkyAuthenticatedTransport,
    PubkyUnauthenticatedTransport as ExternalPubkyUnauthenticatedTransport,
};
use std::convert::TryInto;

use crate::paykit::errors::PaykitError;
use crate::paykit::types::{EndpointData, MethodId, PublicKey, SupportedPayments};

/// Authenticated transport wrapper for Paykit write operations.
#[derive(Clone, uniffi::Object)]
pub struct PubkyAuthenticatedTransport {
    pub(crate) inner: ExternalPubkyAuthenticatedTransport,
}

#[uniffi::export]
impl PubkyAuthenticatedTransport {
    /// Creates a new authenticated transport.
    /// Note: This requires proper session initialization which should be handled
    /// by the application layer. For now, this returns an error indicating the
    /// need for external session management.
    #[uniffi::constructor]
    pub fn new() -> Result<Self, PaykitError> {
        Err(PaykitError::SessionError(
            "Direct session creation not supported. Initialize session externally through Pubky SDK".to_string(),
        ))
    }


    /// Stores or updates a payment endpoint.
    ///
    /// # Parameters
    /// - `method`: Payment method identifier (e.g., "lightning", "onchain")
    /// - `data`: Endpoint data payload (UTF-8 JSON or other text format)
    pub async fn set_payment_endpoint(
        &self,
        method: MethodId,
        data: EndpointData,
    ) -> Result<(), PaykitError> {
        set_payment_endpoint(self, method, data).await
    }

    /// Removes a payment endpoint.
    ///
    /// # Parameters
    /// - `method`: Payment method identifier to remove
    ///
    /// # Returns
    /// - `Ok(())` on successful removal
    /// - `Err` if the endpoint doesn't exist or transport fails
    pub async fn remove_payment_endpoint(&self, method: MethodId) -> Result<(), PaykitError> {
        remove_payment_endpoint(self, method).await
    }
}

impl From<ExternalPubkyAuthenticatedTransport> for PubkyAuthenticatedTransport {
    fn from(inner: ExternalPubkyAuthenticatedTransport) -> Self {
        Self { inner }
    }
}

/// Unauthenticated transport wrapper for Paykit read operations.
#[derive(Clone, uniffi::Object)]
pub struct PubkyUnauthenticatedTransport {
    pub(crate) inner: ExternalPubkyUnauthenticatedTransport,
}

#[uniffi::export]
impl PubkyUnauthenticatedTransport {
    /// Creates a new unauthenticated transport for reading public payment data.
    #[uniffi::constructor]
    pub fn new() -> Result<Self, PaykitError> {
        let inner = ExternalPubkyUnauthenticatedTransport::try_new()
            .map_err(|e| PaykitError::from(e))?;
        Ok(Self { inner })
    }

    /// Retrieves all supported payment methods for a given payee.
    ///
    /// # Parameters
    /// - `payee`: Public key of the payee to query
    ///
    /// # Returns
    /// - `Ok(SupportedPayments)` with map of method IDs to endpoint data
    /// - Returns empty map if no endpoints are published
    /// - `Err` only on transport failures
    pub async fn get_payment_list(&self, payee: &PublicKey) -> Result<SupportedPayments, PaykitError> {
        get_payment_list(self, payee).await
    }

    /// Retrieves a specific payment endpoint for a payee and method.
    ///
    /// # Parameters
    /// - `payee`: Public key of the payee
    /// - `method`: Payment method identifier to query
    ///
    /// # Returns
    /// - `Ok(Some(EndpointData))` if the endpoint exists
    /// - `Ok(None)` if the endpoint is not published
    /// - `Err` only on transport failures
    pub async fn get_payment_endpoint(
        &self,
        payee: &PublicKey,
        method: &MethodId,
    ) -> Result<Option<EndpointData>, PaykitError> {
        get_payment_endpoint(self, payee, method).await
    }

    /// Returns known contacts (follows) of a given public key.
    ///
    /// # Parameters
    /// - `key`: Public key to query for contacts
    ///
    /// # Returns
    /// - `Ok(Vec<PublicKey>)` with list of known contacts
    /// - Returns empty vector if no contacts are stored
    /// - `Err` only on transport failures
    pub async fn get_known_contacts(&self, key: &PublicKey) -> Result<Vec<PublicKey>, PaykitError> {
        get_known_contacts(self, key).await
    }
}

impl From<ExternalPubkyUnauthenticatedTransport> for PubkyUnauthenticatedTransport {
    fn from(inner: ExternalPubkyUnauthenticatedTransport) -> Self {
        Self { inner }
    }
}

/// Stores or updates a payment endpoint via the authenticated transport.
///
/// # Parameters
/// - `client`: Authenticated transport client
/// - `method`: Payment method identifier (e.g., "lightning", "onchain")
/// - `data`: Endpoint data payload (UTF-8 JSON or other text format)
///
/// # Example
/// ```
/// let method = MethodId { id: "lightning".to_string() };
/// let data = EndpointData { data: r#"{"bolt11":"lnbc..."}"#.to_string() };
/// set_payment_endpoint(&client, method, data).await?;
/// ```
#[uniffi::export]
pub async fn set_payment_endpoint(
    client: &PubkyAuthenticatedTransport,
    method: MethodId,
    data: EndpointData,
) -> Result<(), PaykitError> {
    paykit_lib::set_payment_endpoint(&client.inner, method.into(), data.into())
        .await
        .map_err(|e| e.into())
}

/// Removes a payment endpoint via the authenticated transport.
///
/// # Parameters
/// - `client`: Authenticated transport client
/// - `method`: Payment method identifier to remove
///
/// # Returns
/// - `Ok(())` on successful removal
/// - `Err` if the endpoint doesn't exist or transport fails
#[uniffi::export]
pub async fn remove_payment_endpoint(
    client: &PubkyAuthenticatedTransport,
    method: MethodId,
) -> Result<(), PaykitError> {
    paykit_lib::remove_payment_endpoint(&client.inner, method.into())
        .await
        .map_err(|e| e.into())
}

/// Retrieves all supported payment methods for a given payee.
///
/// # Parameters
/// - `reader`: Unauthenticated transport for reading public data
/// - `payee`: Public key of the payee to query
///
/// # Returns
/// - `Ok(SupportedPayments)` with map of method IDs to endpoint data
/// - Returns empty map if no endpoints are published
/// - `Err` only on transport failures
///
/// # Example
/// ```
/// let reader = PubkyUnauthenticatedTransport::new()?;
/// let payee = PublicKey { key: "...".to_string() };
/// let payments = get_payment_list(&reader, &payee).await?;
/// for (method_id, data) in payments.entries {
///     println!("Method: {}, Data: {}", method_id, data.data);
/// }
/// ```
#[uniffi::export]
pub async fn get_payment_list(
    reader: &PubkyUnauthenticatedTransport,
    payee: &PublicKey,
) -> Result<SupportedPayments, PaykitError> {
    let external_key: paykit_lib::PublicKey = payee.clone()
        .try_into()
        .map_err(|e: String| PaykitError::InvalidPublicKey(e))?;
    paykit_lib::get_payment_list(&reader.inner, &external_key)
        .await
        .map(|p| p.into())
        .map_err(|e| e.into())
}

/// Retrieves a specific payment endpoint for a payee and method.
///
/// # Parameters
/// - `reader`: Unauthenticated transport for reading public data
/// - `payee`: Public key of the payee
/// - `method`: Payment method identifier to query
///
/// # Returns
/// - `Ok(Some(EndpointData))` if the endpoint exists
/// - `Ok(None)` if the endpoint is not published
/// - `Err` only on transport failures
///
/// # Example
/// ```
/// let reader = PubkyUnauthenticatedTransport::new()?;
/// let payee = PublicKey { key: "...".to_string() };
/// let method = MethodId { id: "lightning".to_string() };
/// if let Some(endpoint) = get_payment_endpoint(&reader, &payee, &method).await? {
///     println!("Lightning endpoint: {}", endpoint.data);
/// }
/// ```
#[uniffi::export]
pub async fn get_payment_endpoint(
    reader: &PubkyUnauthenticatedTransport,
    payee: &PublicKey,
    method: &MethodId,
) -> Result<Option<EndpointData>, PaykitError> {
    let external_key: paykit_lib::PublicKey = payee.clone()
        .try_into()
        .map_err(|e: String| PaykitError::InvalidPublicKey(e))?;
    let external_method: paykit_lib::MethodId = method.clone().into();
    paykit_lib::get_payment_endpoint(&reader.inner, &external_key, &external_method)
        .await
        .map(|opt| opt.map(|e| e.into()))
        .map_err(|e| e.into())
}

/// Returns known contacts (follows) of a given public key.
///
/// # Parameters
/// - `reader`: Unauthenticated transport for reading public data
/// - `key`: Public key to query for contacts
///
/// # Returns
/// - `Ok(Vec<PublicKey>)` with list of known contacts
/// - Returns empty vector if no contacts are stored
/// - `Err` only on transport failures
///
/// # Example
/// ```
/// let reader = PubkyUnauthenticatedTransport::new()?;
/// let user = PublicKey { key: "...".to_string() };
/// let contacts = get_known_contacts(&reader, &user).await?;
/// for contact in contacts {
///     println!("Contact: {}", contact.key);
/// }
/// ```
#[uniffi::export]
pub async fn get_known_contacts(
    reader: &PubkyUnauthenticatedTransport,
    key: &PublicKey,
) -> Result<Vec<PublicKey>, PaykitError> {
    let external_key: paykit_lib::PublicKey = key.clone()
        .try_into()
        .map_err(|e: String| PaykitError::InvalidPublicKey(e))?;
    paykit_lib::get_known_contacts(&reader.inner, &external_key)
        .await
        .map(|contacts| contacts.into_iter().map(|c| c.into()).collect())
        .map_err(|e| e.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unauthenticated_transport_creation() {
        // This test will fail in CI without a Pubky network, but demonstrates the API
        let result = PubkyUnauthenticatedTransport::new();
        // We expect this to either succeed or fail with a transport error, not unimplemented
        match result {
            Ok(_) => println!("Successfully created unauthenticated transport"),
            Err(PaykitError::Transport(..)) => {
                println!("Transport error (expected in test environment)")
            }
            Err(e) => panic!("Unexpected error type: {:?}", e),
        }
    }
}
