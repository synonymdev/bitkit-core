use crate::lnurl::{ChannelRequestParams, LnurlAuthParams, LnurlError, WithdrawCallbackParams};
use crate::modules::scanner::LnurlPayData;
use bitcoin::bip32::Xpriv;
use bitcoin::secp256k1::{Message, PublicKey, Secp256k1};
use lightning_invoice::Bolt11Invoice;
use lnurl::lightning_address::LightningAddress;
use lnurl::lnurl::LnUrl;
use lnurl::{get_derivation_path, AsyncClient, Builder, LnUrlResponse, Response};
use serde::Deserialize;
use std::str::FromStr;
use url::Url;

#[derive(Deserialize)]
struct LnurlPayCallbackResponse {
    pr: Option<String>,
}

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

pub async fn get_lnurl_invoice_for_pay_data(
    data: LnurlPayData,
    amount_msats: u64,
    comment: Option<String>,
) -> Result<String, LnurlError> {
    validate_amount_msats(amount_msats, data.min_sendable, data.max_sendable)?;

    let callback_url =
        build_lnurl_pay_callback_url(&data.callback, amount_msats, comment.as_deref())?;

    let response = reqwest::get(callback_url)
        .await
        .map_err(|_| LnurlError::RequestFailed)?
        .error_for_status()
        .map_err(|_| LnurlError::RequestFailed)?;

    let callback_response = response
        .json::<LnurlPayCallbackResponse>()
        .await
        .map_err(|_| LnurlError::InvalidResponse)?;
    let pr = callback_response.pr.ok_or(LnurlError::InvalidResponse)?;

    validate_lnurl_pay_invoice(&pr, amount_msats, &data.metadata_str)?;

    Ok(pr)
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

    validate_amount_msats(amount_msats, pay.min_sendable, pay.max_sendable)?;

    let invoice = client
        .get_invoice(pay, amount_msats, None, None)
        .await
        .map_err(|e| LnurlError::InvoiceCreationFailed {
            error_details: e.to_string(),
        })?;

    validate_lnurl_pay_invoice(&invoice.pr, amount_msats, &pay.metadata)?;

    Ok(invoice.pr)
}

fn validate_amount_msats(amount_msats: u64, min: u64, max: u64) -> Result<(), LnurlError> {
    if amount_msats < min || amount_msats > max {
        return Err(LnurlError::InvalidAmount {
            amount_satoshis: amount_msats.div_ceil(1000),
            min: min / 1000,
            max: max / 1000,
        });
    }

    Ok(())
}

pub(crate) fn build_lnurl_pay_callback_url(
    callback: &str,
    amount_msats: u64,
    comment: Option<&str>,
) -> Result<Url, LnurlError> {
    let mut url = Url::parse(callback).map_err(|_| LnurlError::InvalidAddress)?;

    {
        let mut query_pairs = url.query_pairs_mut();
        query_pairs.append_pair("amount", &amount_msats.to_string());
        if let Some(comment) = comment {
            if !comment.is_empty() {
                query_pairs.append_pair("comment", comment);
            }
        }
    }

    Ok(url)
}

pub(crate) fn validate_lnurl_pay_invoice(
    pr: &str,
    amount_msats: u64,
    _metadata: &str,
) -> Result<(), LnurlError> {
    let invoice = Bolt11Invoice::from_str(pr).map_err(|_| LnurlError::InvalidResponse)?;
    let invoice_msats = invoice
        .amount_milli_satoshis()
        .ok_or(LnurlError::AmountMismatch {
            requested_msats: amount_msats,
            invoice_msats: 0,
        })?;

    if invoice_msats != amount_msats {
        return Err(LnurlError::AmountMismatch {
            requested_msats: amount_msats,
            invoice_msats,
        });
    }

    Ok(())
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
