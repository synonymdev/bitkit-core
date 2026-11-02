use super::*;

fn binding(value: &Value) -> UsdtPaymentProofBinding {
    UsdtPaymentProofBinding {
        payer: value["payer"].as_str().unwrap().into(),
        payee: value["payee"].as_str().unwrap().into(),
        payment_app_id: value["paymentAppId"].as_str().unwrap().into(),
        payment_request_id: value["paymentRequestId"].as_str().unwrap().into(),
        payment_reference: value["paymentReference"].as_str().unwrap().into(),
        payment_endpoint_identifier: value["paymentEndpointIdentifier"].as_str().unwrap().into(),
        period_starts_at: value["periodStartsAt"].as_str().unwrap().into(),
        period_ends_at: value["periodEndsAt"].as_str().unwrap().into(),
        conversion_quote_id: value["conversionQuoteId"].as_str().unwrap().into(),
    }
}

#[test]
fn erc20_profile_matches_published_one_time_and_recurring_vectors() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/erc20-payment-proofs.json")).unwrap();
    for vector in fixture["vectors"].as_array().unwrap() {
        let data = &vector["typed_data"];
        let chain = data["domain"]["chainId"].as_u64().unwrap();
        let hash = canonical_hash(data["message"]["transactionHash"].as_str().unwrap()).unwrap();
        let index = decimal_index(data["message"]["receiptLogIndex"].as_str().unwrap()).unwrap();
        let mut binding = binding(&data["message"]["request"]);
        let digest = proof_digest(chain, hash, index, &binding);
        assert_eq!(format!("{digest:#x}"), vector["digest"].as_str().unwrap());
        let signature = vector["signature"].as_str().unwrap();
        let signer = proof_sender(digest, signature).unwrap();
        assert_eq!(signer.to_checksum(None), vector["signer"].as_str().unwrap());
        let app = std::mem::replace(&mut binding.payment_app_id, "another-app".into());
        assert_ne!(
            proof_sender(proof_digest(chain, hash, index, &binding), signature).unwrap(),
            signer
        );
        binding.payment_app_id = app;
        binding.payment_reference = vector["changed_reference"].as_str().unwrap().into();
        let changed = proof_digest(chain, hash, index, &binding);
        assert_eq!(
            format!("{changed:#x}"),
            vector["changed_reference_digest"].as_str().unwrap()
        );
        assert_ne!(proof_sender(changed, signature).unwrap(), signer);
        assert_ne!(
            proof_sender(proof_digest(chain + 1, hash, index, &binding), signature).unwrap(),
            signer
        );
        let mut high_s = signature.parse::<Bytes>().unwrap().to_vec();
        let order = U256::from_str_radix(
            "fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141",
            16,
        )
        .unwrap();
        let s = U256::from_be_slice(&high_s[32..64]);
        high_s[32..64].copy_from_slice(&(order - s).to_be_bytes::<32>());
        high_s[64] = if high_s[64] == 27 { 28 } else { 27 };
        assert!(proof_sender(digest, &format!("{:#x}", Bytes::from(high_s))).is_err());
    }
}

#[test]
fn proof_identifiers_have_one_canonical_encoding() {
    assert_eq!(decimal_index("0").unwrap(), U256::ZERO);
    assert_eq!(decimal_index(&U256::MAX.to_string()).unwrap(), U256::MAX);
    for value in [
        "",
        "00",
        "01",
        "+1",
        "-1",
        " 1",
        "1 ",
        "1e1",
        "１",
        "0x1",
        "115792089237316195423570985008687907853269984665640564039457584007913129639936",
    ] {
        assert!(decimal_index(value).is_err(), "{value}");
    }
    assert!(canonical_hash(&format!("0x{}", "AB".repeat(32))).is_err());
}
