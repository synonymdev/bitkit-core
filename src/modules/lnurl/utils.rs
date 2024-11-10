use lazy_regex::Lazy;
use regex::Regex;

static LNURL_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-z0-9._-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap()
});

pub fn is_lnurl_address(address: &str) -> bool {
    LNURL_ADDRESS_REGEX.is_match(address)
}
