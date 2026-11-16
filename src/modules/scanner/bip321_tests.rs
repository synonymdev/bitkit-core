//! BIP 321 test vectors.
//!
//! The URIs come from the BIP's appendix
//! (https://github.com/bitcoin/bips/blob/master/bip-0321.mediawiki#examples). The BIP uses intentionally invalid
//! placeholders for every address and invoice, so vectors that need a payable one substitute a valid one of the
//! same kind. Offers and silent-payment addresses are used verbatim, since Bitkit does not parse them.
//! The point-of-sale vectors are the NFC payloads of Square Register (2nd generation) and Numo.

use super::{DecodingError, NetworkType, OnChainInvoice, Scanner};

const BIP_PLACEHOLDER_ADDRESS: &str = "175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W";
/// Stands in for the BIP's placeholder address.
const ADDRESS: &str = "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa";
const BIP_PLACEHOLDER_SEGWIT: [&str; 3] = [
    "bc1qufgy354j3kmvuch987xe4s40836x3h0lg8f5nq",
    "bc1p5swkugezn97763tl0yty6556856uug0q6jflljvep9m4p7339x5qzyrh4q",
    "tb1qghfhmd4zh7ncpmxl3qzhmq566jk8ckq4gafnmq",
];
/// Stand in for the BIP's segwit placeholders: BIP 173 and BIP 350 test vectors.
const SEGWIT_V0: &str = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
const SEGWIT_V1: &str = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0";
const TESTNET_SEGWIT: &str = "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx";

/// A real signet BOLT11 invoice of the size a Square Register serves over NFC: 447 characters, uppercase as in
/// its QR code, for 7,990 sats. `lightning:` plus this invoice is a 465-byte NDEF message.
const POS_INVOICE: &str = concat!(
    "LNTBS79900N1P4TZWUQD962DCH2CTJV5S9YET8D9EHGETJYP8YVSEQWFJKWUN9WDEKJMMWYPNXJ7R5W4EX2GZNW96K",
    "ZUN9YPFX2EMFWD6X2U3QFERYXGRJV4NHYETNWD5K7M3QVE5HSAR4WFJJQ5M3W4SHYEFQ2FJKW6TNW3JHYGZWGEPJQU",
    "N9VAEX2UMND9HKUGRXD9U8GATJV5PP5QURSWPC8QURSWPC8QURSWPC8QURSWPC8QURSWPC8QURSWPC8QURSSP5PYYS",
    "JZGFPYYSJZGFPYYSJZGFPYYSJZGFPYYSJZGFPYYSJZGFPYYS9QRSGQXQ97ZVUQCQZYSKUAHP9D6GKX2H8ERFH5E9NZ",
    "Z52SY0UFR2F6VGK43QEJ0ZJU7FTYQYJTQE8705R2KM9QS5NYFM3ZPR330ML3S3HWJP5D0RERD49QGT5CPUXAWEL",
);

/// Stands in for the BIP's `lnbc420bogusinvoice`: a signed mainnet invoice for 42,000 sats.
fn bip_invoice() -> String {
    use bitcoin::hashes::Hash;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use lightning_invoice::{Currency, InvoiceBuilder};

    let secp = Secp256k1::new();
    let secret_key = SecretKey::from_slice(&[0x21; 32]).unwrap();
    InvoiceBuilder::new(Currency::Bitcoin)
        .amount_milli_satoshis(42_000_000)
        .description("BIP 321".to_string())
        .payment_hash(bitcoin::hashes::sha256::Hash::from_byte_array([1; 32]))
        .payment_secret(lightning_invoice::PaymentSecret([2; 32]))
        .current_timestamp()
        .min_final_cltv_expiry_delta(144)
        .build_signed(|hash| secp.sign_ecdsa_recoverable(hash, &secret_key))
        .unwrap()
        .to_string()
}

async fn decode(uri: &str) -> Result<Scanner, DecodingError> {
    Scanner::decode(uri.to_string()).await
}

async fn onchain(uri: &str) -> OnChainInvoice {
    match decode(uri).await {
        Ok(Scanner::OnChain { invoice }) => invoice,
        other => panic!("expected on-chain for {uri}, got {other:?}"),
    }
}

async fn lightning_bolt11(uri: &str) -> (String, u64, NetworkType) {
    match decode(uri).await {
        Ok(Scanner::Lightning { invoice }) => (
            invoice.bolt11,
            invoice.amount_satoshis,
            invoice.network_type,
        ),
        other => panic!("expected Lightning for {uri}, got {other:?}"),
    }
}

fn param<'a>(invoice: &'a OnChainInvoice, key: &str) -> Option<&'a str> {
    invoice.params.as_ref()?.get(key).map(String::as_str)
}

#[tokio::test]
async fn bip_placeholder_addresses_are_invalid() {
    assert!(matches!(
        decode(&format!("bitcoin:{BIP_PLACEHOLDER_ADDRESS}")).await,
        Err(DecodingError::InvalidAddress)
    ));
    for address in BIP_PLACEHOLDER_SEGWIT {
        assert!(
            matches!(
                decode(&format!("bitcoin:{address}")).await,
                Err(DecodingError::InvalidAddress)
            ),
            "{address}"
        );
    }
}

// Appendix: URIs

#[tokio::test]
async fn just_the_address() {
    let invoice = onchain(&format!("bitcoin:{ADDRESS}")).await;
    assert_eq!(invoice.address, ADDRESS);
    assert_eq!(invoice.amount_satoshis, 0);
}

#[tokio::test]
async fn address_with_label() {
    let invoice = onchain(&format!("bitcoin:{ADDRESS}?label=Luke-Jr")).await;
    assert_eq!(invoice.label.as_deref(), Some("Luke-Jr"));
}

#[tokio::test]
async fn amount_with_label() {
    let invoice = onchain(&format!("bitcoin:{ADDRESS}?amount=20.3&label=Luke-Jr")).await;
    assert_eq!(invoice.amount_satoshis, 2_030_000_000);
    assert_eq!(invoice.label.as_deref(), Some("Luke-Jr"));
}

#[tokio::test]
async fn amount_with_message() {
    let invoice = onchain(&format!(
        "bitcoin:{ADDRESS}?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz"
    ))
    .await;
    assert_eq!(invoice.amount_satoshis, 5_000_000_000);
    assert!(invoice.message.is_some());
}

#[tokio::test]
async fn bolt11_with_onchain_fallback() {
    let bolt11 = bip_invoice();
    let invoice = onchain(&format!("bitcoin:{ADDRESS}?lightning={bolt11}")).await;
    assert_eq!(invoice.address, ADDRESS);
    assert_eq!(param(&invoice, "lightning"), Some(bolt11.as_str()));
}

#[tokio::test]
async fn bolt11_without_fallback() {
    let bolt11 = bip_invoice();
    let (decoded, amount, network) =
        lightning_bolt11(&format!("bitcoin:?lightning={bolt11}")).await;
    assert_eq!(decoded, bolt11);
    assert_eq!(amount, 42_000);
    assert_eq!(network, NetworkType::Bitcoin);
}

#[tokio::test]
async fn bolt12_offer_without_fallback_is_unsupported() {
    assert!(matches!(
        decode("bitcoin:?lno=lno1bogusoffer").await,
        Err(DecodingError::UnsupportedType)
    ));
}

#[tokio::test]
async fn bolt12_offer_or_silent_payment_without_fallback_is_unsupported() {
    assert!(matches!(
        decode("bitcoin:?lno=lno1bogusoffer&sp=sp1qsilentpayment").await,
        Err(DecodingError::UnsupportedType)
    ));
}

#[tokio::test]
async fn silent_payment_without_fallback_is_unsupported() {
    assert!(matches!(
        decode("bitcoin:?sp=sp1qsilentpayment").await,
        Err(DecodingError::UnsupportedType)
    ));
}

#[tokio::test]
async fn silent_payment_with_fallback_pays_the_fallback() {
    let invoice = onchain(&format!("bitcoin:{ADDRESS}?sp=sp1qsilentpayment")).await;
    assert_eq!(invoice.address, ADDRESS);
}

#[tokio::test]
async fn unknown_required_parameters_are_invalid() {
    assert!(matches!(
        decode(&format!(
            "bitcoin:{ADDRESS}?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999"
        ))
        .await,
        Err(DecodingError::InvalidFormat)
    ));
}

#[tokio::test]
async fn unknown_optional_parameters_are_ignored() {
    let invoice = onchain(&format!(
        "bitcoin:{ADDRESS}?somethingyoudontunderstand=50&somethingelseyoudontget=999"
    ))
    .await;
    assert_eq!(invoice.address, ADDRESS);
}

#[tokio::test]
async fn several_segwit_addresses_in_bc_parameters() {
    let invoice = onchain(&format!("bitcoin:?bc={SEGWIT_V0}&bc={SEGWIT_V1}")).await;
    assert_eq!(invoice.address, SEGWIT_V0);
}

#[tokio::test]
async fn uppercase_uris() {
    let upper_v0 = SEGWIT_V0.to_uppercase();
    let upper_v1 = SEGWIT_V1.to_uppercase();

    let invoice = onchain(&format!("BITCOIN:{upper_v0}?BC={upper_v1}")).await;
    assert_eq!(invoice.address, SEGWIT_V0);

    let invoice = onchain(&format!("BITCOIN:?BC={upper_v0}&BC={upper_v1}")).await;
    assert_eq!(invoice.address, SEGWIT_V0);
}

#[tokio::test]
async fn testnet_segwit_address_in_tb_parameter() {
    let invoice = onchain(&format!("bitcoin:?tb={TESTNET_SEGWIT}")).await;
    assert_eq!(invoice.address, TESTNET_SEGWIT);
}

// Appendix: invalid URIs

#[tokio::test]
async fn label_must_not_appear_twice() {
    assert!(matches!(
        decode(&format!("bitcoin:{ADDRESS}?label=Luke-Jr&label=Matt")).await,
        Err(DecodingError::InvalidFormat)
    ));
}

#[tokio::test]
async fn amount_must_not_appear_twice() {
    for query in ["amount=42&amount=10", "amount=42&amount=42"] {
        assert!(
            matches!(
                decode(&format!("bitcoin:{ADDRESS}?{query}")).await,
                Err(DecodingError::InvalidFormat)
            ),
            "{query}"
        );
    }
}

#[tokio::test]
async fn pop_must_not_appear_twice() {
    assert!(matches!(
        decode(&format!(
            "bitcoin:{ADDRESS}?pop=callback%3a&req-pop=callback%3a"
        ))
        .await,
        Err(DecodingError::InvalidFormat)
    ));
}

#[tokio::test]
async fn testnet_address_in_bc_parameter_is_invalid() {
    assert!(matches!(
        decode(&format!("bitcoin:?bc={TESTNET_SEGWIT}")).await,
        Err(DecodingError::InvalidAddress)
    ));
}

// Appendix: proof of payment. Bitkit does not open pop callbacks, so it pays as usual when `pop` is optional and
// refuses the URI when it is required.

#[tokio::test]
async fn optional_pop_is_ignored() {
    let bolt11 = bip_invoice();
    let invoice = onchain(&format!("bitcoin:{ADDRESS}?pop=initiatingapp%3a")).await;
    assert_eq!(invoice.address, ADDRESS);

    let (decoded, _, _) = lightning_bolt11(&format!(
        "bitcoin:?lightning={bolt11}&pop=callbackuri%3abody%3fpop="
    ))
    .await;
    assert_eq!(decoded, bolt11);

    let invoice = onchain(&format!(
        "bitcoin:{ADDRESS}?lightning={bolt11}&pop=app%3a%3f"
    ))
    .await;
    assert_eq!(param(&invoice, "lightning"), Some(bolt11.as_str()));

    let invoice = onchain(&format!(
        "bitcoin:{ADDRESS}?pop=https%3aiwantyouripaddress.com"
    ))
    .await;
    assert_eq!(invoice.address, ADDRESS);
}

#[tokio::test]
async fn required_pop_is_refused() {
    assert!(matches!(
        decode(&format!(
            "bitcoin:{ADDRESS}?req-pop=https%3aevilwebsite.com"
        ))
        .await,
        Err(DecodingError::InvalidFormat)
    ));
}

// Point-of-sale NFC payloads

#[tokio::test]
async fn square_register_lightning_uri() {
    let (bolt11, amount, network) = lightning_bolt11(&format!("lightning:{POS_INVOICE}")).await;
    assert_eq!(bolt11, POS_INVOICE.to_lowercase());
    assert_eq!(amount, 7_990);
    assert_eq!(network, NetworkType::Signet);
}

#[tokio::test]
async fn numo_bip321_uri_without_address() {
    for uri in [
        format!("bitcoin:?lightning={POS_INVOICE}"),
        format!("BITCOIN:?LIGHTNING={POS_INVOICE}"),
        format!("bitcoin:?lightning={}", POS_INVOICE.to_lowercase()),
        format!("bitcoin:?amount=0.0000799&lightning={POS_INVOICE}&creq=creqA1"),
    ] {
        let (bolt11, amount, network) = lightning_bolt11(&uri).await;
        assert_eq!(bolt11, POS_INVOICE.to_lowercase(), "{uri}");
        assert_eq!(amount, 7_990, "{uri}");
        assert_eq!(network, NetworkType::Signet, "{uri}");
    }
}

#[tokio::test]
async fn bc_parameter_keeps_the_lightning_instruction() {
    let invoice = onchain(&format!("bitcoin:?bc={SEGWIT_V0}&lightning={POS_INVOICE}")).await;
    assert_eq!(invoice.address, SEGWIT_V0);
    assert_eq!(param(&invoice, "lightning"), Some(POS_INVOICE));
}

// Edge cases

#[tokio::test]
async fn uri_without_address_or_instruction_is_invalid() {
    for uri in [
        "bitcoin:",
        "bitcoin:?",
        "bitcoin:?amount=0.001",
        "bitcoin:?lightning=",
        "bitcoin:?label=Luke-Jr",
    ] {
        assert!(
            matches!(decode(uri).await, Err(DecodingError::InvalidAddress)),
            "{uri}"
        );
    }
}

#[tokio::test]
async fn uri_without_address_keeps_singleton_and_required_checks() {
    let bolt11 = bip_invoice();
    for uri in [
        format!("bitcoin:?lightning={bolt11}&label=a&label=b"),
        format!("bitcoin:?lightning={bolt11}&req-pop=callback%3a"),
        format!("bitcoin:?lightning={bolt11}&req-unknown=1"),
    ] {
        assert!(
            matches!(decode(&uri).await, Err(DecodingError::InvalidFormat)),
            "{uri}"
        );
    }
}

#[tokio::test]
async fn uri_without_address_rejects_an_embedded_uri() {
    let bolt11 = bip_invoice();
    let uri = format!("bitcoin:?lightning={bolt11}bitcoin:{SEGWIT_V0}?amount=0.002");
    assert!(matches!(
        decode(&uri).await,
        Err(DecodingError::InvalidFormat)
    ));
}

#[tokio::test]
async fn invalid_lightning_instruction_is_invalid() {
    assert!(matches!(
        decode("bitcoin:?lightning=lnbc420bogusinvoice").await,
        Err(DecodingError::InvalidFormat)
    ));
}

#[tokio::test]
async fn invalid_segwit_instruction_is_invalid() {
    for uri in [
        "bitcoin:?bc=bc1qbogus".to_string(),
        format!("bitcoin:?bcrt={SEGWIT_V0}"),
        format!("bitcoin:?tb={SEGWIT_V0}&lightning={POS_INVOICE}"),
    ] {
        assert!(
            matches!(decode(&uri).await, Err(DecodingError::InvalidAddress)),
            "{uri}"
        );
    }
}
