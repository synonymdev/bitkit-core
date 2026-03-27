use crate::lnurl::{ChannelRequestParams, LnurlAuthParams, LnurlError, WithdrawCallbackParams};
use bitcoin::bip32::Xpriv;
use bitcoin::secp256k1::{Message, PublicKey, Secp256k1};
use lnurl::lightning_address::LightningAddress;
use lnurl::lnurl::LnUrl;
use lnurl::{get_derivation_path, AsyncClient, Builder, LnUrlResponse, Response};
use std::str::FromStr;
use url::Url;

pub async fn get_lnurl_invoice(address: &str, amount_satoshis: u64) -> Result<String, LnurlError> {
    let ln_addr = match parse_lightning_address(address) {
        Ok(addr) => addr,
        Err(e) => return Err(e),
    };
    let client = match create_async_client() {
        Ok(client) => client,
        Err(e) => return Err(e),
    };
    let pay_response = match fetch_lnurl_pay_response(&client, &ln_addr).await {
        Ok(response) => response,
        Err(e) => return Err(e),
    };
    generate_invoice(&client, &pay_response, amount_satoshis).await
}

fn parse_lightning_address(address: &str) -> Result<LightningAddress, LnurlError> {
    LightningAddress::from_str(address).map_err(|_| LnurlError::InvalidAddress)
}

fn create_async_client() -> Result<AsyncClient, LnurlError> {
    Builder::default()
        .build_async()
        .map_err(|_| LnurlError::ClientCreationFailed)
}

async fn fetch_lnurl_pay_response(
    client: &AsyncClient,
    ln_addr: &LightningAddress,
) -> Result<LnUrlResponse, LnurlError> {
    match client.make_request(&ln_addr.lnurlp_url()).await {
        Ok(response @ LnUrlResponse::LnUrlPayResponse(_)) => Ok(response),
        Ok(_) => Err(LnurlError::InvalidResponse),
        Err(_) => Err(LnurlError::RequestFailed),
    }
}

async fn generate_invoice(
    client: &AsyncClient,
    pay_response: &LnUrlResponse,
    amount_satoshis: u64,
) -> Result<String, LnurlError> {
    let pay = match pay_response {
        LnUrlResponse::LnUrlPayResponse(pay) => pay,
        _ => return Err(LnurlError::InvalidResponse),
    };

    let amount_msats = amount_satoshis * 1000;

    // Validate amount range
    if amount_msats < pay.min_sendable || amount_msats > pay.max_sendable {
        return Err(LnurlError::InvalidAmount {
            amount_satoshis,
            min: pay.min_sendable / 1000,
            max: pay.max_sendable / 1000,
        });
    }

    // Generate invoice
    client
        .get_invoice(pay, amount_msats, None, None)
        .await
        .map(|invoice| invoice.pr)
        .map_err(|e| LnurlError::InvoiceCreationFailed {
            error_details: e.to_string(),
        })
}

pub fn create_channel_request_url(params: ChannelRequestParams) -> Result<String, LnurlError> {
    let mut url = Url::parse(&params.callback).map_err(|_| LnurlError::InvalidAddress)?;

    // Collect all query parameters, excluding "k1"
    let existing_params: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| key != "k1")
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // Clear all query parameters
    url.set_query(None);

    {
        let mut query_pairs = url.query_pairs_mut();
        for (key, value) in existing_params {
            query_pairs.append_pair(&key, &value);
        }

        // Add the new k1 and other parameters
        query_pairs
            .append_pair("k1", &params.k1)
            .append_pair("remoteid", &params.local_node_id)
            .append_pair("private", if params.is_private { "1" } else { "0" })
            .append_pair("cancel", if params.cancel { "1" } else { "0" });
    }

    Ok(url.to_string())
}

pub fn create_withdraw_callback_url(params: WithdrawCallbackParams) -> Result<String, LnurlError> {
    let mut url = Url::parse(&params.callback).map_err(|_| LnurlError::InvalidAddress)?;

    // Collect all query parameters, excluding "k1" and "pr"
    let existing_params: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| key != "k1" && key != "pr")
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // Clear all query parameters
    url.set_query(None);

    {
        let mut query_pairs = url.query_pairs_mut();
        for (key, value) in existing_params {
            query_pairs.append_pair(&key, &value);
        }

        // Add the new k1 and pr parameters
        query_pairs
            .append_pair("k1", &params.k1)
            .append_pair("pr", &params.payment_request);
    }

    Ok(url.to_string())
}

pub async fn lnurl_auth(params: LnurlAuthParams) -> Result<String, LnurlError> {
    let domain_url = Url::parse(&format!("https://{}", params.domain))
        .map_err(|_| LnurlError::InvalidAddress)?;

    let derivation_path = get_derivation_path(params.hashing_key, &domain_url)
        .map_err(|_| LnurlError::AuthenticationFailed)?;

    let secp = Secp256k1::new();
    let master_key = Xpriv::new_master(bitcoin::Network::Bitcoin, &params.hashing_key)
        .map_err(|_| LnurlError::AuthenticationFailed)?;

    let derived_key = master_key
        .derive_priv(&secp, &derivation_path)
        .map_err(|_| LnurlError::AuthenticationFailed)?;

    let private_key = derived_key.private_key;
    let public_key = PublicKey::from_secret_key(&secp, &private_key);

    let k1_bytes = hex::decode(&params.k1).map_err(|_| LnurlError::AuthenticationFailed)?;
    let message =
        Message::from_digest_slice(&k1_bytes).map_err(|_| LnurlError::AuthenticationFailed)?;

    let signature = secp.sign_ecdsa(&message, &private_key);

    let lnurl = if params.callback.starts_with("lnurl1") {
        LnUrl::from_str(&params.callback).map_err(|_| LnurlError::InvalidAddress)?
    } else {
        LnUrl {
            url: params.callback,
        }
    };

    let client = create_async_client()?;

    let response = client
        .lnurl_auth(lnurl, signature, public_key)
        .await
        .map_err(|_| LnurlError::RequestFailed)?;

    match response {
        Response::Ok { .. } => Ok("Authentication successful".to_string()),
        Response::Error { reason: _ } => Err(LnurlError::AuthenticationFailed),
    }
}
