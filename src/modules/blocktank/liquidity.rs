use serde::{Deserialize, Serialize};

// EUR threshold constants for channel liquidity calculations
const THRESHOLD_1_EUR: u64 = 225;
const THRESHOLD_2_EUR: u64 = 495;
const DEFAULT_LSP_TARGET_EUR: u64 = 450;

// LDK/LSP specific constants
const MAX_CHANNEL_SIZE_BUFFER_PERCENT: f64 = 0.98; // 2% buffer for fee fluctuations
const LDK_RESERVE_PERCENT: f64 = 0.025; // 2.5% reserve requirement

#[derive(uniffi::Record, Deserialize, Serialize, Clone, Debug)]
pub struct ChannelLiquidityOptions {
    pub default_lsp_balance_sat: u64,
    pub min_lsp_balance_sat: u64,
    pub max_lsp_balance_sat: u64,
    pub max_client_balance_sat: u64,
}

#[derive(uniffi::Record, Deserialize, Serialize, Clone, Debug)]
pub struct ChannelLiquidityParams {
    pub client_balance_sat: u64,
    pub existing_channels_total_sat: u64,
    pub min_channel_size_sat: u64,
    pub max_channel_size_sat: u64,
    pub sats_per_eur: u64,
}

#[derive(uniffi::Record, Deserialize, Serialize, Clone, Debug)]
pub struct DefaultLspBalanceParams {
    pub client_balance_sat: u64,
    pub max_channel_size_sat: u64,
    pub sats_per_eur: u64,
}

/// Calculates channel liquidity options including default, min, and max LSP balance.
/// Used for normal channel opening UI with existing channel deduction and 2% buffer.
pub fn calculate_channel_liquidity_options(params: ChannelLiquidityParams) -> ChannelLiquidityOptions {
    let threshold_1_sat = THRESHOLD_1_EUR * params.sats_per_eur;
    let threshold_2_sat = THRESHOLD_2_EUR * params.sats_per_eur;
    let default_lsp_target_sat = DEFAULT_LSP_TARGET_EUR * params.sats_per_eur;

    // Apply 2% buffer to max channel size (LSP limits fluctuate with network fees)
    let max_channel_size_buffered = (params.max_channel_size_sat as f64 * MAX_CHANNEL_SIZE_BUFFER_PERCENT) as u64;

    // Subtract existing channels from max (users have a total liquidity cap)
    let max_channel_size = max_channel_size_buffered
        .saturating_sub(params.existing_channels_total_sat);

    let min_lsp_balance = calc_min_lsp_balance(
        params.client_balance_sat,
        params.min_channel_size_sat,
    );

    let max_lsp_balance = max_channel_size.saturating_sub(params.client_balance_sat);

    let default_lsp_balance = calc_default_lsp_balance(
        params.client_balance_sat,
        max_lsp_balance,
        threshold_1_sat,
        threshold_2_sat,
        default_lsp_target_sat,
    );

    let max_client_balance = calc_max_client_balance(max_channel_size);

    ChannelLiquidityOptions {
        default_lsp_balance_sat: default_lsp_balance,
        min_lsp_balance_sat: min_lsp_balance,
        max_lsp_balance_sat: max_lsp_balance,
        max_client_balance_sat: max_client_balance,
    }
}

/// Calculates just the default LSP balance for CJIT channels.
/// Simpler calculation without existing channel deduction or 2% buffer.
pub fn get_default_lsp_balance(params: DefaultLspBalanceParams) -> u64 {
    let threshold_1_sat = THRESHOLD_1_EUR * params.sats_per_eur;
    let threshold_2_sat = THRESHOLD_2_EUR * params.sats_per_eur;
    let default_lsp_target_sat = DEFAULT_LSP_TARGET_EUR * params.sats_per_eur;

    let lsp_balance = if params.client_balance_sat > threshold_2_sat {
        params.max_channel_size_sat
    } else if params.client_balance_sat > threshold_1_sat {
        params.client_balance_sat
    } else {
        default_lsp_target_sat.saturating_sub(params.client_balance_sat)
    };

    lsp_balance.min(params.max_channel_size_sat)
}

fn calc_default_lsp_balance(
    client_balance_sat: u64,
    max_lsp_balance: u64,
    threshold_1_sat: u64,
    threshold_2_sat: u64,
    default_lsp_target_sat: u64,
) -> u64 {
    let lsp_balance = if client_balance_sat > threshold_2_sat {
        max_lsp_balance
    } else if client_balance_sat > threshold_1_sat {
        client_balance_sat
    } else {
        default_lsp_target_sat.saturating_sub(client_balance_sat)
    };

    lsp_balance.min(max_lsp_balance)
}

fn calc_min_lsp_balance(client_balance_sat: u64, min_channel_size_sat: u64) -> u64 {
    let ldk_minimum = (client_balance_sat as f64 * LDK_RESERVE_PERCENT) as u64;
    let channel_minimum = min_channel_size_sat.saturating_sub(client_balance_sat);
    ldk_minimum.max(channel_minimum)
}

fn calc_max_client_balance(max_channel_size: u64) -> u64 {
    let min_remote_balance = (max_channel_size as f64 * LDK_RESERVE_PERCENT) as u64;
    max_channel_size.saturating_sub(min_remote_balance)
}
