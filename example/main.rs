
use bitkitcore::*;

fn handle_decode_result(result: Result<Scanner, DecodingError>) {
    match result {
        Ok(Scanner::Lightning { invoice: ln_invoice }) => {
            println!("Successfully decoded Lightning invoice:");
            println!("Payment hash: {:?}", ln_invoice.payment_hash);
            println!("Amount: {} sats", ln_invoice.amount_satoshis);
            println!("Network: {}", ln_invoice.network_type);
            println!("Timestamp: {}", ln_invoice.get_timestamp());
            println!("Expiry: {} seconds", ln_invoice.expiry_seconds);
            println!("Is expired: {}", ln_invoice.is_expired);
            if let Some(desc) = ln_invoice.description {
                println!("Description: {}", desc);
            }
            if let Some(node_id) = ln_invoice.payee_node_id {
                println!("Payee node ID: {:?}", node_id);
            }
        }

        Ok(Scanner::OnChain { invoice: btc_invoice }) => {
            println!("\nSuccessfully decoded on-chain invoice:");
            println!("Address: {}", btc_invoice.address);
            println!("Amount Sats: {}", btc_invoice.amount_satoshis);
            if let Some(label) = &btc_invoice.label {
                println!("Label: {}", label);
            }
            if let Some(message) = &btc_invoice.message {
                println!("Message: {}", message);
            }
            if let Some(params) = &btc_invoice.params {
                println!("\nAll parameters:");
                for (key, value) in params {
                    println!("  {}: {}", key, value);
                }
            }
        }

        Ok(Scanner::PubkyAuth { auth }) => {
            println!("\nSuccessfully decoded Pubkey Auth:");
            println!("Data: {}", auth.data);
        }

        Ok(Scanner::LnurlChannel { data }) => {
            println!("\nSuccessfully decoded LNURL-channel:");
            println!("URI: {}", data.uri);
            println!("Callback: {}", data.callback);
            println!("K1: {}", data.k1);
            println!("Tag: {}", data.tag);
        }

        Ok(Scanner::LnurlAuth { data }) => {
            println!("\nSuccessfully decoded LNURL-auth:");
            println!("URI: {}", data.uri);
            println!("Tag: {}", data.tag);
            println!("K1: {}", data.k1);
        }

        Ok(Scanner::LnurlWithdraw { data }) => {
            println!("\nSuccessfully decoded LNURL-withdraw:");
            println!("URI: {}", data.uri);
            println!("Callback: {}", data.callback);
            println!("K1: {}", data.k1);
            println!("Default Description: {}", data.default_description);
            println!("Max Withdrawable: {} sats", data.max_withdrawable);
            if let Some(min) = data.min_withdrawable {
                println!("Min Withdrawable: {} sats", min);
            }
            println!("Tag: {}", data.tag);
        }

        Ok(Scanner::LnurlAddress { data }) => {
            println!("\nSuccessfully decoded Lightning Address:");
            println!("URI: {}", data.uri);
            println!("Username: {}", data.username);
            println!("Domain: {}", data.domain);
        }

        Ok(Scanner::LnurlPay { data }) => {
            println!("\nSuccessfully decoded LNURL-pay:");
            println!("URI: {}", data.uri);
            println!("Callback: {}", data.callback);
            println!("Min Sendable: {} sats", data.min_sendable);
            println!("Max Sendable: {} sats", data.max_sendable);
            println!("Metadata: {}", data.metadata_str);
            if let Some(comment_length) = data.comment_allowed {
                println!("Comment Allowed (max length): {}", comment_length);
            }
            println!("Allows Nostr: {}", data.allows_nostr);
            if let Some(pubkey) = &data.nostr_pubkey {
                println!("Nostr Pubkey: {:?}", pubkey);
            }
        }

        Ok(Scanner::NodeId { url, network }) => {
            println!("\nSuccessfully decoded Node Connection:");
            println!("URL: {}", url);
            println!("Network: {}", network);
            println!("Type: {}", if url.contains("onion") { "Tor" } else { "Clearnet" });
        }

        Ok(Scanner::TreasureHunt { chest_id }) => {
            println!("\nSuccessfully decoded Treasure Hunt:");
            println!("Chest ID: {}", chest_id);
        }

        Ok(Scanner::OrangeTicket { ticket_id }) => {
            println!("\nSuccessfully decoded Orange Ticket:");
            println!("Ticket ID: {}", ticket_id);
        }

        Err(e) => {
            println!("Error decoding invoice: {:?}", e);
        }
    }
}

#[tokio::main]
async fn main() {
    let lightning_invoice = "lightning:lnbc543210n1pnjdrvfpp5s720f4z6wzvjwpdnrlpffgct375l46yu9c6cpe7gdvvdfay47cnsdqqcqzzsxqrrsssp53uty4kfw8k3wmw4ga802udavz7e64tc7dmaz2cmtkj9srfxaq3ps9p4gqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpqysgqwl2tdhzm9e6mtedt7a4263yw7dqxehdwjnjk23r4g8tuppk6rs994f6scunwsev3w207tjldwkpdt32rcegzphgk05c0lctv8he7smgqyfn5xq";
    let lnurl_address = "ben@zaps.benthecarman.com";
    let lnurl_auth = "lightning:LNURL1DP68GURN8GHJ7MRWW4EXCTNXD9SHG6NPVCHXXMMD9AKXUATJDSKKCMM8D9HR7ARPVU7KCMM8D9HZV6E385URYWP3XQEXZDPKX4JNJCEHXSMN2WRRXQCK2VRZX3SKYV3CVGMNGE35VGEKXDRXXPSNJVEKXDJNSVFN8Y6XGCNRXP3NSVRRXUUNQVNYS9JR2C";
    let lnurl_pay = "lightning:LNURL1DP68GURN8GHJ7MRWW4EXCTNXD9SHG6NPVCHXXMMD9AKXUATJDSKHQCTE8AEK2UMND9HKU0FCXGURZVPJVY6RVDT9893NWDPHX5UXXVP3V5CXYDRPVGERSC3HX3NRGC3NVV6XVVRP8YENVVM98QCNXWF5V33XXVRR8QCXXDEEXQEXGWEM4XD";
    let lnurl_withdraw = "lightning:LNURL1DP68GURN8GHJ7MRWW4EXCTNXD9SHG6NPVCHXXMMD9AKXUATJDSKHW6T5DPJ8YCTH8AEK2UMND9HKU0FCXGURZVPJVY6RVDT9893NWDPHX5UXXVP3V5CXYDRPVGERSC3HX3NRGC3NVV6XVVRP8YENVVM98QCNXWF5V33XXVRR8QCXXDEEXQEXGHSD4GA";
    let lnurl_channel = "lightning:LNURL1DP68GURN8GHJ7MRWW4EXCTNXD9SHG6NPVCHXXMMD9AKXUATJDSKKX6RPDEHX2MPLWDJHXUMFDAHR6WPJ8QCNQVNPXSMR2EFEVVMNGDE48P3NQVT9XP3RGCTZXGUXYDE5VC6XYVMRX3NRQCFEXVMRXEFCXYENJDRYVF3NQCECXP3NWWFSXFJQA98HUU";
    let bitcoin_address_invoice = "bitcoin:bc1qxj2ecu0uv7mk767cwnf8kdu22rv63suqvgxxhd?amount=50&label=My Example&message=Donation";
    let invalid_bitcoin_address = "bitcoin:bc1qxj2ecu07mk767cwnf8kdu22rv63suqvgxxhd?amount=50&label=My Example&message=Donation&extra=invalid";
    let native_segwit_address = "bc1qxj2ecu0uv7mk767cwnf8kdu22rv63suqvgxxhd";
    let wrapped_segwit_address = "3ExqhLGnbTChwCLZLC7V3T6yuUydcHMbdk";
    let legacy_address = "199Grz1BcL5KffikSAtbgngAPgYZZRa3cs";
    let random_string = "random_string";
    let tor_node_id = "72413cc3e96168cb4320f992bfa483865133dc28d@3phi2gcmu3nsbvux53hixrxjgyg3u6vd6kqy3yq6rlrvudqrjsxir6id.onion:9735";
    let node_id = "039b8b4dd1d88c2c5db374290cda397a8f5d79f312d6ea5d5bfdfc7c6ff363eae3@34.65.111.104:9735";
    handle_decode_result(Scanner::decode(lnurl_address.to_string()).await);
}
