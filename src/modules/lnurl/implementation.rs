use std::str::FromStr;
use lnurl::{LnUrlResponse, AsyncClient, Builder};
use lnurl::lightning_address::LightningAddress;
use crate::lnurl::LnurlError;

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
    LightningAddress::from_str(address)
        .map_err(|_| LnurlError::InvalidAddress)
}

fn create_async_client() -> Result<AsyncClient, LnurlError> {
    Builder::default().build_async()
        .map_err(|_| LnurlError::ClientCreationFailed)
}

async fn fetch_lnurl_pay_response(client: &AsyncClient, ln_addr: &LightningAddress) -> Result<LnUrlResponse, LnurlError> {
    match client.make_request(&ln_addr.lnurlp_url()).await {
        Ok(response @ LnUrlResponse::LnUrlPayResponse(_)) => Ok(response),
        Ok(_) => Err(LnurlError::InvalidResponse),
        Err(_) => Err(LnurlError::RequestFailed),
    }
}

async fn generate_invoice(
    client: &AsyncClient,
    pay_response: &LnUrlResponse,
    amount_satoshis: u64
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
    client.get_invoice(pay, amount_msats, None, None)
        .await
        .map(|invoice| invoice.pr)
        .map_err(|e| LnurlError::InvoiceCreationFailed {
            message: e.to_string(),
        })
}