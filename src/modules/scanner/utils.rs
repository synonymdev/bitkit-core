use crate::DecodingError;

/// Parses a BIP 321 amount into satoshis.
///
/// BIP 321 allows only decimal BTC: digits, optionally followed by a period and digits
/// (`*digit [ "." *digit ]`). Units, commas, signs and exponents are invalid, as are digits below one satoshi.
pub fn parse_amount_as_satoshis(amount: &str) -> Result<u64, DecodingError> {
    const SATS_PER_BTC: u64 = 100_000_000;
    const SAT_DIGITS: usize = 8;

    let (whole, fraction) = amount.split_once('.').unwrap_or((amount, ""));
    let is_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !is_digits(whole) || !is_digits(fraction) {
        return Err(DecodingError::InvalidAmount);
    }
    let (sat_digits, below_a_sat) = fraction.split_at(fraction.len().min(SAT_DIGITS));
    if below_a_sat.bytes().any(|byte| byte != b'0') {
        return Err(DecodingError::InvalidAmount);
    }

    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| DecodingError::InvalidAmount)?
    };
    let sats: u64 = format!("{sat_digits:0<SAT_DIGITS$}")
        .parse()
        .map_err(|_| DecodingError::InvalidAmount)?;
    whole
        .checked_mul(SATS_PER_BTC)
        .and_then(|btc| btc.checked_add(sats))
        .ok_or(DecodingError::InvalidAmount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bip321_amounts_parse_exactly() {
        for (amount, sats) in [
            ("50", 5_000_000_000),
            ("50.00", 5_000_000_000),
            ("20.3", 2_030_000_000),
            ("0.0000799", 7_990),
            ("0.00000001", 1),
            ("0.000000010", 1),
            (".5", 50_000_000),
            ("5.", 500_000_000),
            ("0.1", 10_000_000),
        ] {
            assert_eq!(
                parse_amount_as_satoshis(amount).ok(),
                Some(sats),
                "{amount}"
            );
        }
    }

    /// Writes `sats` as decimal BTC, with all eight fraction digits or with trailing zeros trimmed.
    fn btc(sats: u64, trim: bool) -> String {
        let fraction = format!("{:08}", sats % 100_000_000);
        let fraction = if trim {
            fraction.trim_end_matches('0')
        } else {
            &fraction
        };
        match fraction {
            "" => (sats / 100_000_000).to_string(),
            fraction => format!("{}.{fraction}", sats / 100_000_000),
        }
    }

    #[test]
    fn every_satoshi_amount_round_trips() {
        let mut sats_values = vec![
            0,
            1,
            99_999_999,
            100_000_000,
            100_000_001,
            u64::MAX - 1,
            u64::MAX,
        ];
        // A fixed linear congruential sequence spreads samples over the whole u64 range.
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        for _ in 0..20_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            sats_values.push(state >> (state % 64));
        }
        for sats in sats_values {
            for trim in [false, true] {
                let amount = btc(sats, trim);
                assert_eq!(
                    parse_amount_as_satoshis(&amount).ok(),
                    Some(sats),
                    "{amount}"
                );
            }
        }
        assert_eq!(btc(u64::MAX, false), "184467440737.09551615");
        assert!(parse_amount_as_satoshis("184467440737.09551616").is_err());
        assert!(parse_amount_as_satoshis("184467440738").is_err());
    }

    #[test]
    fn amounts_outside_the_bip321_grammar_are_invalid() {
        for amount in [
            "",
            ".",
            "0.0000799BTC",
            "0.0000799,50",
            "50,000.00",
            "1e3",
            "-1",
            "+1",
            " 1",
            "inf",
            "NaN",
            "0.000000015",
            "1.2.3",
            "184467440738",
        ] {
            assert!(
                matches!(
                    parse_amount_as_satoshis(amount),
                    Err(DecodingError::InvalidAmount)
                ),
                "{amount}"
            );
        }
    }
}
