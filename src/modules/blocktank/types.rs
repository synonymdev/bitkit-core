use rust_blocktank_client::{
    BitcoinNetworkEnum as ExternalBitcoinNetworkEnum,
    BtBolt11InvoiceState as ExternalBtBolt11InvoiceState,
    BtChannelOrderErrorType as ExternalBtChannelOrderErrorType,
    BtOpenChannelState as ExternalBtOpenChannelState, BtOrderState as ExternalBtOrderState,
    BtOrderState2 as ExternalBtOrderState2, BtPaymentState as ExternalBtPaymentState,
    BtPaymentState2 as ExternalBtPaymentState2, CJitStateEnum as ExternalCJitStateEnum,
    CreateCjitOptions as ExternalCreateCjitOptions,
    CreateOrderOptions as ExternalCreateOrderOptions, FeeRates as ExternalFeeRates,
    FundingTx as ExternalFundingTx, IBt0ConfMinTxFeeWindow as ExternalIBt0ConfMinTxFeeWindow,
    IBtBolt11Invoice as ExternalIBtBolt11Invoice, IBtChannel as ExternalIBtChannel,
    IBtChannelClose as ExternalIBtChannelClose,
    IBtEstimateFeeResponse as ExternalIBtEstimateFeeResponse,
    IBtEstimateFeeResponse2 as ExternalIBtEstimateFeeResponse2, IBtInfo as ExternalIBtInfo,
    IBtInfoOnchain as ExternalIBtInfoOnchain, IBtInfoOptions as ExternalIBtInfoOptions,
    IBtInfoVersions as ExternalIBtInfoVersions,
    IBtOnchainTransaction as ExternalIBtOnchainTransaction,
    IBtOnchainTransactions as ExternalIBtOnchainTransactions, IBtOrder as ExternalIBtOrder,
    IBtPayment as ExternalIBtPayment, ICJitEntry as ExternalICJitEntry,
    IDiscount as ExternalIDiscount, IGift as ExternalIGift,
    IGiftBolt11Invoice as ExternalIGiftBolt11Invoice, IGiftBtcAddress as ExternalIGiftBtcAddress,
    IGiftCode as ExternalIGiftCode, IGiftLspNode as ExternalIGiftLspNode,
    IGiftOrder as ExternalIGiftOrder, IGiftPayment as ExternalIGiftPayment,
    ILspNode as ExternalILspNode, IManualRefund as ExternalIManualRefund,
    ManualRefundStateEnum as ExternalManualRefundStateEnum,
};
use serde::{Deserialize, Serialize};

#[derive(uniffi::Enum, Deserialize, Serialize)]
pub enum BitcoinNetworkEnum {
    Mainnet,
    Testnet,
    Signet,
    Regtest,
}

impl From<ExternalBitcoinNetworkEnum> for BitcoinNetworkEnum {
    fn from(other: ExternalBitcoinNetworkEnum) -> Self {
        match other {
            ExternalBitcoinNetworkEnum::Mainnet => BitcoinNetworkEnum::Mainnet,
            ExternalBitcoinNetworkEnum::Testnet => BitcoinNetworkEnum::Testnet,
            ExternalBitcoinNetworkEnum::Signet => BitcoinNetworkEnum::Signet,
            ExternalBitcoinNetworkEnum::Regtest => BitcoinNetworkEnum::Regtest,
        }
    }
}

impl From<BitcoinNetworkEnum> for ExternalBitcoinNetworkEnum {
    fn from(other: BitcoinNetworkEnum) -> Self {
        match other {
            BitcoinNetworkEnum::Mainnet => ExternalBitcoinNetworkEnum::Mainnet,
            BitcoinNetworkEnum::Testnet => ExternalBitcoinNetworkEnum::Testnet,
            BitcoinNetworkEnum::Signet => ExternalBitcoinNetworkEnum::Signet,
            BitcoinNetworkEnum::Regtest => ExternalBitcoinNetworkEnum::Regtest,
        }
    }
}

#[derive(uniffi::Enum, Deserialize, Serialize)]
pub enum BtBolt11InvoiceState {
    Pending,
    Holding,
    Paid,
    Canceled,
}

impl From<ExternalBtBolt11InvoiceState> for BtBolt11InvoiceState {
    fn from(other: ExternalBtBolt11InvoiceState) -> Self {
        match other {
            ExternalBtBolt11InvoiceState::Pending => BtBolt11InvoiceState::Pending,
            ExternalBtBolt11InvoiceState::Holding => BtBolt11InvoiceState::Holding,
            ExternalBtBolt11InvoiceState::Paid => BtBolt11InvoiceState::Paid,
            ExternalBtBolt11InvoiceState::Canceled => BtBolt11InvoiceState::Canceled,
        }
    }
}

impl From<BtBolt11InvoiceState> for ExternalBtBolt11InvoiceState {
    fn from(other: BtBolt11InvoiceState) -> Self {
        match other {
            BtBolt11InvoiceState::Pending => ExternalBtBolt11InvoiceState::Pending,
            BtBolt11InvoiceState::Holding => ExternalBtBolt11InvoiceState::Holding,
            BtBolt11InvoiceState::Paid => ExternalBtBolt11InvoiceState::Paid,
            BtBolt11InvoiceState::Canceled => ExternalBtBolt11InvoiceState::Canceled,
        }
    }
}

#[derive(uniffi::Enum, Debug, Deserialize, Serialize)]
pub enum BtChannelOrderErrorType {
    WrongOrderState,
    PeerNotReachable,
    ChannelRejectedByDestination,
    ChannelRejectedByLsp,
    BlocktankNotReady,
}

impl From<ExternalBtChannelOrderErrorType> for BtChannelOrderErrorType {
    fn from(other: ExternalBtChannelOrderErrorType) -> Self {
        match other {
            ExternalBtChannelOrderErrorType::WrongOrderState => {
                BtChannelOrderErrorType::WrongOrderState
            }
            ExternalBtChannelOrderErrorType::PeerNotReachable => {
                BtChannelOrderErrorType::PeerNotReachable
            }
            ExternalBtChannelOrderErrorType::ChannelRejectedByDestination => {
                BtChannelOrderErrorType::ChannelRejectedByDestination
            }
            ExternalBtChannelOrderErrorType::ChannelRejectedByLsp => {
                BtChannelOrderErrorType::ChannelRejectedByLsp
            }
            ExternalBtChannelOrderErrorType::BlocktankNotReady => {
                BtChannelOrderErrorType::BlocktankNotReady
            }
        }
    }
}

impl From<BtChannelOrderErrorType> for ExternalBtChannelOrderErrorType {
    fn from(other: BtChannelOrderErrorType) -> Self {
        match other {
            BtChannelOrderErrorType::WrongOrderState => {
                ExternalBtChannelOrderErrorType::WrongOrderState
            }
            BtChannelOrderErrorType::PeerNotReachable => {
                ExternalBtChannelOrderErrorType::PeerNotReachable
            }
            BtChannelOrderErrorType::ChannelRejectedByDestination => {
                ExternalBtChannelOrderErrorType::ChannelRejectedByDestination
            }
            BtChannelOrderErrorType::ChannelRejectedByLsp => {
                ExternalBtChannelOrderErrorType::ChannelRejectedByLsp
            }
            BtChannelOrderErrorType::BlocktankNotReady => {
                ExternalBtChannelOrderErrorType::BlocktankNotReady
            }
        }
    }
}

#[derive(uniffi::Enum, Deserialize, Serialize)]
pub enum BtOpenChannelState {
    Opening,
    Open,
    Closed,
}

impl From<ExternalBtOpenChannelState> for BtOpenChannelState {
    fn from(other: ExternalBtOpenChannelState) -> Self {
        match other {
            ExternalBtOpenChannelState::Opening => BtOpenChannelState::Opening,
            ExternalBtOpenChannelState::Open => BtOpenChannelState::Open,
            ExternalBtOpenChannelState::Closed => BtOpenChannelState::Closed,
        }
    }
}

impl From<BtOpenChannelState> for ExternalBtOpenChannelState {
    fn from(other: BtOpenChannelState) -> Self {
        match other {
            BtOpenChannelState::Opening => ExternalBtOpenChannelState::Opening,
            BtOpenChannelState::Open => ExternalBtOpenChannelState::Open,
            BtOpenChannelState::Closed => ExternalBtOpenChannelState::Closed,
        }
    }
}

#[derive(uniffi::Enum, Deserialize, Serialize)]
pub enum BtOrderState {
    Created,
    Expired,
    Open,
    Closed,
}

impl From<ExternalBtOrderState> for BtOrderState {
    fn from(other: ExternalBtOrderState) -> Self {
        match other {
            ExternalBtOrderState::Created => BtOrderState::Created,
            ExternalBtOrderState::Expired => BtOrderState::Expired,
            ExternalBtOrderState::Open => BtOrderState::Open,
            ExternalBtOrderState::Closed => BtOrderState::Closed,
        }
    }
}

impl From<BtOrderState> for ExternalBtOrderState {
    fn from(other: BtOrderState) -> Self {
        match other {
            BtOrderState::Created => ExternalBtOrderState::Created,
            BtOrderState::Expired => ExternalBtOrderState::Expired,
            BtOrderState::Open => ExternalBtOrderState::Open,
            BtOrderState::Closed => ExternalBtOrderState::Closed,
        }
    }
}

#[derive(uniffi::Enum, Deserialize, Serialize)]
pub enum BtOrderState2 {
    Created,
    Expired,
    Executed,
    Paid,
}

impl From<ExternalBtOrderState2> for BtOrderState2 {
    fn from(other: ExternalBtOrderState2) -> Self {
        match other {
            ExternalBtOrderState2::Created => BtOrderState2::Created,
            ExternalBtOrderState2::Expired => BtOrderState2::Expired,
            ExternalBtOrderState2::Executed => BtOrderState2::Executed,
            ExternalBtOrderState2::Paid => BtOrderState2::Paid,
        }
    }
}

impl From<BtOrderState2> for ExternalBtOrderState2 {
    fn from(other: BtOrderState2) -> Self {
        match other {
            BtOrderState2::Created => ExternalBtOrderState2::Created,
            BtOrderState2::Expired => ExternalBtOrderState2::Expired,
            BtOrderState2::Executed => ExternalBtOrderState2::Executed,
            BtOrderState2::Paid => ExternalBtOrderState2::Paid,
        }
    }
}

#[derive(uniffi::Enum, Deserialize, Serialize)]
pub enum BtPaymentState {
    Created,
    PartiallyPaid,
    Paid,
    Refunded,
    RefundAvailable,
}

impl From<ExternalBtPaymentState> for BtPaymentState {
    fn from(other: ExternalBtPaymentState) -> Self {
        match other {
            ExternalBtPaymentState::Created => BtPaymentState::Created,
            ExternalBtPaymentState::PartiallyPaid => BtPaymentState::PartiallyPaid,
            ExternalBtPaymentState::Paid => BtPaymentState::Paid,
            ExternalBtPaymentState::Refunded => BtPaymentState::Refunded,
            ExternalBtPaymentState::RefundAvailable => BtPaymentState::RefundAvailable,
        }
    }
}

impl From<BtPaymentState> for ExternalBtPaymentState {
    fn from(other: BtPaymentState) -> Self {
        match other {
            BtPaymentState::Created => ExternalBtPaymentState::Created,
            BtPaymentState::PartiallyPaid => ExternalBtPaymentState::PartiallyPaid,
            BtPaymentState::Paid => ExternalBtPaymentState::Paid,
            BtPaymentState::Refunded => ExternalBtPaymentState::Refunded,
            BtPaymentState::RefundAvailable => ExternalBtPaymentState::RefundAvailable,
        }
    }
}

#[derive(uniffi::Enum, Deserialize, Serialize)]
pub enum BtPaymentState2 {
    Created,
    Paid,
    Refunded,
    RefundAvailable,
    Canceled,
}

impl From<ExternalBtPaymentState2> for BtPaymentState2 {
    fn from(other: ExternalBtPaymentState2) -> Self {
        match other {
            ExternalBtPaymentState2::Created => BtPaymentState2::Created,
            ExternalBtPaymentState2::Paid => BtPaymentState2::Paid,
            ExternalBtPaymentState2::Refunded => BtPaymentState2::Refunded,
            ExternalBtPaymentState2::RefundAvailable => BtPaymentState2::RefundAvailable,
            ExternalBtPaymentState2::Canceled => BtPaymentState2::Canceled,
        }
    }
}

impl From<BtPaymentState2> for ExternalBtPaymentState2 {
    fn from(other: BtPaymentState2) -> Self {
        match other {
            BtPaymentState2::Created => ExternalBtPaymentState2::Created,
            BtPaymentState2::Paid => ExternalBtPaymentState2::Paid,
            BtPaymentState2::Refunded => ExternalBtPaymentState2::Refunded,
            BtPaymentState2::RefundAvailable => ExternalBtPaymentState2::RefundAvailable,
            BtPaymentState2::Canceled => ExternalBtPaymentState2::Canceled,
        }
    }
}

#[derive(uniffi::Enum, Deserialize, Serialize)]
pub enum CJitStateEnum {
    Created,
    Completed,
    Expired,
    Failed,
}

impl From<ExternalCJitStateEnum> for CJitStateEnum {
    fn from(other: ExternalCJitStateEnum) -> Self {
        match other {
            ExternalCJitStateEnum::Created => CJitStateEnum::Created,
            ExternalCJitStateEnum::Completed => CJitStateEnum::Completed,
            ExternalCJitStateEnum::Expired => CJitStateEnum::Expired,
            ExternalCJitStateEnum::Failed => CJitStateEnum::Failed,
        }
    }
}

impl From<CJitStateEnum> for ExternalCJitStateEnum {
    fn from(other: CJitStateEnum) -> Self {
        match other {
            CJitStateEnum::Created => ExternalCJitStateEnum::Created,
            CJitStateEnum::Completed => ExternalCJitStateEnum::Completed,
            CJitStateEnum::Expired => ExternalCJitStateEnum::Expired,
            CJitStateEnum::Failed => ExternalCJitStateEnum::Failed,
        }
    }
}

#[derive(uniffi::Enum, Deserialize, Serialize)]
pub enum ManualRefundStateEnum {
    Created,
    Approved,
    Rejected,
    Sent,
}

impl From<ExternalManualRefundStateEnum> for ManualRefundStateEnum {
    fn from(other: ExternalManualRefundStateEnum) -> Self {
        match other {
            ExternalManualRefundStateEnum::Created => ManualRefundStateEnum::Created,
            ExternalManualRefundStateEnum::Approved => ManualRefundStateEnum::Approved,
            ExternalManualRefundStateEnum::Rejected => ManualRefundStateEnum::Rejected,
            ExternalManualRefundStateEnum::Sent => ManualRefundStateEnum::Sent,
        }
    }
}

impl From<ManualRefundStateEnum> for ExternalManualRefundStateEnum {
    fn from(other: ManualRefundStateEnum) -> Self {
        match other {
            ManualRefundStateEnum::Created => ExternalManualRefundStateEnum::Created,
            ManualRefundStateEnum::Approved => ExternalManualRefundStateEnum::Approved,
            ManualRefundStateEnum::Rejected => ExternalManualRefundStateEnum::Rejected,
            ManualRefundStateEnum::Sent => ExternalManualRefundStateEnum::Sent,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct ILspNode {
    pub alias: String,
    pub pubkey: String,
    pub connection_strings: Vec<String>,
    pub readonly: Option<bool>,
}

impl From<ExternalILspNode> for ILspNode {
    fn from(other: ExternalILspNode) -> Self {
        Self {
            alias: other.alias,
            pubkey: other.pubkey,
            connection_strings: other.connection_strings,
            readonly: other.readonly,
        }
    }
}

impl From<ILspNode> for ExternalILspNode {
    fn from(other: ILspNode) -> Self {
        Self {
            alias: other.alias,
            pubkey: other.pubkey,
            connection_strings: other.connection_strings,
            readonly: other.readonly,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtInfoOptions {
    pub min_channel_size_sat: u64,
    pub max_channel_size_sat: u64,
    pub min_expiry_weeks: u32,
    pub max_expiry_weeks: u32,
    pub min_payment_confirmations: u32,
    pub min_high_risk_payment_confirmations: u32,
    pub max_0_conf_client_balance_sat: u64,
    pub max_client_balance_sat: u64,
}

impl From<ExternalIBtInfoOptions> for IBtInfoOptions {
    fn from(other: ExternalIBtInfoOptions) -> Self {
        Self {
            min_channel_size_sat: other.min_channel_size_sat,
            max_channel_size_sat: other.max_channel_size_sat,
            min_expiry_weeks: other.min_expiry_weeks,
            max_expiry_weeks: other.max_expiry_weeks,
            min_payment_confirmations: other.min_payment_confirmations,
            min_high_risk_payment_confirmations: other.min_high_risk_payment_confirmations,
            max_0_conf_client_balance_sat: other.max_0_conf_client_balance_sat,
            max_client_balance_sat: other.max_client_balance_sat,
        }
    }
}

impl From<IBtInfoOptions> for ExternalIBtInfoOptions {
    fn from(other: IBtInfoOptions) -> Self {
        Self {
            min_channel_size_sat: other.min_channel_size_sat,
            max_channel_size_sat: other.max_channel_size_sat,
            min_expiry_weeks: other.min_expiry_weeks,
            max_expiry_weeks: other.max_expiry_weeks,
            min_payment_confirmations: other.min_payment_confirmations,
            min_high_risk_payment_confirmations: other.min_high_risk_payment_confirmations,
            max_0_conf_client_balance_sat: other.max_0_conf_client_balance_sat,
            max_client_balance_sat: other.max_client_balance_sat,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IDiscount {
    pub code: String,
    pub absolute_sat: u64,
    pub relative: f64,
    pub overall_sat: u64,
}

impl From<ExternalIDiscount> for IDiscount {
    fn from(other: ExternalIDiscount) -> Self {
        Self {
            code: other.code,
            absolute_sat: other.absolute_sat,
            relative: other.relative,
            overall_sat: other.overall_sat,
        }
    }
}

impl From<IDiscount> for ExternalIDiscount {
    fn from(other: IDiscount) -> Self {
        Self {
            code: other.code,
            absolute_sat: other.absolute_sat,
            relative: other.relative,
            overall_sat: other.overall_sat,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtBolt11Invoice {
    pub request: String,
    pub state: BtBolt11InvoiceState,
    pub expires_at: String,
    pub updated_at: String,
}

impl From<ExternalIBtBolt11Invoice> for IBtBolt11Invoice {
    fn from(other: ExternalIBtBolt11Invoice) -> Self {
        Self {
            request: other.request,
            state: other.state.into(),
            expires_at: other.expires_at,
            updated_at: other.updated_at,
        }
    }
}

impl From<IBtBolt11Invoice> for ExternalIBtBolt11Invoice {
    fn from(other: IBtBolt11Invoice) -> Self {
        Self {
            request: other.request,
            state: other.state.into(),
            expires_at: other.expires_at,
            updated_at: other.updated_at,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtChannelClose {
    pub tx_id: String,
    pub close_type: String, // 'cooperative' | 'force' | 'breach'
    pub initiator: String,  // 'lsp' | 'client'
    pub registered_at: String,
}

impl From<ExternalIBtChannelClose> for IBtChannelClose {
    fn from(other: ExternalIBtChannelClose) -> Self {
        Self {
            tx_id: other.tx_id,
            close_type: other.close_type,
            initiator: other.initiator,
            registered_at: other.registered_at,
        }
    }
}

impl From<IBtChannelClose> for ExternalIBtChannelClose {
    fn from(other: IBtChannelClose) -> Self {
        Self {
            tx_id: other.tx_id,
            close_type: other.close_type,
            initiator: other.initiator,
            registered_at: other.registered_at,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct FundingTx {
    pub id: String,
    pub vout: u64,
}

impl From<ExternalFundingTx> for FundingTx {
    fn from(other: ExternalFundingTx) -> Self {
        Self {
            id: other.id,
            vout: other.vout,
        }
    }
}

impl From<FundingTx> for ExternalFundingTx {
    fn from(other: FundingTx) -> Self {
        Self {
            id: other.id,
            vout: other.vout,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtChannel {
    pub state: BtOpenChannelState,
    pub lsp_node_pubkey: String,
    pub client_node_pubkey: String,
    pub announce_channel: bool,
    pub funding_tx: FundingTx,
    pub closing_tx_id: Option<String>,
    pub close: Option<IBtChannelClose>,
    pub short_channel_id: Option<String>,
}

impl From<ExternalIBtChannel> for IBtChannel {
    fn from(other: ExternalIBtChannel) -> Self {
        Self {
            state: other.state.into(),
            lsp_node_pubkey: other.lsp_node_pubkey,
            client_node_pubkey: other.client_node_pubkey,
            announce_channel: other.announce_channel,
            funding_tx: other.funding_tx.into(),
            closing_tx_id: other.closing_tx_id,
            close: other.close.map(|close| close.into()),
            short_channel_id: other.short_channel_id,
        }
    }
}

impl From<IBtChannel> for ExternalIBtChannel {
    fn from(other: IBtChannel) -> Self {
        Self {
            state: other.state.into(),
            lsp_node_pubkey: other.lsp_node_pubkey,
            client_node_pubkey: other.client_node_pubkey,
            announce_channel: other.announce_channel,
            funding_tx: other.funding_tx.into(),
            closing_tx_id: other.closing_tx_id,
            close: other.close.map(|close| close.into()),
            short_channel_id: other.short_channel_id,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IManualRefund {
    pub amount_sat: u64,
    pub target: String,
    pub state: ManualRefundStateEnum,
    pub created_by_name: String,
    pub voted_by_name: Option<String>,
    pub reason: Option<String>,
    pub target_type: String, // 'lightning' | 'onchain'
}

impl From<ExternalIManualRefund> for IManualRefund {
    fn from(other: ExternalIManualRefund) -> Self {
        Self {
            amount_sat: other.amount_sat,
            target: other.target,
            state: other.state.into(),
            created_by_name: other.created_by_name,
            voted_by_name: other.voted_by_name,
            reason: other.reason,
            target_type: other.target_type,
        }
    }
}

impl From<IManualRefund> for ExternalIManualRefund {
    fn from(other: IManualRefund) -> Self {
        Self {
            amount_sat: other.amount_sat,
            target: other.target,
            state: other.state.into(),
            created_by_name: other.created_by_name,
            voted_by_name: other.voted_by_name,
            reason: other.reason,
            target_type: other.target_type,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtOnchainTransaction {
    pub amount_sat: u64,
    pub tx_id: String,
    pub vout: u32,
    pub block_height: Option<u32>,
    pub block_confirmation_count: u32,
    pub fee_rate_sat_per_vbyte: f64,
    pub confirmed: bool,
    pub suspicious_0_conf_reason: String,
}

impl From<ExternalIBtOnchainTransaction> for IBtOnchainTransaction {
    fn from(other: ExternalIBtOnchainTransaction) -> Self {
        Self {
            amount_sat: other.amount_sat,
            tx_id: other.tx_id,
            vout: other.vout,
            block_height: other.block_height,
            block_confirmation_count: other.block_confirmation_count,
            fee_rate_sat_per_vbyte: other.fee_rate_sat_per_vbyte,
            confirmed: other.confirmed,
            suspicious_0_conf_reason: other.suspicious_0_conf_reason,
        }
    }
}

impl From<IBtOnchainTransaction> for ExternalIBtOnchainTransaction {
    fn from(other: IBtOnchainTransaction) -> Self {
        Self {
            amount_sat: other.amount_sat,
            tx_id: other.tx_id,
            vout: other.vout,
            block_height: other.block_height,
            block_confirmation_count: other.block_confirmation_count,
            fee_rate_sat_per_vbyte: other.fee_rate_sat_per_vbyte,
            confirmed: other.confirmed,
            suspicious_0_conf_reason: other.suspicious_0_conf_reason,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtOnchainTransactions {
    pub address: String,
    pub confirmed_sat: u64,
    pub required_confirmations: u32,
    pub transactions: Vec<IBtOnchainTransaction>,
}

impl From<ExternalIBtOnchainTransactions> for IBtOnchainTransactions {
    fn from(other: ExternalIBtOnchainTransactions) -> Self {
        Self {
            address: other.address,
            confirmed_sat: other.confirmed_sat,
            required_confirmations: other.required_confirmations,
            transactions: other.transactions.into_iter().map(|tx| tx.into()).collect(),
        }
    }
}

impl From<IBtOnchainTransactions> for ExternalIBtOnchainTransactions {
    fn from(other: IBtOnchainTransactions) -> Self {
        Self {
            address: other.address,
            confirmed_sat: other.confirmed_sat,
            required_confirmations: other.required_confirmations,
            transactions: other.transactions.into_iter().map(|tx| tx.into()).collect(),
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtPayment {
    pub state: BtPaymentState,
    pub state2: Option<BtPaymentState2>,
    pub paid_sat: u64,
    pub bolt11_invoice: Option<IBtBolt11Invoice>,
    pub onchain: Option<IBtOnchainTransactions>,
    pub is_manually_paid: Option<bool>,
    pub manual_refunds: Option<Vec<IManualRefund>>,
}

impl From<ExternalIBtPayment> for IBtPayment {
    fn from(other: ExternalIBtPayment) -> Self {
        Self {
            state: other.state.into(),
            state2: other.state2.map(|s| s.into()),
            paid_sat: other.paid_sat,
            bolt11_invoice: other.bolt11_invoice.map(|b| b.into()),
            onchain: other.onchain.map(|o| o.into()),
            is_manually_paid: other.is_manually_paid,
            manual_refunds: other
                .manual_refunds
                .map(|refunds| refunds.into_iter().map(|refund| refund.into()).collect()),
        }
    }
}

impl From<IBtPayment> for ExternalIBtPayment {
    fn from(other: IBtPayment) -> Self {
        Self {
            state: other.state.into(),
            state2: other.state2.map(|s| s.into()),
            paid_sat: other.paid_sat,
            bolt11_invoice: other.bolt11_invoice.map(|b| b.into()),
            onchain: other.onchain.map(|o| o.into()),
            is_manually_paid: other.is_manually_paid,
            manual_refunds: other
                .manual_refunds
                .map(|refunds| refunds.into_iter().map(|refund| refund.into()).collect()),
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBt0ConfMinTxFeeWindow {
    pub sat_per_vbyte: f64,
    pub validity_ends_at: String,
}

impl From<ExternalIBt0ConfMinTxFeeWindow> for IBt0ConfMinTxFeeWindow {
    fn from(other: ExternalIBt0ConfMinTxFeeWindow) -> Self {
        Self {
            sat_per_vbyte: other.sat_per_vbyte,
            validity_ends_at: other.validity_ends_at,
        }
    }
}

impl From<IBt0ConfMinTxFeeWindow> for ExternalIBt0ConfMinTxFeeWindow {
    fn from(other: IBt0ConfMinTxFeeWindow) -> Self {
        Self {
            sat_per_vbyte: other.sat_per_vbyte,
            validity_ends_at: other.validity_ends_at,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtOrder {
    pub id: String,
    pub state: BtOrderState,
    pub state2: Option<BtOrderState2>,
    pub fee_sat: u64,
    pub network_fee_sat: u64,
    pub service_fee_sat: u64,
    pub lsp_balance_sat: u64,
    pub client_balance_sat: u64,
    pub zero_conf: bool,
    pub zero_reserve: bool,
    pub client_node_id: Option<String>,
    pub channel_expiry_weeks: u32,
    pub channel_expires_at: String,
    pub order_expires_at: String,
    pub channel: Option<IBtChannel>,
    pub lsp_node: Option<ILspNode>,
    pub lnurl: Option<String>,
    pub payment: Option<IBtPayment>,
    pub coupon_code: Option<String>,
    pub source: Option<String>,
    pub discount: Option<IDiscount>,
    pub updated_at: String,
    pub created_at: String,
}

impl From<ExternalIBtOrder> for IBtOrder {
    fn from(other: ExternalIBtOrder) -> Self {
        Self {
            id: other.id,
            state: other.state.into(),
            state2: other.state2.map(|s| s.into()),
            fee_sat: other.fee_sat,
            network_fee_sat: other.network_fee_sat,
            service_fee_sat: other.service_fee_sat,
            lsp_balance_sat: other.lsp_balance_sat,
            client_balance_sat: other.client_balance_sat,
            zero_conf: other.zero_conf,
            zero_reserve: other.zero_reserve,
            client_node_id: other.client_node_id,
            channel_expiry_weeks: other.channel_expiry_weeks,
            channel_expires_at: other.channel_expires_at,
            order_expires_at: other.order_expires_at,
            channel: other.channel.map(|c| c.into()),
            lsp_node: other.lsp_node.map(|l| l.into()),
            lnurl: other.lnurl,
            payment: other.payment.map(|p| p.into()),
            coupon_code: other.coupon_code,
            source: other.source,
            discount: other.discount.map(|d| d.into()),
            updated_at: other.updated_at,
            created_at: other.created_at,
        }
    }
}

impl From<IBtOrder> for ExternalIBtOrder {
    fn from(other: IBtOrder) -> Self {
        Self {
            id: other.id,
            state: other.state.into(),
            state2: other.state2.map(|s| s.into()),
            fee_sat: other.fee_sat,
            network_fee_sat: other.network_fee_sat,
            service_fee_sat: other.service_fee_sat,
            lsp_balance_sat: other.lsp_balance_sat,
            client_balance_sat: other.client_balance_sat,
            zero_conf: other.zero_conf,
            zero_reserve: other.zero_reserve,
            client_node_id: other.client_node_id,
            channel_expiry_weeks: other.channel_expiry_weeks,
            channel_expires_at: other.channel_expires_at,
            order_expires_at: other.order_expires_at,
            channel: other.channel.map(|c| c.into()),
            lsp_node: other.lsp_node.map(|l| l.into()),
            lnurl: other.lnurl,
            payment: other.payment.map(|p| p.into()),
            coupon_code: other.coupon_code,
            source: other.source,
            discount: other.discount.map(|d| d.into()),
            updated_at: other.updated_at,
            created_at: other.created_at,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct ICJitEntry {
    pub id: String,
    pub state: CJitStateEnum,
    pub fee_sat: u64,
    pub network_fee_sat: u64,
    pub service_fee_sat: u64,
    pub channel_size_sat: u64,
    pub channel_expiry_weeks: u32,
    pub channel_open_error: Option<String>,
    pub node_id: String,
    pub invoice: IBtBolt11Invoice,
    pub channel: Option<IBtChannel>,
    pub lsp_node: ILspNode,
    pub coupon_code: String,
    pub source: Option<String>,
    pub discount: Option<IDiscount>,
    pub expires_at: String,
    pub updated_at: String,
    pub created_at: String,
}

impl From<ExternalICJitEntry> for ICJitEntry {
    fn from(other: ExternalICJitEntry) -> Self {
        Self {
            id: other.id,
            state: other.state.into(),
            fee_sat: other.fee_sat,
            network_fee_sat: other.network_fee_sat,
            service_fee_sat: other.service_fee_sat,
            channel_size_sat: other.channel_size_sat,
            channel_expiry_weeks: other.channel_expiry_weeks,
            channel_open_error: other.channel_open_error,
            node_id: other.node_id,
            invoice: other.invoice.into(),
            channel: other.channel.map(|c| c.into()),
            lsp_node: other.lsp_node.into(),
            coupon_code: other.coupon_code,
            source: other.source,
            discount: other.discount.map(|d| d.into()),
            expires_at: other.expires_at,
            updated_at: other.updated_at,
            created_at: other.created_at,
        }
    }
}

impl From<ICJitEntry> for ExternalICJitEntry {
    fn from(other: ICJitEntry) -> Self {
        Self {
            id: other.id,
            state: other.state.into(),
            fee_sat: other.fee_sat,
            network_fee_sat: other.network_fee_sat,
            service_fee_sat: other.service_fee_sat,
            channel_size_sat: other.channel_size_sat,
            channel_expiry_weeks: other.channel_expiry_weeks,
            channel_open_error: other.channel_open_error,
            node_id: other.node_id,
            invoice: other.invoice.into(),
            channel: other.channel.map(|c| c.into()),
            lsp_node: other.lsp_node.into(),
            coupon_code: other.coupon_code,
            source: other.source,
            discount: other.discount.map(|d| d.into()),
            expires_at: other.expires_at,
            updated_at: other.updated_at,
            created_at: other.created_at,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtInfoVersions {
    pub http: String,
    pub btc: String,
    pub ln2: String,
}

impl From<ExternalIBtInfoVersions> for IBtInfoVersions {
    fn from(other: ExternalIBtInfoVersions) -> Self {
        Self {
            http: other.http,
            btc: other.btc,
            ln2: other.ln2,
        }
    }
}

impl From<IBtInfoVersions> for ExternalIBtInfoVersions {
    fn from(other: IBtInfoVersions) -> Self {
        Self {
            http: other.http,
            btc: other.btc,
            ln2: other.ln2,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtInfo {
    pub version: u32,
    pub nodes: Vec<ILspNode>,
    pub options: IBtInfoOptions,
    pub versions: IBtInfoVersions,
    pub onchain: IBtInfoOnchain,
}

impl From<ExternalIBtInfo> for IBtInfo {
    fn from(other: ExternalIBtInfo) -> Self {
        Self {
            version: other.version,
            nodes: other.nodes.into_iter().map(|node| node.into()).collect(),
            options: other.options.into(),
            versions: other.versions.into(),
            onchain: other.onchain.into(),
        }
    }
}

impl From<IBtInfo> for ExternalIBtInfo {
    fn from(other: IBtInfo) -> Self {
        Self {
            version: other.version,
            nodes: other.nodes.into_iter().map(|node| node.into()).collect(),
            options: other.options.into(),
            versions: other.versions.into(),
            onchain: other.onchain.into(),
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct FeeRates {
    pub fast: u32,
    pub mid: u32,
    pub slow: u32,
}

impl From<ExternalFeeRates> for FeeRates {
    fn from(other: ExternalFeeRates) -> Self {
        Self {
            fast: other.fast,
            mid: other.mid,
            slow: other.slow,
        }
    }
}

impl From<FeeRates> for ExternalFeeRates {
    fn from(other: FeeRates) -> Self {
        Self {
            fast: other.fast,
            mid: other.mid,
            slow: other.slow,
        }
    }
}
#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtInfoOnchain {
    pub network: BitcoinNetworkEnum,
    pub fee_rates: FeeRates,
}

impl From<ExternalIBtInfoOnchain> for IBtInfoOnchain {
    fn from(other: ExternalIBtInfoOnchain) -> Self {
        Self {
            network: other.network.into(),
            fee_rates: other.fee_rates.into(),
        }
    }
}

impl From<IBtInfoOnchain> for ExternalIBtInfoOnchain {
    fn from(other: IBtInfoOnchain) -> Self {
        Self {
            network: other.network.into(),
            fee_rates: other.fee_rates.into(),
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtEstimateFeeResponse {
    pub fee_sat: u64,
    pub min_0_conf_tx_fee: IBt0ConfMinTxFeeWindow,
}

impl From<ExternalIBtEstimateFeeResponse> for IBtEstimateFeeResponse {
    fn from(other: ExternalIBtEstimateFeeResponse) -> Self {
        Self {
            fee_sat: other.fee_sat,
            min_0_conf_tx_fee: other.min_0_conf_tx_fee.into(),
        }
    }
}

impl From<IBtEstimateFeeResponse> for ExternalIBtEstimateFeeResponse {
    fn from(other: IBtEstimateFeeResponse) -> Self {
        Self {
            fee_sat: other.fee_sat,
            min_0_conf_tx_fee: other.min_0_conf_tx_fee.into(),
        }
    }
}
#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IBtEstimateFeeResponse2 {
    pub fee_sat: u64,
    pub network_fee_sat: u64,
    pub service_fee_sat: u64,
    pub min_0_conf_tx_fee: IBt0ConfMinTxFeeWindow,
}

impl From<ExternalIBtEstimateFeeResponse2> for IBtEstimateFeeResponse2 {
    fn from(other: ExternalIBtEstimateFeeResponse2) -> Self {
        Self {
            fee_sat: other.fee_sat,
            network_fee_sat: other.network_fee_sat,
            service_fee_sat: other.service_fee_sat,
            min_0_conf_tx_fee: other.min_0_conf_tx_fee.into(),
        }
    }
}

impl From<IBtEstimateFeeResponse2> for ExternalIBtEstimateFeeResponse2 {
    fn from(other: IBtEstimateFeeResponse2) -> Self {
        Self {
            fee_sat: other.fee_sat,
            network_fee_sat: other.network_fee_sat,
            service_fee_sat: other.service_fee_sat,
            min_0_conf_tx_fee: other.min_0_conf_tx_fee.into(),
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct CreateOrderOptions {
    pub client_balance_sat: u64,
    pub lsp_node_id: Option<String>,
    pub coupon_code: String,
    pub source: Option<String>,
    pub discount_code: Option<String>,
    pub zero_conf: bool,
    pub zero_conf_payment: Option<bool>,
    pub zero_reserve: bool,
    pub client_node_id: Option<String>,
    pub signature: Option<String>,
    pub timestamp: Option<String>,
    pub refund_onchain_address: Option<String>,
    pub announce_channel: bool,
}

impl From<ExternalCreateOrderOptions> for CreateOrderOptions {
    fn from(other: ExternalCreateOrderOptions) -> Self {
        Self {
            client_balance_sat: other.client_balance_sat,
            lsp_node_id: other.lsp_node_id,
            coupon_code: other.coupon_code,
            source: other.source,
            discount_code: other.discount_code,
            zero_conf: other.zero_conf,
            zero_conf_payment: other.zero_conf_payment,
            zero_reserve: other.zero_reserve,
            client_node_id: other.client_node_id,
            signature: other.signature,
            timestamp: other.timestamp,
            refund_onchain_address: other.refund_onchain_address,
            announce_channel: other.announce_channel,
        }
    }
}

impl From<CreateOrderOptions> for ExternalCreateOrderOptions {
    fn from(other: CreateOrderOptions) -> Self {
        Self {
            client_balance_sat: other.client_balance_sat,
            lsp_node_id: other.lsp_node_id,
            coupon_code: other.coupon_code,
            source: other.source,
            discount_code: other.discount_code,
            zero_conf: other.zero_conf,
            zero_conf_payment: other.zero_conf_payment,
            zero_reserve: other.zero_reserve,
            client_node_id: other.client_node_id,
            signature: other.signature,
            timestamp: other.timestamp,
            refund_onchain_address: other.refund_onchain_address,
            announce_channel: other.announce_channel,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct CreateCjitOptions {
    pub source: Option<String>,
    pub discount_code: Option<String>,
}

impl From<ExternalCreateCjitOptions> for CreateCjitOptions {
    fn from(other: ExternalCreateCjitOptions) -> Self {
        Self {
            source: other.source,
            discount_code: other.discount_code,
        }
    }
}

impl From<CreateCjitOptions> for ExternalCreateCjitOptions {
    fn from(other: CreateCjitOptions) -> Self {
        Self {
            source: other.source,
            discount_code: other.discount_code,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IGiftBtcAddress {
    pub id: String,
    pub address: String,
    pub transactions: Vec<String>, // Simplified from serde_json::Value
    pub all_transactions: Vec<String>, // Simplified from serde_json::Value
    pub is_blacklisted: Option<bool>,
    pub watch_until: Option<String>,
    pub watch_for_block_confirmations: Option<u32>,
    pub updated_at: Option<String>,
    pub created_at: Option<String>,
}

impl From<ExternalIGiftBtcAddress> for IGiftBtcAddress {
    fn from(other: ExternalIGiftBtcAddress) -> Self {
        Self {
            id: other.id,
            address: other.address,
            transactions: other.transactions.iter().map(|_| "".to_string()).collect(), // Simplified
            all_transactions: other
                .all_transactions
                .iter()
                .map(|_| "".to_string())
                .collect(), // Simplified
            is_blacklisted: other.is_blacklisted,
            watch_until: other.watch_until,
            watch_for_block_confirmations: other.watch_for_block_confirmations,
            updated_at: other.updated_at,
            created_at: other.created_at,
        }
    }
}

impl From<IGiftBtcAddress> for ExternalIGiftBtcAddress {
    fn from(other: IGiftBtcAddress) -> Self {
        Self {
            id: other.id,
            address: other.address,
            transactions: vec![],     // Simplified
            all_transactions: vec![], // Simplified
            is_blacklisted: other.is_blacklisted,
            watch_until: other.watch_until,
            watch_for_block_confirmations: other.watch_for_block_confirmations,
            updated_at: other.updated_at,
            created_at: other.created_at,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IGiftBolt11Invoice {
    pub id: String,
    pub request: String,
    pub state: String, // String instead of enum for gift types
    pub is_hodl_invoice: Option<bool>,
    pub payment_hash: Option<String>,
    pub amount_sat: Option<u64>,
    pub amount_msat: Option<String>,
    pub internal_node_pubkey: Option<String>,
    pub updated_at: Option<String>,
    pub created_at: Option<String>,
    pub expires_at: Option<String>,
}

impl From<ExternalIGiftBolt11Invoice> for IGiftBolt11Invoice {
    fn from(other: ExternalIGiftBolt11Invoice) -> Self {
        Self {
            id: other.id,
            request: other.request,
            state: other.state,
            is_hodl_invoice: other.is_hodl_invoice,
            payment_hash: other.payment_hash,
            amount_sat: other.amount_sat,
            amount_msat: other.amount_msat,
            internal_node_pubkey: other.internal_node_pubkey,
            updated_at: other.updated_at,
            created_at: other.created_at,
            expires_at: other.expires_at,
        }
    }
}

impl From<IGiftBolt11Invoice> for ExternalIGiftBolt11Invoice {
    fn from(other: IGiftBolt11Invoice) -> Self {
        Self {
            id: other.id,
            request: other.request,
            state: other.state,
            is_hodl_invoice: other.is_hodl_invoice,
            payment_hash: other.payment_hash,
            amount_sat: other.amount_sat,
            amount_msat: other.amount_msat,
            internal_node_pubkey: other.internal_node_pubkey,
            updated_at: other.updated_at,
            created_at: other.created_at,
            expires_at: other.expires_at,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IGiftLspNode {
    pub alias: String,
    pub pubkey: String,
    pub connection_strings: Vec<String>,
}

impl From<ExternalIGiftLspNode> for IGiftLspNode {
    fn from(other: ExternalIGiftLspNode) -> Self {
        Self {
            alias: other.alias,
            pubkey: other.pubkey,
            connection_strings: other.connection_strings,
        }
    }
}

impl From<IGiftLspNode> for ExternalIGiftLspNode {
    fn from(other: IGiftLspNode) -> Self {
        Self {
            alias: other.alias,
            pubkey: other.pubkey,
            connection_strings: other.connection_strings,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IGiftPayment {
    pub id: String,
    pub state: String, // String instead of enum
    pub old_state: Option<String>,
    pub onchain_state: Option<String>,
    pub ln_state: Option<String>,
    pub paid_onchain_sat: Option<u64>,
    pub paid_ln_sat: Option<u64>,
    pub paid_sat: Option<u64>,
    pub is_overpaid: Option<bool>,
    pub is_refunded: Option<bool>,
    pub overpaid_amount_sat: Option<u64>,
    pub required_onchain_confirmations: Option<u32>,
    pub settlement_state: Option<String>,
    pub expected_amount_sat: Option<u64>,
    pub is_manually_paid: Option<bool>,
    pub btc_address: Option<IGiftBtcAddress>,
    pub btc_address_id: Option<String>,
    pub bolt11_invoice: Option<IGiftBolt11Invoice>,
    pub bolt11_invoice_id: Option<String>,
    pub manual_refunds: Vec<String>, // Simplified from serde_json::Value
}

impl From<ExternalIGiftPayment> for IGiftPayment {
    fn from(other: ExternalIGiftPayment) -> Self {
        Self {
            id: other.id,
            state: other.state,
            old_state: other.old_state,
            onchain_state: other.onchain_state,
            ln_state: other.ln_state,
            paid_onchain_sat: other.paid_onchain_sat,
            paid_ln_sat: other.paid_ln_sat,
            paid_sat: other.paid_sat,
            is_overpaid: other.is_overpaid,
            is_refunded: other.is_refunded,
            overpaid_amount_sat: other.overpaid_amount_sat,
            required_onchain_confirmations: other.required_onchain_confirmations,
            settlement_state: other.settlement_state,
            expected_amount_sat: other.expected_amount_sat,
            is_manually_paid: other.is_manually_paid,
            btc_address: other.btc_address.map(|b| b.into()),
            btc_address_id: other.btc_address_id,
            bolt11_invoice: other.bolt11_invoice.map(|b| b.into()),
            bolt11_invoice_id: other.bolt11_invoice_id,
            manual_refunds: other
                .manual_refunds
                .iter()
                .map(|_| "".to_string())
                .collect(), // Simplified
        }
    }
}

impl From<IGiftPayment> for ExternalIGiftPayment {
    fn from(other: IGiftPayment) -> Self {
        Self {
            id: other.id,
            state: other.state,
            old_state: other.old_state,
            onchain_state: other.onchain_state,
            ln_state: other.ln_state,
            paid_onchain_sat: other.paid_onchain_sat,
            paid_ln_sat: other.paid_ln_sat,
            paid_sat: other.paid_sat,
            is_overpaid: other.is_overpaid,
            is_refunded: other.is_refunded,
            overpaid_amount_sat: other.overpaid_amount_sat,
            required_onchain_confirmations: other.required_onchain_confirmations,
            settlement_state: other.settlement_state,
            expected_amount_sat: other.expected_amount_sat,
            is_manually_paid: other.is_manually_paid,
            btc_address: other.btc_address.map(|b| b.into()),
            btc_address_id: other.btc_address_id,
            bolt11_invoice: other.bolt11_invoice.map(|b| b.into()),
            bolt11_invoice_id: other.bolt11_invoice_id,
            manual_refunds: vec![], // Simplified
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IGiftOrder {
    pub id: String,
    pub state: String, // String instead of enum
    pub old_state: Option<String>,
    pub is_channel_expired: Option<bool>,
    pub is_order_expired: Option<bool>,
    pub lsp_balance_sat: Option<u64>,
    pub client_balance_sat: Option<u64>,
    pub channel_expiry_weeks: Option<u32>,
    pub zero_conf: Option<bool>,
    pub zero_reserve: Option<bool>,
    pub announced: Option<bool>,
    pub client_node_id: Option<String>,
    pub channel_expires_at: Option<String>,
    pub order_expires_at: Option<String>,
    pub fee_sat: Option<u64>,
    pub network_fee_sat: Option<u64>,
    pub service_fee_sat: Option<u64>,
    pub payment: Option<IGiftPayment>,
    pub lsp_node: Option<IGiftLspNode>,
    pub updated_at: Option<String>,
    pub created_at: Option<String>,
    pub node_id_verified: Option<bool>,
}

impl From<ExternalIGiftOrder> for IGiftOrder {
    fn from(other: ExternalIGiftOrder) -> Self {
        Self {
            id: other.id,
            state: other.state,
            old_state: other.old_state,
            is_channel_expired: other.is_channel_expired,
            is_order_expired: other.is_order_expired,
            lsp_balance_sat: other.lsp_balance_sat,
            client_balance_sat: other.client_balance_sat,
            channel_expiry_weeks: other.channel_expiry_weeks,
            zero_conf: other.zero_conf,
            zero_reserve: other.zero_reserve,
            announced: other.announced,
            client_node_id: other.client_node_id,
            channel_expires_at: other.channel_expires_at,
            order_expires_at: other.order_expires_at,
            fee_sat: other.fee_sat,
            network_fee_sat: other.network_fee_sat,
            service_fee_sat: other.service_fee_sat,
            payment: other.payment.map(|p| p.into()),
            lsp_node: other.lsp_node.map(|l| l.into()),
            updated_at: other.updated_at,
            created_at: other.created_at,
            node_id_verified: other.node_id_verified,
        }
    }
}

impl From<IGiftOrder> for ExternalIGiftOrder {
    fn from(other: IGiftOrder) -> Self {
        Self {
            id: other.id,
            state: other.state,
            old_state: other.old_state,
            is_channel_expired: other.is_channel_expired,
            is_order_expired: other.is_order_expired,
            lsp_balance_sat: other.lsp_balance_sat,
            client_balance_sat: other.client_balance_sat,
            channel_expiry_weeks: other.channel_expiry_weeks,
            zero_conf: other.zero_conf,
            zero_reserve: other.zero_reserve,
            announced: other.announced,
            client_node_id: other.client_node_id,
            channel_expires_at: other.channel_expires_at,
            order_expires_at: other.order_expires_at,
            fee_sat: other.fee_sat,
            network_fee_sat: other.network_fee_sat,
            service_fee_sat: other.service_fee_sat,
            payment: other.payment.map(|p| p.into()),
            lsp_node: other.lsp_node.map(|l| l.into()),
            updated_at: other.updated_at,
            created_at: other.created_at,
            node_id_verified: other.node_id_verified,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IGiftCode {
    pub id: String,
    pub code: String,
    pub created_at: String,
    pub updated_at: String,
    pub expires_at: String,
    pub gift_sat: Option<u64>,
    pub scope: Option<String>,
    pub max_count: Option<u32>,
}

impl From<ExternalIGiftCode> for IGiftCode {
    fn from(other: ExternalIGiftCode) -> Self {
        Self {
            id: other.id,
            code: other.code,
            created_at: other.created_at.unwrap_or_default(),
            updated_at: other.updated_at.unwrap_or_default(),
            expires_at: other.expires_at.unwrap_or_default(),
            gift_sat: other.gift_sat,
            scope: other.scope,
            max_count: other.max_count,
        }
    }
}

impl From<IGiftCode> for ExternalIGiftCode {
    fn from(other: IGiftCode) -> Self {
        Self {
            id: other.id,
            code: other.code,
            created_at: Some(other.created_at),
            updated_at: Some(other.updated_at),
            expires_at: Some(other.expires_at),
            gift_sat: other.gift_sat,
            scope: other.scope,
            max_count: other.max_count,
        }
    }
}

#[derive(uniffi::Record, Deserialize, Serialize)]
pub struct IGift {
    pub id: String,
    pub node_id: String,
    pub order_id: Option<String>,
    pub order: Option<IGiftOrder>,
    pub bolt11_payment_id: Option<String>,
    pub bolt11_payment: Option<IGiftPayment>,
    pub applied_gift_code_id: Option<String>,
    pub applied_gift_code: Option<IGiftCode>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl From<ExternalIGift> for IGift {
    fn from(other: ExternalIGift) -> Self {
        Self {
            id: other.id,
            node_id: other.node_id,
            order_id: other.order_id,
            order: other.order.map(|o| o.into()),
            bolt11_payment_id: other.bolt11_payment_id,
            bolt11_payment: other.bolt11_payment.map(|p| p.into()),
            applied_gift_code_id: other.applied_gift_code_id,
            applied_gift_code: other.applied_gift_code.map(|c| c.into()),
            created_at: other.created_at,
            updated_at: other.updated_at,
        }
    }
}

impl From<IGift> for ExternalIGift {
    fn from(other: IGift) -> Self {
        Self {
            id: other.id,
            node_id: other.node_id,
            order_id: other.order_id,
            order: other.order.map(|o| o.into()),
            bolt11_payment_id: other.bolt11_payment_id,
            bolt11_payment: other.bolt11_payment.map(|p| p.into()),
            applied_gift_code_id: other.applied_gift_code_id,
            applied_gift_code: other.applied_gift_code.map(|c| c.into()),
            created_at: other.created_at,
            updated_at: other.updated_at,
        }
    }
}
