use std::collections::HashMap;
use std::str::FromStr;
use async_trait::async_trait;
use bitcoin::Network;
use lazy_regex::{lazy_regex, Lazy};
use lightning_invoice::Bolt11Invoice;
use lnurl::{Builder, LnUrlResponse};
use lnurl::lightning_address::LightningAddress;
use lnurl::lnurl::LnUrl;
use url::Url;
use chrono::{DateTime, Utc};
use regex::Regex;
use crate::lnurl::is_lnurl_address;
use super::errors::DecodingError;
use super::types::*;
use super::utils::*;

use crate::modules::onchain::BitcoinAddressValidator;

impl LightningInvoice {
    pub fn get_timestamp(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(self.timestamp_seconds as i64, 0)
            .unwrap_or_default()
    }

    pub fn get_expiry(&self) -> core::time::Duration {
        core::time::Duration::from_secs(self.expiry_seconds)
    }

    pub fn get_network(&self) -> Option<Network> {
        match self.network_type {
            NetworkType::Bitcoin => Some(Network::Bitcoin),
            NetworkType::Testnet => Some(Network::Testnet),
            NetworkType::Regtest => Some(Network::Regtest),
            NetworkType::Signet => Some(Network::Signet),
        }
    }
}

impl Scanner {
    /// Normalizes a Bitcoin address for validation.
    /// Only lowercases bech32 addresses (bc1, tb1, bcrt1) which are case-insensitive.
    /// Preserves case for legacy addresses (P2PKH/P2SH) which are case-sensitive.
    fn normalize_address_for_validation(address: &str) -> String {
        let address_lower = address.to_lowercase();
        // Check if it's a bech32 address (starts with bc1, tb1, bcrt1, etc.)
        if address_lower.starts_with("bc1") || 
           address_lower.starts_with("tb1") || 
           address_lower.starts_with("bcrt1") {
            address_lower
        } else {
            // Legacy address - preserve original case
            address.to_string()
        }
    }

    pub async fn decode(invoice_str: String) -> Result<Self, DecodingError> {
        let invoice_str = invoice_str.trim();
        let invoice_str = invoice_str
            .strip_prefix("lightning:")
            .unwrap_or(invoice_str);

        // Handle Bitkit deep links
        if invoice_str.starts_with("bitkit://") {
            let data = invoice_str.replace("bitkit://", "");
            
            // Check if it's a gift code format: bitkit://gift-<code>-<amount>
            if data.starts_with("gift-") {
                let parts: Vec<&str> = data.splitn(3, '-').collect();
                if parts.len() == 3 && parts[0] == "gift" {
                    let code = parts[1];
                    let amount = parts[2].parse::<u64>()
                        .map_err(|_| DecodingError::InvalidFormat)?;
                    return Ok(Scanner::Gift {
                        code: code.to_string(),
                        amount,
                    });
                }
            }
            
            return Box::pin(Self::decode(data)).await;
        }

        // Node connection string handling
        if invoice_str.contains('@') {
            let parts: Vec<&str> = invoice_str.split(':').collect();
            if parts.len() == 2 && parts[0].contains('@') {
                if invoice_str.contains("onion") {
                    return Err(DecodingError::UnsupportedType);
                }
                return Ok(Scanner::NodeId {
                    url: invoice_str.to_string(),
                    network: NetworkType::Bitcoin
                });
            }
        }

        if  invoice_str.to_lowercase().contains("lightning:") ||
            invoice_str.to_lowercase().starts_with("lntb") ||
            invoice_str.to_lowercase().starts_with("lnbc")
        {
            let invoice_lower = invoice_str.to_lowercase();
            let invoice = invoice_lower
                .strip_prefix("lightning:")
                .unwrap_or(&invoice_lower);
            Self::decode_lightning(invoice)
        } else if invoice_str.to_lowercase().starts_with("bitcoin:") {
            // Extract address and query params (preserve original case for query params)
            let (address_part, query_part) = invoice_str[8..].split_once('?')
                .map(|(addr, query)| (addr, Some(query)))
                .unwrap_or((&invoice_str[8..], None));
            
            // Normalize address (only lowercase bech32, preserve legacy case)
            let address_normalized = Self::normalize_address_for_validation(address_part);
            
            if BitcoinAddressValidator::validate_address(&address_normalized).is_ok() {
                let normalized = if let Some(query) = query_part {
                    format!("bitcoin:{}?{}", address_normalized, query)
                } else {
                    format!("bitcoin:{}", address_normalized)
                };
                Self::decode_onchain(&normalized)
            } else {
                Err(DecodingError::InvalidAddress)
            }
        } else if invoice_str.to_lowercase().starts_with("pubkyauth:") {
            Ok(Scanner::PubkyAuth {
                data: invoice_str.to_string()
            })
        } else if let Some(lnurl) = Self::find_lnurl(invoice_str) {
            Self::decode_lnurl(&lnurl).await
        } else if is_lnurl_address(invoice_str) {
            Self::decode_lnurl(invoice_str).await
        } else {
            // If no prefix, validate as a raw Bitcoin address
            // Normalize address (only lowercase bech32, preserve legacy case)
            let address_normalized = Self::normalize_address_for_validation(invoice_str);
            if BitcoinAddressValidator::validate_address(&address_normalized).is_ok() {
                Self::decode_onchain(&format!("bitcoin:{}", address_normalized))
            } else {
                Err(DecodingError::InvalidAddress)
            }
        }
    }

    pub fn find_lnurl(text: &str) -> Option<String> {
        static LNURL_REGEX: Lazy<Regex> = lazy_regex!(r"^(?:(http.*|bitcoin:.*)[&?]lightning=|lightning:)?(lnurl1[02-9ac-hj-np-z]+)");

        // Convert input to lowercase and store it in a variable
        let text_lower = text.to_lowercase();
        let text_trimmed = text_lower.trim();

        // Find the first match and return the second capture group if found
        LNURL_REGEX
            .captures(text_trimmed)
            .and_then(|caps| caps.get(2))
            .map(|m| m.as_str().to_string())
    }

    async fn decode_lnurl(invoice_str: &str) -> Result<Scanner, DecodingError> {
        // Helper function to convert responses to Scanner enum
        fn convert_response(uri: String, response: LnUrlResponse) -> Result<Scanner, DecodingError> {
            match response {
                LnUrlResponse::LnUrlPayResponse(pay) => {
                    Ok(Scanner::LnurlPay {
                        data: LnurlPayData {
                            uri,
                            callback: pay.callback,
                            min_sendable: pay.min_sendable,
                            max_sendable: pay.max_sendable,
                            metadata_str: pay.metadata,
                            comment_allowed: pay.comment_allowed,
                            allows_nostr: pay.allows_nostr.unwrap_or(false),
                            nostr_pubkey: pay.nostr_pubkey.map(|key| key.serialize().to_vec()),
                        }
                    })
                },
                LnUrlResponse::LnUrlWithdrawResponse(withdraw) => {
                    Ok(Scanner::LnurlWithdraw {
                        data: LnurlWithdrawData {
                            uri,
                            callback: withdraw.callback,
                            k1: withdraw.k1,
                            default_description: withdraw.default_description,
                            min_withdrawable: withdraw.min_withdrawable,
                            max_withdrawable: withdraw.max_withdrawable,
                            tag: withdraw.tag.to_string(),
                        }
                    })
                },
                LnUrlResponse::LnUrlChannelResponse(channel) => {
                    Ok(Scanner::LnurlChannel {
                        data: LnurlChannelData {
                            uri: channel.uri,
                            callback: channel.callback,
                            k1: channel.k1,
                            tag: channel.tag.to_string(),
                        }
                    })
                },
                _ => Err(DecodingError::InvalidFormat)
            }
        }

        // Handle Lightning Address
        if is_lnurl_address(invoice_str) {
            if let Ok(ln_addr) = LightningAddress::from_str(invoice_str) {
                let url = ln_addr.lnurlp_url();
                let async_client = Builder::default().build_async()
                    .map_err(|_| DecodingError::InvalidFormat)?;

                let response = async_client.make_request(&*url)
                    .await
                    .map_err(|_| DecodingError::InvalidFormat)?;

                return convert_response(url, response);
            }
        }

        // Handle LNURL
        let lnurl = LnUrl::from_str(invoice_str)
            .map_err(|_| DecodingError::InvalidFormat)?;

        // Check for LNURL-auth
        if lnurl.is_lnurl_auth() {
            let parsed_url = Url::parse(&lnurl.url)
                .map_err(|_| DecodingError::InvalidFormat)?;

            let k1 = parsed_url.query_pairs()
                .find(|(key, _)| key == "k1")
                .map(|(_, value)| value.to_string())
                .ok_or(DecodingError::InvalidFormat)?;

            let domain = parsed_url.host_str()
                .ok_or(DecodingError::InvalidFormat)?
                .to_string();

            return Ok(Scanner::LnurlAuth {
                data: LnurlAuthData {
                    uri: lnurl.url,
                    tag: "login".to_string(),
                    k1,
                    domain,
                }
            });
        }

        // Handle other LNURL types
        let async_client = Builder::default().build_async()
            .map_err(|_| DecodingError::InvalidFormat)?;

        let response = async_client.make_request(&lnurl.url)
            .await
            .map_err(|_| DecodingError::InvalidFormat)?;

        convert_response(lnurl.url, response)
    }

    fn decode_lightning(invoice_str: &str) -> Result<Self, DecodingError> {
        let bolt11_invoice = Bolt11Invoice::from_str(invoice_str)
            .map_err(|_| DecodingError::InvalidFormat)?;

        let network = NetworkType::from(bolt11_invoice.network());
        let amount_satoshis: u64 = bolt11_invoice.amount_milli_satoshis().unwrap_or(0) / 1000u64;
        let is_expired = bolt11_invoice.is_expired();

        let timestamp = DateTime::<Utc>::from_timestamp(
            bolt11_invoice.duration_since_epoch().as_secs() as i64,
            0
        ).unwrap_or_default();

        let description = match bolt11_invoice.description() {
            lightning_invoice::Bolt11InvoiceDescription::Direct(desc) => Some(desc.to_string()),
            lightning_invoice::Bolt11InvoiceDescription::Hash(_) => None,
        };

        let payment_hash = AsRef::<[u8]>::as_ref(bolt11_invoice.payment_hash()).to_vec();
        let payee_node_id = bolt11_invoice.recover_payee_pub_key()
            .serialize().to_vec();

        let expiry = bolt11_invoice.expiry_time();

        Ok(Scanner::Lightning {
            invoice: LightningInvoice {
                bolt11: invoice_str.to_string(),
                payment_hash,
                amount_satoshis,
                timestamp_seconds: timestamp.timestamp() as u64,
                expiry_seconds: expiry.as_secs(),
                is_expired,
                description,
                network_type: network,
                payee_node_id: Some(payee_node_id),
            }
        })
    }

    fn handle_node_id(invoice_str: &str) -> Result<Self, DecodingError> {
        Ok(Scanner::NodeId {
            url: invoice_str.to_string(),
            network: NetworkType::Bitcoin
        })
    }

    fn decode_onchain(invoice_str: &str) -> Result<Self, DecodingError> {
        let parts: Vec<&str> = invoice_str
            .strip_prefix("bitcoin:")
            .unwrap_or(invoice_str)
            .split('?')
            .collect();

        let address = parts[0].to_string();

        let params = if parts.len() > 1 {
            parts[1]
                .split('&')
                .filter_map(|param| {
                    param.split_once('=').map(|(k, v)| {
                        (k.to_string(), v.to_string())
                    })
                })
                .collect::<HashMap<String, String>>()
        } else {
            HashMap::new()
        };

        let amount_satoshis = params.get("amount")
            .and_then(|amount| parse_amount_as_satoshis(amount).ok())
            .unwrap_or(0);

        let label = params.get("label")
            .map(String::from);

        let message = params.get("message")
            .map(String::from);

        Ok(Scanner::OnChain {
            invoice: OnChainInvoice {
                address,
                amount_satoshis,
                label,
                message,
                params: Some(params),
            }
        })
    }
}

#[async_trait]
pub trait AsyncFromStr: Sized {
    type Err;
    async fn from_str(s: &str) -> Result<Self, Self::Err>;
}

#[async_trait]
impl AsyncFromStr for Scanner {
    type Err = DecodingError;

    async fn from_str(s: &str) -> Result<Self, Self::Err> {
        Scanner::decode(s.to_string()).await
    }
}