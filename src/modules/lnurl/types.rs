pub struct LightningAddressInvoice {
    pub address: String,
    pub amount_satoshis: u64,
    pub invoice: String,
}

pub struct ChannelRequestParams {
    pub k1: String,
    pub callback: String,
    pub local_node_id: String,
    pub is_private: bool,
    pub cancel: bool,
}

pub struct WithdrawCallbackParams {
    pub k1: String,
    pub callback: String,
    pub payment_request: String,
}

pub struct LnurlAuthParams {
    pub domain: String,
    pub k1: String,
    pub callback: String,
    pub hashing_key: [u8; 32],
}
