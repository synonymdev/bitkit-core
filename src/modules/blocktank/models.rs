use rust_blocktank_client::BlocktankClient;
use tokio::sync::Mutex;
use rusqlite::Connection;

pub struct BlocktankDB {
    pub(crate) conn: Mutex<Connection>,
    pub(crate) client: BlocktankClient,
}

pub const CREATE_ENUM_TABLES: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS order_states (
        state TEXT PRIMARY KEY,
        description TEXT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS payment_states (
        state TEXT PRIMARY KEY,
        description TEXT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS cjit_states (
        state TEXT PRIMARY KEY,
        description TEXT NOT NULL
    )",
];

pub const CREATE_ORDERS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS orders (
        id TEXT PRIMARY KEY,
        state TEXT NOT NULL REFERENCES order_states(state),
        state2 TEXT NOT NULL,
        fee_sat INTEGER NOT NULL CHECK (fee_sat >= 0),
        network_fee_sat INTEGER NOT NULL CHECK (network_fee_sat >= 0),
        service_fee_sat INTEGER NOT NULL CHECK (service_fee_sat >= 0),
        lsp_balance_sat INTEGER NOT NULL CHECK (lsp_balance_sat > 0),
        client_balance_sat INTEGER NOT NULL CHECK (client_balance_sat >= 0),
        zero_conf BOOLEAN NOT NULL,
        zero_reserve BOOLEAN NOT NULL,
        client_node_id TEXT,
        channel_expiry_weeks INTEGER NOT NULL CHECK (channel_expiry_weeks > 0),
        channel_expires_at INTEGER NOT NULL CHECK (channel_expires_at > 0),
        order_expires_at INTEGER NOT NULL CHECK (order_expires_at > 0),
        lnurl TEXT,
        coupon_code TEXT,
        source TEXT,
        updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
        created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
        channel_data TEXT,  -- JSON for IBtChannel
        lsp_node_data TEXT NOT NULL,  -- JSON for ILspNode
        payment_data TEXT NOT NULL,  -- JSON for IBtPayment
        discount_data TEXT,  -- JSON for IDiscount
        CONSTRAINT check_expires CHECK (
            channel_expires_at > created_at
            AND order_expires_at > created_at
        )
    )";

pub const CREATE_INFO_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS info (
        version INTEGER PRIMARY KEY,
        nodes TEXT NOT NULL,  -- JSON array of ILspNode
        options TEXT NOT NULL,  -- JSON of IBtInfoOptions
        versions TEXT NOT NULL,  -- JSON of IBtInfoVersions
        onchain TEXT NOT NULL,  -- JSON of IBtInfoOnchain
        updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
        is_current BOOLEAN NOT NULL DEFAULT 1
    )";

pub const CREATE_CJIT_ENTRIES_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS cjit_entries (
        id TEXT PRIMARY KEY,
        state TEXT NOT NULL REFERENCES cjit_states(state),
        fee_sat INTEGER NOT NULL CHECK (fee_sat >= 0),
        network_fee_sat INTEGER NOT NULL CHECK (network_fee_sat >= 0),
        service_fee_sat INTEGER NOT NULL CHECK (service_fee_sat >= 0),
        channel_size_sat INTEGER NOT NULL CHECK (channel_size_sat > 0),
        channel_expiry_weeks INTEGER NOT NULL CHECK (channel_expiry_weeks > 0),
        channel_open_error TEXT,
        node_id TEXT NOT NULL,
        coupon_code TEXT NOT NULL,
        source TEXT,
        expires_at INTEGER NOT NULL CHECK (expires_at > 0),
        updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
        created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
        invoice_data TEXT NOT NULL,  -- JSON for IBtBolt11Invoice
        channel_data TEXT,  -- JSON for IBtChannel
        lsp_node_data TEXT NOT NULL,  -- JSON for ILspNode
        discount_data TEXT,  -- JSON for IDiscount
        CONSTRAINT check_expires CHECK (expires_at > created_at)
    )";

/// Trigger statements for automatic timestamp updates and data management
pub const TRIGGER_STATEMENTS: &[&str] = &[
    // Orders update trigger
    "CREATE TRIGGER IF NOT EXISTS orders_update_trigger
     AFTER UPDATE ON orders
     BEGIN
         UPDATE orders
         SET updated_at = strftime('%s', 'now')
         WHERE id = NEW.id;
     END",

    // Info update trigger with version management
    "CREATE TRIGGER IF NOT EXISTS info_update_trigger
     AFTER UPDATE ON info
     BEGIN
         UPDATE info
         SET updated_at = strftime('%s', 'now')
         WHERE version = NEW.version;
     END",

    // CJIT entries update trigger
    "CREATE TRIGGER IF NOT EXISTS cjit_entries_update_trigger
     AFTER UPDATE ON cjit_entries
     BEGIN
         UPDATE cjit_entries
         SET updated_at = strftime('%s', 'now')
         WHERE id = NEW.id;
     END",

    // Ensure single current version trigger - INSERT
    "CREATE TRIGGER IF NOT EXISTS ensure_single_current_version_insert
     BEFORE INSERT ON info
     WHEN NEW.is_current = 1
     BEGIN
         UPDATE info SET is_current = 0;
     END",

    // Ensure single current version trigger - UPDATE
    "CREATE TRIGGER IF NOT EXISTS ensure_single_current_version
     BEFORE UPDATE ON info
     WHEN NEW.is_current = 1
     BEGIN
         UPDATE info SET is_current = 0
         WHERE version != NEW.version;
     END"
];

/// Database indexes for optimizing queries
pub const INDEX_STATEMENTS: &[&str] = &[
    // Orders indexes
    "CREATE INDEX IF NOT EXISTS idx_orders_state ON orders(state)",
    "CREATE INDEX IF NOT EXISTS idx_orders_state2 ON orders(state2)",
    "CREATE INDEX IF NOT EXISTS idx_orders_state_created ON orders(state, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_orders_node_id ON orders(client_node_id)
     WHERE client_node_id IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS idx_orders_coupon ON orders(coupon_code)
     WHERE coupon_code IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS idx_orders_created_at ON orders(created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_orders_updated_at ON orders(updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_orders_expires_at ON orders(order_expires_at DESC)",

    // CJIT entries indexes
    "CREATE INDEX IF NOT EXISTS idx_cjit_state ON cjit_entries(state)",
    "CREATE INDEX IF NOT EXISTS idx_cjit_node_state ON cjit_entries(node_id, state)",
    "CREATE INDEX IF NOT EXISTS idx_cjit_expires_at ON cjit_entries(expires_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_cjit_created_at ON cjit_entries(created_at DESC)"
];