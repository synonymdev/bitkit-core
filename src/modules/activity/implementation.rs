use crate::activity::{
    Activity, ActivityError, ActivityFilter, ActivityTags, ClosedChannelDetails, LightningActivity,
    OnchainActivity, PaymentState, PaymentType, PreActivityMetadata, SortDirection,
    TransactionDetails, TxInput, TxOutput,
};
use rusqlite::{Connection, OptionalExtension};
use serde_json;

pub struct ActivityDB {
    pub conn: Connection,
}
const CREATE_ACTIVITIES_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS activities (
        id TEXT PRIMARY KEY,
        activity_type TEXT NOT NULL CHECK (activity_type IN ('onchain', 'lightning')),
        tx_type TEXT NOT NULL CHECK (tx_type IN ('sent', 'received')),
        timestamp INTEGER NOT NULL CHECK (timestamp > 0),
        created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
        updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
        seen_at INTEGER CHECK (seen_at IS NULL OR seen_at > 0),
        contact TEXT CHECK (contact IS NULL OR length(contact) > 0)
    )";

const CREATE_ONCHAIN_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS onchain_activity (
        id TEXT PRIMARY KEY,
        tx_id TEXT NOT NULL,
        address TEXT NOT NULL CHECK (length(address) > 0),
        confirmed BOOLEAN NOT NULL,
        value INTEGER NOT NULL CHECK (value >= 0),
        fee INTEGER NOT NULL CHECK (fee >= 0),
        fee_rate INTEGER NOT NULL CHECK (fee_rate >= 0),
        is_boosted BOOLEAN NOT NULL,
        boost_tx_ids TEXT NOT NULL,
        is_transfer BOOLEAN NOT NULL,
        does_exist BOOLEAN NOT NULL,
        confirm_timestamp INTEGER CHECK (
            confirm_timestamp IS NULL OR confirm_timestamp >= 0
        ),
        channel_id TEXT CHECK (
            channel_id IS NULL OR length(channel_id) > 0
        ),
        transfer_tx_id TEXT CHECK (
            transfer_tx_id IS NULL OR length(transfer_tx_id) > 0
        ),
        FOREIGN KEY (id) REFERENCES activities(id) ON DELETE CASCADE
    )";

const CREATE_LIGHTNING_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS lightning_activity (
        id TEXT PRIMARY KEY,
        invoice TEXT NOT NULL CHECK (length(invoice) > 0),
        value INTEGER NOT NULL CHECK (value >= 0),
        status TEXT NOT NULL CHECK (status IN ('pending', 'succeeded', 'failed')),
        fee INTEGER CHECK (fee IS NULL OR fee >= 0),
        message TEXT NOT NULL,
        preimage TEXT CHECK (
            preimage IS NULL OR length(preimage) > 0
        ),
        FOREIGN KEY (id) REFERENCES activities(id) ON DELETE CASCADE
    )";

const CREATE_TAGS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS activity_tags (
        activity_id TEXT NOT NULL,
        tag TEXT NOT NULL,
        PRIMARY KEY (activity_id, tag),
        FOREIGN KEY (activity_id) REFERENCES activities(id)
            ON DELETE CASCADE
    )";

const CREATE_PRE_ACTIVITY_METADATA_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS pre_activity_metadata (
        payment_id TEXT PRIMARY KEY,
        tags TEXT NOT NULL,
        payment_hash TEXT,
        tx_id TEXT,
        address TEXT,
        is_receive BOOLEAN NOT NULL DEFAULT FALSE,
        fee_rate INTEGER NOT NULL DEFAULT 0,
        is_transfer BOOLEAN NOT NULL DEFAULT FALSE,
        channel_id TEXT,
        created_at INTEGER NOT NULL DEFAULT 0
    )";

const CREATE_CLOSED_CHANNELS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS closed_channels (
        channel_id TEXT PRIMARY KEY,
        counterparty_node_id TEXT NOT NULL,
        funding_txo_txid TEXT NOT NULL,
        funding_txo_index INTEGER NOT NULL CHECK (funding_txo_index >= 0),
        channel_value_sats INTEGER NOT NULL CHECK (channel_value_sats >= 0),
        closed_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
        outbound_capacity_msat INTEGER NOT NULL CHECK (outbound_capacity_msat >= 0),
        inbound_capacity_msat INTEGER NOT NULL CHECK (inbound_capacity_msat >= 0),
        counterparty_unspendable_punishment_reserve INTEGER NOT NULL CHECK (counterparty_unspendable_punishment_reserve >= 0),
        unspendable_punishment_reserve INTEGER NOT NULL CHECK (unspendable_punishment_reserve >= 0),
        forwarding_fee_proportional_millionths INTEGER NOT NULL CHECK (forwarding_fee_proportional_millionths >= 0),
        forwarding_fee_base_msat INTEGER NOT NULL CHECK (forwarding_fee_base_msat >= 0),
        channel_name TEXT NOT NULL,
        channel_closure_reason TEXT NOT NULL
    )";

const CREATE_TRANSACTION_DETAILS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS transaction_details (
        tx_id TEXT PRIMARY KEY,
        amount_sats INTEGER NOT NULL,
        inputs TEXT NOT NULL,
        outputs TEXT NOT NULL
    )";

const UPSERT_CLOSED_CHANNEL_SQL: &str = "
    INSERT OR REPLACE INTO closed_channels (
        channel_id, counterparty_node_id, funding_txo_txid, funding_txo_index,
        channel_value_sats, closed_at, outbound_capacity_msat, inbound_capacity_msat,
        counterparty_unspendable_punishment_reserve, unspendable_punishment_reserve,
        forwarding_fee_proportional_millionths, forwarding_fee_base_msat,
        channel_name, channel_closure_reason
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
    )";

const INDEX_STATEMENTS: &[&str] = &[
    // Activity indexes
    "CREATE INDEX IF NOT EXISTS idx_activities_type_timestamp ON activities(activity_type, timestamp DESC)",
    "CREATE INDEX IF NOT EXISTS idx_activities_timestamp ON activities(timestamp DESC)",

    // Onchain indexes
    "CREATE INDEX IF NOT EXISTS idx_onchain_txid_confirmed ON onchain_activity(tx_id, confirmed)",
    "CREATE INDEX IF NOT EXISTS idx_onchain_confirmed_timestamp ON onchain_activity(confirmed, confirm_timestamp DESC)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_onchain_id ON onchain_activity(id)",

    // Lightning indexes
    "CREATE INDEX IF NOT EXISTS idx_lightning_status_value ON lightning_activity(status, value DESC)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_lightning_id ON lightning_activity(id)",

    // Tags indexes
    "CREATE INDEX IF NOT EXISTS idx_activity_tags_tag_activity ON activity_tags(tag, activity_id)",

    // Pre-activity metadata indexes
    "CREATE INDEX IF NOT EXISTS idx_pre_activity_metadata_id ON pre_activity_metadata(payment_id)",
    "CREATE INDEX IF NOT EXISTS idx_pre_activity_metadata_address ON pre_activity_metadata(address)",
    "CREATE INDEX IF NOT EXISTS idx_pre_activity_metadata_tx_id ON pre_activity_metadata(tx_id)",

    // Closed channels indexes
    "CREATE INDEX IF NOT EXISTS idx_closed_channels_funding_txo ON closed_channels(funding_txo_txid)"
];

const TRIGGER_STATEMENTS: &[&str] = &[
    // Update timestamp trigger
    "CREATE TRIGGER IF NOT EXISTS activities_update_trigger
     AFTER UPDATE ON activities
     BEGIN
         UPDATE activities
         SET updated_at = strftime('%s', 'now')
         WHERE id = NEW.id;
     END",
    // Insert confirm timestamp validation trigger
    "CREATE TRIGGER IF NOT EXISTS onchain_confirm_timestamp_check_insert
     AFTER INSERT ON onchain_activity
     WHEN NEW.confirm_timestamp IS NOT NULL
     BEGIN
         SELECT CASE
             WHEN NEW.confirm_timestamp < (
                 SELECT timestamp FROM activities WHERE id = NEW.id
             )
             THEN RAISE(ABORT, 'confirm_timestamp must be greater than or equal to timestamp')
         END;
     END",
    // New update confirm timestamp validation trigger
    "CREATE TRIGGER IF NOT EXISTS onchain_confirm_timestamp_check_update
     AFTER UPDATE ON onchain_activity
     WHEN NEW.confirm_timestamp IS NOT NULL
     BEGIN
         SELECT CASE
             WHEN NEW.confirm_timestamp < (
                 SELECT timestamp FROM activities WHERE id = NEW.id
             )
             THEN RAISE(ABORT, 'confirm_timestamp must be greater than or equal to timestamp')
         END;
     END",
];

/// Migrations to apply to the activities table.
/// Each entry is (column_name, ALTER TABLE statement). The column is checked
/// via `PRAGMA table_info` before running the statement to avoid relying on
/// locale-dependent SQLite error messages.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "seen_at",
        "ALTER TABLE activities ADD COLUMN seen_at INTEGER CHECK (seen_at IS NULL OR seen_at > 0)",
    ),
    (
        "contact",
        "ALTER TABLE activities ADD COLUMN contact TEXT CHECK (contact IS NULL OR length(contact) > 0)",
    ),
];

impl ActivityDB {
    /// Creates a new ActivityDB instance with the specified database path.
    /// Initializes the database schema if it doesn't exist.
    pub fn new(db_path: &str) -> Result<ActivityDB, ActivityError> {
        // Create the directory if it doesn't exist
        if let Some(dir_path) = std::path::Path::new(db_path).parent() {
            if !dir_path.exists() {
                std::fs::create_dir_all(dir_path).map_err(|e| {
                    ActivityError::InitializationError {
                        error_details: format!("Failed to create directory: {}", e),
                    }
                })?;
            }
        }

        // If the path already contains .db or .sqlite, use it as is
        let final_path = if db_path.ends_with(".db") || db_path.ends_with(".sqlite") {
            db_path.to_string()
        } else {
            // Otherwise append activity.db
            format!("{}/activity.db", db_path.trim_end_matches('/'))
        };

        let conn = match Connection::open(&final_path) {
            Ok(conn) => conn,
            Err(e) => {
                return Err(ActivityError::InitializationError {
                    error_details: format!("Error opening database: {}", e),
                });
            }
        };
        let db = ActivityDB { conn };
        db.initialize()?;
        Ok(db)
    }

    /// Initialize database schema with tables, indexes, and triggers
    fn initialize(&self) -> Result<(), ActivityError> {
        // Create base activities table
        if let Err(e) = self.conn.execute(CREATE_ACTIVITIES_TABLE, []) {
            return Err(ActivityError::InitializationError {
                error_details: format!("Error creating activities table: {}", e),
            });
        }

        // Create onchain table
        if let Err(e) = self.conn.execute(CREATE_ONCHAIN_TABLE, []) {
            return Err(ActivityError::InitializationError {
                error_details: format!("Error creating onchain_activity table: {}", e),
            });
        }

        // Create lightning table
        if let Err(e) = self.conn.execute(CREATE_LIGHTNING_TABLE, []) {
            return Err(ActivityError::InitializationError {
                error_details: format!("Error creating lightning_activity table: {}", e),
            });
        }

        // Create tags table
        if let Err(e) = self.conn.execute(CREATE_TAGS_TABLE, []) {
            return Err(ActivityError::InitializationError {
                error_details: format!("Error creating tags table: {}", e),
            });
        }

        // Create pre-activity metadata table
        if let Err(e) = self.conn.execute(CREATE_PRE_ACTIVITY_METADATA_TABLE, []) {
            return Err(ActivityError::InitializationError {
                error_details: format!("Error creating pre_activity_metadata table: {}", e),
            });
        }

        // Create closed channels table
        if let Err(e) = self.conn.execute(CREATE_CLOSED_CHANNELS_TABLE, []) {
            return Err(ActivityError::InitializationError {
                error_details: format!("Error creating closed_channels table: {}", e),
            });
        }

        // Create transaction details table
        if let Err(e) = self.conn.execute(CREATE_TRANSACTION_DETAILS_TABLE, []) {
            return Err(ActivityError::InitializationError {
                error_details: format!("Error creating transaction_details table: {}", e),
            });
        }

        // Create indexes
        for statement in INDEX_STATEMENTS {
            if let Err(e) = self.conn.execute(statement, []) {
                return Err(ActivityError::InitializationError {
                    error_details: format!("Error creating index: {}", e),
                });
            }
        }

        // Create triggers
        for statement in TRIGGER_STATEMENTS {
            if let Err(e) = self.conn.execute(statement, []) {
                return Err(ActivityError::InitializationError {
                    error_details: format!("Error creating trigger: {}", e),
                });
            }
        }

        // Run migrations. Check if each column already exists via PRAGMA table_info
        // before running ALTER TABLE, so we don't rely on locale-dependent error messages.
        for (column, statement) in MIGRATIONS {
            let column_exists: bool = self
                .conn
                .prepare("PRAGMA table_info(activities)")
                .and_then(|mut stmt| {
                    stmt.query_map([], |row| row.get::<_, String>(1))
                        .map(|rows| rows.filter_map(|r| r.ok()).any(|name| name == *column))
                })
                .unwrap_or(false);

            if !column_exists {
                self.conn.execute(statement, []).map_err(|e| {
                    ActivityError::InitializationError {
                        error_details: format!("Error running migration: {}", e),
                    }
                })?;
            }
        }

        Ok(())
    }

    pub fn upsert_activity(&mut self, activity: &Activity) -> Result<(), ActivityError> {
        match activity {
            Activity::Onchain(onchain) => {
                match self.update_onchain_activity_by_id(&onchain.id, onchain) {
                    Ok(_) => Ok(()),
                    Err(ActivityError::DataError { error_details })
                        if error_details == "No activity found with given ID" =>
                    {
                        self.insert_onchain_activity(onchain)
                    }
                    Err(e) => Err(e),
                }
            }
            Activity::Lightning(lightning) => {
                match self.update_lightning_activity_by_id(&lightning.id, lightning) {
                    Ok(_) => Ok(()),
                    Err(ActivityError::DataError { error_details })
                        if error_details == "No activity found with given ID" =>
                    {
                        self.insert_lightning_activity(lightning)
                    }
                    Err(e) => Err(e),
                }
            }
        }
    }

    /// Inserts a new onchain activity into the database.
    pub fn insert_onchain_activity(
        &mut self,
        activity: &OnchainActivity,
    ) -> Result<(), ActivityError> {
        if activity.id.is_empty() {
            return Err(ActivityError::DataError {
                error_details: "Activity ID cannot be empty".to_string(),
            });
        }

        let tx = match self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            }) {
            Ok(tx) => tx,
            Err(e) => return Err(e),
        };

        let activities_sql = "
            INSERT INTO activities (
                id, activity_type, tx_type, timestamp, contact
            ) VALUES (
                ?1, 'onchain', ?2, ?3, ?4
            )";

        tx.execute(
            activities_sql,
            (
                &activity.id,
                Self::payment_type_to_string(&activity.tx_type),
                activity.timestamp,
                &activity.contact,
            ),
        )
        .map_err(|e| ActivityError::InsertError {
            error_details: format!("Failed to insert into activities: {}", e),
        })?;

        let onchain_sql = "
            INSERT INTO onchain_activity (
                id, tx_id, address, confirmed, value, fee, fee_rate, is_boosted,
                boost_tx_ids, is_transfer, does_exist, confirm_timestamp,
                channel_id, transfer_tx_id
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
            )";

        let boost_tx_ids_str = activity.boost_tx_ids.join(",");

        tx.execute(
            onchain_sql,
            (
                &activity.id,
                &activity.tx_id,
                &activity.address,
                activity.confirmed,
                activity.value,
                activity.fee,
                activity.fee_rate,
                activity.is_boosted,
                &boost_tx_ids_str,
                activity.is_transfer,
                activity.does_exist,
                activity.confirm_timestamp,
                &activity.channel_id,
                &activity.transfer_tx_id,
            ),
        )
        .map_err(|e| ActivityError::InsertError {
            error_details: format!("Failed to insert into onchain_activity: {}", e),
        })?;

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        if activity.tx_type == PaymentType::Received {
            let _ = self.transfer_pre_activity_metadata_to_activity(
                &activity.address,
                &activity.id,
                true,
            );
        } else if activity.tx_type == PaymentType::Sent {
            let _ = self.transfer_pre_activity_metadata_to_activity(
                &activity.tx_id,
                &activity.id,
                false,
            );
        }

        Ok(())
    }

    /// Inserts a new lightning activity into the database.
    pub fn insert_lightning_activity(
        &mut self,
        activity: &LightningActivity,
    ) -> Result<(), ActivityError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        let activities_sql = "
            INSERT INTO activities (
                id, activity_type, tx_type, timestamp, contact
            ) VALUES (
                ?1, 'lightning', ?2, ?3, ?4
            )";

        tx.execute(
            activities_sql,
            (
                &activity.id,
                Self::payment_type_to_string(&activity.tx_type),
                activity.timestamp,
                &activity.contact,
            ),
        )
        .map_err(|e| ActivityError::InsertError {
            error_details: format!("Failed to insert into activities: {}", e),
        })?;

        let lightning_sql = "
            INSERT INTO lightning_activity (
                id, invoice, value, status, fee, message, preimage
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7
            )";

        tx.execute(
            lightning_sql,
            (
                &activity.id,
                &activity.invoice,
                activity.value,
                Self::payment_state_to_string(&activity.status),
                activity.fee,
                &activity.message,
                &activity.preimage,
            ),
        )
        .map_err(|e| ActivityError::InsertError {
            error_details: format!("Failed to insert into lightning_activity: {}", e),
        })?;

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        let _ = self.transfer_pre_activity_metadata_to_activity(&activity.id, &activity.id, false);

        Ok(())
    }

    pub fn upsert_onchain_activities(
        &mut self,
        activities: &[OnchainActivity],
    ) -> Result<(), ActivityError> {
        if activities.is_empty() {
            return Ok(());
        }

        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        {
            let mut stmt_act = tx.prepare(
                "INSERT OR REPLACE INTO activities (id, activity_type, tx_type, timestamp, contact) VALUES (?1, 'onchain', ?2, ?3, ?4)"
            ).map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to prepare activities statement: {}", e),
            })?;
            let mut stmt_onchain = tx
                .prepare(
                    "INSERT OR REPLACE INTO onchain_activity (
                    id, tx_id, address, confirmed, value, fee, fee_rate, is_boosted,
                    boost_tx_ids, is_transfer, does_exist, confirm_timestamp,
                    channel_id, transfer_tx_id
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
                )",
                )
                .map_err(|e| ActivityError::DataError {
                    error_details: format!("Failed to prepare onchain statement: {}", e),
                })?;

            for activity in activities {
                if activity.id.is_empty() {
                    return Err(ActivityError::DataError {
                        error_details: "Activity ID cannot be empty".to_string(),
                    });
                }

                stmt_act
                    .execute((
                        &activity.id,
                        Self::payment_type_to_string(&activity.tx_type),
                        activity.timestamp,
                        &activity.contact,
                    ))
                    .map_err(|e| ActivityError::InsertError {
                        error_details: format!("Failed to upsert activities: {}", e),
                    })?;

                let boost_tx_ids_str = activity.boost_tx_ids.join(",");
                stmt_onchain
                    .execute((
                        &activity.id,
                        &activity.tx_id,
                        &activity.address,
                        activity.confirmed,
                        activity.value,
                        activity.fee,
                        activity.fee_rate,
                        activity.is_boosted,
                        &boost_tx_ids_str,
                        activity.is_transfer,
                        activity.does_exist,
                        activity.confirm_timestamp,
                        &activity.channel_id,
                        &activity.transfer_tx_id,
                    ))
                    .map_err(|e| ActivityError::InsertError {
                        error_details: format!("Failed to upsert onchain_activity: {}", e),
                    })?;
            }
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    pub fn upsert_lightning_activities(
        &mut self,
        activities: &[LightningActivity],
    ) -> Result<(), ActivityError> {
        if activities.is_empty() {
            return Ok(());
        }

        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        {
            let mut stmt_act = tx.prepare(
                "INSERT OR REPLACE INTO activities (id, activity_type, tx_type, timestamp, contact) VALUES (?1, 'lightning', ?2, ?3, ?4)"
            ).map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to prepare activities statement: {}", e),
            })?;
            let mut stmt_ln = tx
                .prepare(
                    "INSERT OR REPLACE INTO lightning_activity (
                    id, invoice, value, status, fee, message, preimage
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7
                )",
                )
                .map_err(|e| ActivityError::DataError {
                    error_details: format!("Failed to prepare lightning statement: {}", e),
                })?;

            for activity in activities {
                if activity.id.is_empty() {
                    return Err(ActivityError::DataError {
                        error_details: "Activity ID cannot be empty".to_string(),
                    });
                }

                stmt_act
                    .execute((
                        &activity.id,
                        Self::payment_type_to_string(&activity.tx_type),
                        activity.timestamp,
                        &activity.contact,
                    ))
                    .map_err(|e| ActivityError::InsertError {
                        error_details: format!("Failed to upsert activities: {}", e),
                    })?;

                stmt_ln
                    .execute((
                        &activity.id,
                        &activity.invoice,
                        activity.value,
                        Self::payment_state_to_string(&activity.status),
                        activity.fee,
                        &activity.message,
                        &activity.preimage,
                    ))
                    .map_err(|e| ActivityError::InsertError {
                        error_details: format!("Failed to upsert lightning_activity: {}", e),
                    })?;
            }
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    pub fn get_activities(
        &self,
        filter: Option<ActivityFilter>,
        tx_type: Option<PaymentType>,
        tags: Option<Vec<String>>,
        search: Option<String>,
        min_date: Option<u64>,
        max_date: Option<u64>,
        limit: Option<u32>,
        sort_direction: Option<SortDirection>,
    ) -> Result<Vec<Activity>, ActivityError> {
        let direction = sort_direction.unwrap_or_default();
        let filter = filter.unwrap_or(ActivityFilter::All);

        let mut query = String::from(
            "WITH filtered_activities AS (
            SELECT DISTINCT a.id
            FROM activities a
            LEFT JOIN activity_tags t ON a.id = t.activity_id
            LEFT JOIN onchain_activity o ON a.id = o.id
            LEFT JOIN lightning_activity l ON a.id = l.id
            WHERE 1=1",
        );

        // Activity type filter
        match filter {
            ActivityFilter::Lightning => query.push_str(" AND a.activity_type = 'lightning'"),
            ActivityFilter::Onchain => query.push_str(" AND a.activity_type = 'onchain'"),
            ActivityFilter::All => {}
        }

        // Transaction type filter
        if let Some(tx_type) = tx_type {
            query.push_str(&format!(
                " AND a.tx_type = '{}'",
                Self::payment_type_to_string(&tx_type)
            ));
        }

        // Tags filter (ANY of the provided tags)
        if let Some(tag_list) = tags {
            if !tag_list.is_empty() {
                query.push_str(" AND t.tag IN (");
                query.push_str(
                    &tag_list
                        .iter()
                        .map(|t| format!("'{}'", t.replace('\'', "''")))
                        .collect::<Vec<_>>()
                        .join(","),
                );
                query.push(')');
            }
        }

        // Date range filters
        if let Some(min) = min_date {
            query.push_str(&format!(" AND a.timestamp >= {}", min));
        }
        if let Some(max) = max_date {
            query.push_str(&format!(" AND a.timestamp <= {}", max));
        }

        // Text search filter
        if let Some(search_text) = search {
            if !search_text.is_empty() {
                let search_pattern = format!("%{}%", search_text.replace('\'', "''"));
                query.push_str(&format!(
                    " AND (
                o.address LIKE '{}' OR
                a.contact LIKE '{}' OR
                l.invoice LIKE '{}' OR
                l.message LIKE '{}'
            )",
                    search_pattern, search_pattern, search_pattern, search_pattern
                ));
            }
        }

        query.push_str(")");

        // Main query
        query.push_str(
            "
        SELECT
            a.id,
            a.activity_type,
            a.tx_type,
            a.timestamp,
            a.created_at,
            a.updated_at,
            a.seen_at,

            -- Onchain columns
            o.tx_id AS onchain_tx_id,
            o.value AS onchain_value,
            o.fee AS onchain_fee,
            o.fee_rate AS onchain_fee_rate,
            o.address AS onchain_address,
            o.confirmed AS onchain_confirmed,
            o.is_boosted AS onchain_is_boosted,
            o.boost_tx_ids AS onchain_boost_tx_ids,
            o.is_transfer AS onchain_is_transfer,
            o.does_exist AS onchain_does_exist,
            o.confirm_timestamp AS onchain_confirm_timestamp,
            o.channel_id AS onchain_channel_id,
            o.transfer_tx_id AS onchain_transfer_tx_id,

            -- Lightning columns
            l.invoice AS ln_invoice,
            l.value AS ln_value,
            l.status AS ln_status,
            l.fee AS ln_fee,
            l.message AS ln_message,
            l.preimage AS ln_preimage,
            a.contact AS contact

        FROM activities a
        INNER JOIN filtered_activities fa ON a.id = fa.id
        LEFT JOIN onchain_activity o ON a.id = o.id AND a.activity_type = 'onchain'
        LEFT JOIN lightning_activity l ON a.id = l.id AND a.activity_type = 'lightning'
        ORDER BY a.timestamp ",
        );

        // Add sort direction and limit
        query.push_str(Self::sort_direction_to_sql(direction));
        if let Some(n) = limit {
            query.push_str(&format!(" LIMIT {}", n));
        }

        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let activity_iter = stmt
            .query_map([], |row| {
                let activity_type: String = row.get(1)?;
                match activity_type.as_str() {
                    "onchain" => {
                        let timestamp: i64 = row.get(3)?;
                        let created_at: Option<i64> = row.get(4)?;
                        let updated_at: Option<i64> = row.get(5)?;
                        let seen_at: Option<i64> = row.get(6)?;
                        let value: i64 = row.get(8)?;
                        let fee: i64 = row.get(9)?;
                        let fee_rate: i64 = row.get(10)?;
                        let confirm_timestamp: Option<i64> = row.get(17)?;
                        let boost_tx_ids_str: String = row.get(14)?;
                        let boost_tx_ids: Vec<String> = if boost_tx_ids_str.is_empty() {
                            Vec::new()
                        } else {
                            boost_tx_ids_str.split(',').map(|s| s.to_string()).collect()
                        };

                        Ok(Activity::Onchain(OnchainActivity {
                            id: row.get(0)?,
                            tx_type: Self::parse_payment_type(row, 2)?,
                            timestamp: timestamp as u64,
                            created_at: created_at.map(|t| t as u64),
                            updated_at: updated_at.map(|t| t as u64),
                            seen_at: seen_at.map(|t| t as u64),
                            tx_id: row.get(7)?,
                            value: value as u64,
                            fee: fee as u64,
                            fee_rate: fee_rate as u64,
                            address: row.get(11)?,
                            confirmed: row.get(12)?,
                            is_boosted: row.get(13)?,
                            boost_tx_ids,
                            is_transfer: row.get(15)?,
                            does_exist: row.get(16)?,
                            confirm_timestamp: confirm_timestamp.map(|t| t as u64),
                            channel_id: row.get(18)?,
                            transfer_tx_id: row.get(19)?,
                            contact: row.get(26)?,
                        }))
                    }
                    "lightning" => {
                        let timestamp: i64 = row.get(3)?;
                        let created_at: Option<i64> = row.get(4)?;
                        let updated_at: Option<i64> = row.get(5)?;
                        let seen_at: Option<i64> = row.get(6)?;
                        let value: i64 = row.get(21)?;
                        let fee: Option<i64> = row.get(23)?;

                        Ok(Activity::Lightning(LightningActivity {
                            id: row.get(0)?,
                            tx_type: Self::parse_payment_type(row, 2)?,
                            timestamp: timestamp as u64,
                            created_at: created_at.map(|t| t as u64),
                            updated_at: updated_at.map(|t| t as u64),
                            seen_at: seen_at.map(|t| t as u64),
                            invoice: row.get(20)?,
                            value: value as u64,
                            status: Self::parse_payment_state(row, 22)?,
                            fee: fee.map(|f| f as u64),
                            message: row.get(24)?,
                            preimage: row.get(25)?,
                            contact: row.get(26)?,
                        }))
                    }
                    _ => Err(rusqlite::Error::InvalidColumnType(
                        1,
                        "activity_type".to_string(),
                        rusqlite::types::Type::Text,
                    )),
                }
            })
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to execute query: {}", e),
            })?;

        let mut activities = Vec::new();
        for activity_res in activity_iter {
            let activity = activity_res.map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to process row: {}", e),
            })?;
            activities.push(activity);
        }

        Ok(activities)
    }

    /// Retrieves a single activity by its ID.
    pub fn get_activity_by_id(&self, activity_id: &str) -> Result<Option<Activity>, ActivityError> {
        let activity_type: String = match self.conn.query_row(
            "SELECT activity_type FROM activities WHERE id = ?1",
            [activity_id],
            |row| row.get(0),
        ) {
            Ok(activity_type) => activity_type,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => {
                return Err(ActivityError::RetrievalError {
                    error_details: format!("Failed to get activity type: {}", e),
                })
            }
        };

        match activity_type.as_str() {
            "onchain" => {
                let sql = "
                SELECT
                    a.id, a.tx_type, o.tx_id, o.value, o.fee, o.fee_rate,
                    o.address, o.confirmed, a.timestamp, o.is_boosted,
                    o.boost_tx_ids, o.is_transfer, o.does_exist, o.confirm_timestamp,
                    o.channel_id, o.transfer_tx_id, a.created_at, a.updated_at, a.seen_at,
                    a.contact
                FROM activities a
                JOIN onchain_activity o ON a.id = o.id
                WHERE a.id = ?1";

                let mut stmt =
                    self.conn
                        .prepare(sql)
                        .map_err(|e| ActivityError::RetrievalError {
                            error_details: format!("Failed to prepare statement: {}", e),
                        })?;

                let activity = match stmt.query_row([activity_id], |row| {
                    let value: i64 = row.get(3)?;
                    let fee: i64 = row.get(4)?;
                    let fee_rate: i64 = row.get(5)?;
                    let timestamp: i64 = row.get(8)?;
                    let confirm_timestamp: Option<i64> = row.get(13)?;
                    let created_at: Option<i64> = row.get(16)?;
                    let updated_at: Option<i64> = row.get(17)?;
                    let seen_at: Option<i64> = row.get(18)?;
                    let boost_tx_ids_str: String = row.get(10)?;
                    let boost_tx_ids: Vec<String> = if boost_tx_ids_str.is_empty() {
                        Vec::new()
                    } else {
                        boost_tx_ids_str.split(',').map(|s| s.to_string()).collect()
                    };

                    Ok(Activity::Onchain(OnchainActivity {
                        id: row.get(0)?,
                        tx_type: Self::parse_payment_type(row, 1)?,
                        tx_id: row.get(2)?,
                        value: value as u64,
                        fee: fee as u64,
                        fee_rate: fee_rate as u64,
                        address: row.get(6)?,
                        confirmed: row.get(7)?,
                        timestamp: timestamp as u64,
                        is_boosted: row.get(9)?,
                        boost_tx_ids,
                        is_transfer: row.get(11)?,
                        does_exist: row.get(12)?,
                        confirm_timestamp: confirm_timestamp.map(|t| t as u64),
                        channel_id: row.get(14)?,
                        transfer_tx_id: row.get(15)?,
                        contact: row.get(19)?,
                        created_at: created_at.map(|t| t as u64),
                        updated_at: updated_at.map(|t| t as u64),
                        seen_at: seen_at.map(|t| t as u64),
                    }))
                }) {
                    Ok(activity) => Ok(Some(activity)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(ActivityError::RetrievalError {
                        error_details: format!("Failed to get onchain activity: {}", e),
                    }),
                };
                activity
            }
            "lightning" => {
                let sql = "
                SELECT
                    a.id, a.tx_type, l.status, l.value, l.fee,
                    l.invoice, l.message, a.timestamp,
                    l.preimage, a.created_at, a.updated_at, a.seen_at,
                    a.contact
                FROM activities a
                JOIN lightning_activity l ON a.id = l.id
                WHERE a.id = ?1";

                let mut stmt =
                    self.conn
                        .prepare(sql)
                        .map_err(|e| ActivityError::RetrievalError {
                            error_details: format!("Failed to prepare statement: {}", e),
                        })?;

                let activity = stmt
                    .query_row([activity_id], |row| {
                        let value: i64 = row.get(3)?;
                        let fee: Option<i64> = row.get(4)?;
                        let timestamp: i64 = row.get(7)?;
                        let created_at: Option<i64> = row.get(9)?;
                        let updated_at: Option<i64> = row.get(10)?;
                        let seen_at: Option<i64> = row.get(11)?;

                        Ok(Activity::Lightning(LightningActivity {
                            id: row.get(0)?,
                            tx_type: Self::parse_payment_type(row, 1)?,
                            status: Self::parse_payment_state(row, 2)?,
                            value: value as u64,
                            fee: fee.map(|f| f as u64),
                            invoice: row.get(5)?,
                            message: row.get(6)?,
                            timestamp: timestamp as u64,
                            preimage: row.get(8)?,
                            contact: row.get(12)?,
                            created_at: created_at.map(|t| t as u64),
                            updated_at: updated_at.map(|t| t as u64),
                            seen_at: seen_at.map(|t| t as u64),
                        }))
                    })
                    .map_err(|e| ActivityError::RetrievalError {
                        error_details: format!("Failed to get lightning activity: {}", e),
                    });

                Ok(Some(activity?))
            }
            _ => Ok(None),
        }
    }

    /// Retrieves an onchain activity by transaction ID.
    pub fn get_activity_by_tx_id(
        &self,
        tx_id: &str,
    ) -> Result<Option<OnchainActivity>, ActivityError> {
        let sql = "
            SELECT
                a.id, a.tx_type, o.tx_id, o.value, o.fee, o.fee_rate,
                o.address, o.confirmed, a.timestamp, o.is_boosted,
                o.boost_tx_ids, o.is_transfer, o.does_exist, o.confirm_timestamp,
                o.channel_id, o.transfer_tx_id, a.created_at, a.updated_at, a.seen_at,
                a.contact
            FROM activities a
            JOIN onchain_activity o ON a.id = o.id
            WHERE o.tx_id = ?1 AND a.activity_type = 'onchain'
            LIMIT 1";

        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let activity = match stmt.query_row([tx_id], |row| {
            let value: i64 = row.get(3)?;
            let fee: i64 = row.get(4)?;
            let fee_rate: i64 = row.get(5)?;
            let timestamp: i64 = row.get(8)?;
            let confirm_timestamp: Option<i64> = row.get(13)?;
            let created_at: Option<i64> = row.get(16)?;
            let updated_at: Option<i64> = row.get(17)?;
            let seen_at: Option<i64> = row.get(18)?;
            let boost_tx_ids_str: String = row.get(10)?;
            let boost_tx_ids: Vec<String> = if boost_tx_ids_str.is_empty() {
                Vec::new()
            } else {
                boost_tx_ids_str.split(',').map(|s| s.to_string()).collect()
            };

            Ok(OnchainActivity {
                id: row.get(0)?,
                tx_type: Self::parse_payment_type(row, 1)?,
                tx_id: row.get(2)?,
                value: value as u64,
                fee: fee as u64,
                fee_rate: fee_rate as u64,
                address: row.get(6)?,
                confirmed: row.get(7)?,
                timestamp: timestamp as u64,
                is_boosted: row.get(9)?,
                boost_tx_ids,
                is_transfer: row.get(11)?,
                does_exist: row.get(12)?,
                confirm_timestamp: confirm_timestamp.map(|t| t as u64),
                channel_id: row.get(14)?,
                transfer_tx_id: row.get(15)?,
                contact: row.get(19)?,
                created_at: created_at.map(|t| t as u64),
                updated_at: updated_at.map(|t| t as u64),
                seen_at: seen_at.map(|t| t as u64),
            })
        }) {
            Ok(activity) => Ok(Some(activity)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ActivityError::RetrievalError {
                error_details: format!("Failed to get activity by tx_id: {}", e),
            }),
        };

        activity
    }

    /// Updates an existing onchain activity by ID.
    pub fn update_onchain_activity_by_id(
        &mut self,
        activity_id: &str,
        activity: &OnchainActivity,
    ) -> Result<(), ActivityError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        let activities_sql = "
            UPDATE activities SET
                tx_type = ?1,
                timestamp = ?2,
                contact = ?3
            WHERE id = ?4 AND activity_type = 'onchain'";

        let rows = tx
            .execute(
                activities_sql,
                (
                    Self::payment_type_to_string(&activity.tx_type),
                    activity.timestamp,
                    &activity.contact,
                    activity_id,
                ),
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to update activities: {}", e),
            })?;

        if rows == 0 {
            return Err(ActivityError::DataError {
                error_details: "No activity found with given ID".to_string(),
            });
        }

        let onchain_sql = "
            UPDATE onchain_activity SET
                tx_id = ?1,
                address = ?2,
                confirmed = ?3,
                value = ?4,
                fee = ?5,
                fee_rate = ?6,
                is_boosted = ?7,
                boost_tx_ids = ?8,
                is_transfer = ?9,
                does_exist = ?10,
                confirm_timestamp = ?11,
                channel_id = ?12,
                transfer_tx_id = ?13
            WHERE id = ?14";

        let boost_tx_ids_str = activity.boost_tx_ids.join(",");

        tx.execute(
            onchain_sql,
            (
                &activity.tx_id,
                &activity.address,
                activity.confirmed,
                activity.value,
                activity.fee,
                activity.fee_rate,
                activity.is_boosted,
                &boost_tx_ids_str,
                activity.is_transfer,
                activity.does_exist,
                activity.confirm_timestamp,
                &activity.channel_id,
                &activity.transfer_tx_id,
                activity_id,
            ),
        )
        .map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to update onchain_activity: {}", e),
        })?;

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Updates an existing lightning activity by ID.
    pub fn update_lightning_activity_by_id(
        &mut self,
        activity_id: &str,
        activity: &LightningActivity,
    ) -> Result<(), ActivityError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        let activities_sql = "
            UPDATE activities SET
                tx_type = ?1,
                timestamp = ?2,
                contact = ?3
            WHERE id = ?4 AND activity_type = 'lightning'";

        let rows = tx
            .execute(
                activities_sql,
                (
                    Self::payment_type_to_string(&activity.tx_type),
                    activity.timestamp,
                    &activity.contact,
                    activity_id,
                ),
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to update activities: {}", e),
            })?;

        if rows == 0 {
            return Err(ActivityError::DataError {
                error_details: "No activity found with given ID".to_string(),
            });
        }

        let lightning_sql = "
            UPDATE lightning_activity SET
                invoice = ?1,
                value = ?2,
                status = ?3,
                fee = ?4,
                message = ?5,
                preimage = ?6
            WHERE id = ?7";

        tx.execute(
            lightning_sql,
            (
                &activity.invoice,
                activity.value,
                Self::payment_state_to_string(&activity.status),
                activity.fee,
                &activity.message,
                &activity.preimage,
                activity_id,
            ),
        )
        .map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to update lightning_activity: {}", e),
        })?;

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Marks an activity as seen by setting the seen_at timestamp.
    pub fn mark_activity_as_seen(
        &mut self,
        activity_id: &str,
        seen_at: u64,
    ) -> Result<(), ActivityError> {
        let rows = self
            .conn
            .execute(
                "UPDATE activities SET seen_at = ?1 WHERE id = ?2",
                rusqlite::params![seen_at as i64, activity_id],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to mark activity as seen: {}", e),
            })?;

        if rows == 0 {
            return Err(ActivityError::DataError {
                error_details: "No activity found with given ID".to_string(),
            });
        }

        Ok(())
    }

    /// Deletes an activity and associated data.
    pub fn delete_activity_by_id(&mut self, activity_id: &str) -> Result<bool, ActivityError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        // Delete from activities table (this will cascade to other tables)
        let rows = match tx.execute("DELETE FROM activities WHERE id = ?1", [activity_id]) {
            Ok(rows) => rows,
            Err(e) => {
                tx.rollback().ok();
                return Err(ActivityError::DataError {
                    error_details: format!("Failed to delete activity: {}", e),
                });
            }
        };

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(rows > 0)
    }

    /// Add tags to an activity
    pub fn add_tags(&mut self, activity_id: &str, tags: &[String]) -> Result<(), ActivityError> {
        // Verify the activity exists
        let exists = self
            .conn
            .query_row(
                "SELECT 1 FROM activities WHERE id = ?1",
                [activity_id],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to check activity existence: {}", e),
            })?
            .unwrap_or(false);

        if !exists {
            return Err(ActivityError::DataError {
                error_details: format!("Activity {} does not exist", activity_id),
            });
        }

        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        for tag in tags {
            tx.execute(
                "INSERT OR IGNORE INTO activity_tags (activity_id, tag) VALUES (?1, ?2)",
                [activity_id, tag],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to insert tag: {}", e),
            })?;
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Remove tags from an activity
    pub fn remove_tags(&mut self, activity_id: &str, tags: &[String]) -> Result<(), ActivityError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        for tag in tags {
            tx.execute(
                "DELETE FROM activity_tags WHERE activity_id = ?1 AND tag = ?2",
                [activity_id, tag],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to remove tag: {}", e),
            })?;
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Get all tags for an activity
    pub fn get_tags(&self, activity_id: &str) -> Result<Vec<String>, ActivityError> {
        // Verify the activity exists
        let exists = self
            .conn
            .query_row(
                "SELECT 1 FROM activities WHERE id = ?1",
                [activity_id],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to check activity existence: {}", e),
            })?
            .unwrap_or(false);

        if !exists {
            return Ok(Vec::new());
        }

        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM activity_tags WHERE activity_id = ?1")
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let tags = stmt
            .query_map([activity_id], |row| row.get(0))
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to execute query: {}", e),
            })?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to process rows: {}", e),
            })?;

        Ok(tags)
    }

    /// Get activities by tag with optional limit
    pub fn get_activities_by_tag(
        &self,
        tag: &str,
        limit: Option<u32>,
        sort_direction: Option<SortDirection>,
    ) -> Result<Vec<Activity>, ActivityError> {
        let direction = sort_direction.unwrap_or_default();
        let sql = format!(
            "SELECT a.id, a.activity_type
             FROM activities a
             JOIN activity_tags t ON a.id = t.activity_id
             WHERE t.tag = ?1
             ORDER BY a.timestamp {} {}",
            Self::sort_direction_to_sql(direction),
            limit.map_or(String::new(), |n| format!("LIMIT {}", n))
        );

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let rows = match stmt.query_map([tag], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }) {
            Ok(rows) => rows,
            Err(e) => {
                return Err(ActivityError::RetrievalError {
                    error_details: format!("Failed to execute query: {}", e),
                })
            }
        };

        let mut activities = Vec::new();
        for row in rows {
            let (id, _) = row.map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to process row: {}", e),
            })?;

            if let Some(activity) = self.get_activity_by_id(&id)? {
                activities.push(activity);
            }
        }

        Ok(activities)
    }

    /// Returns all unique tags stored in the database
    pub fn get_all_unique_tags(&self) -> Result<Vec<String>, ActivityError> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT tag FROM activity_tags ORDER BY tag ASC")
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let tags = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to execute query: {}", e),
            })?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to process rows: {}", e),
            })?;

        Ok(tags)
    }

    /// Get all activity tags for backup
    pub fn get_all_activities_tags(&self) -> Result<Vec<ActivityTags>, ActivityError> {
        let mut stmt = self
            .conn
            .prepare("SELECT activity_id, tag FROM activity_tags ORDER BY activity_id, tag")
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to execute query: {}", e),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to process rows: {}", e),
            })?;

        // Group by activity_id
        let mut grouped: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for (activity_id, tag) in rows {
            grouped
                .entry(activity_id)
                .or_insert_with(Vec::new)
                .push(tag);
        }

        let mut result: Vec<ActivityTags> = grouped
            .into_iter()
            .map(|(activity_id, tags)| ActivityTags { activity_id, tags })
            .collect();

        // Sort for consistent output
        result.sort_by(|a, b| a.activity_id.cmp(&b.activity_id));

        Ok(result)
    }

    /// Bulk upsert tags for multiple activities
    pub fn upsert_tags(&mut self, activity_tags: &[ActivityTags]) -> Result<(), ActivityError> {
        if activity_tags.is_empty() {
            return Ok(());
        }

        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        {
            let mut stmt = tx
                .prepare("INSERT OR IGNORE INTO activity_tags (activity_id, tag) VALUES (?1, ?2)")
                .map_err(|e| ActivityError::DataError {
                    error_details: format!("Failed to prepare statement: {}", e),
                })?;

            for activity_tag in activity_tags {
                if activity_tag.activity_id.is_empty() {
                    return Err(ActivityError::DataError {
                        error_details: "Activity ID cannot be empty".to_string(),
                    });
                }

                for tag in &activity_tag.tags {
                    if tag.is_empty() {
                        continue; // Skip empty tags
                    }
                    stmt.execute([&activity_tag.activity_id, tag])
                        .map_err(|e| ActivityError::DataError {
                            error_details: format!("Failed to insert tag: {}", e),
                        })?;
                }
            }
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Add pre-activity metadata for an onchain address or lightning invoice
    /// If the metadata has an address, any existing metadata with the same address will be removed first
    pub fn add_pre_activity_metadata(
        &mut self,
        pre_activity_metadata: &PreActivityMetadata,
    ) -> Result<(), ActivityError> {
        if pre_activity_metadata.payment_id.is_empty() {
            return Err(ActivityError::DataError {
                error_details: "Payment ID cannot be empty".to_string(),
            });
        }

        let tags_json = serde_json::to_string(&pre_activity_metadata.tags).map_err(|e| {
            ActivityError::DataError {
                error_details: format!("Failed to serialize tags: {}", e),
            }
        })?;

        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        if let Some(ref address) = pre_activity_metadata.address {
            if !address.is_empty() {
                tx.execute(
                    "DELETE FROM pre_activity_metadata WHERE address = ?1",
                    [address],
                )
                .map_err(|e| ActivityError::DataError {
                    error_details: format!(
                        "Failed to delete existing metadata with address: {}",
                        e
                    ),
                })?;
            }
        }

        tx.execute(
            "INSERT OR REPLACE INTO pre_activity_metadata (payment_id, tags, payment_hash, tx_id, address, is_receive, fee_rate, is_transfer, channel_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                &pre_activity_metadata.payment_id,
                &tags_json,
                &pre_activity_metadata.payment_hash,
                &pre_activity_metadata.tx_id,
                &pre_activity_metadata.address,
                pre_activity_metadata.is_receive,
                pre_activity_metadata.fee_rate as i64,
                pre_activity_metadata.is_transfer,
                &pre_activity_metadata.channel_id,
                pre_activity_metadata.created_at as i64,
            ],
        ).map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to insert pre-activity metadata: {}", e),
        })?;

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Add tags to existing pre-activity metadata for an onchain address or lightning invoice
    /// Returns an error if the metadata doesn't exist
    pub fn add_pre_activity_metadata_tags(
        &mut self,
        payment_id: &str,
        tags_to_add: &[String],
    ) -> Result<(), ActivityError> {
        // Get current metadata
        let current_tags_json: Option<String> = self
            .conn
            .query_row(
                "SELECT tags FROM pre_activity_metadata WHERE payment_id = ?1",
                [payment_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to get current tags: {}", e),
            })?;

        let mut current_tags: Vec<String> = if let Some(tags_json) = current_tags_json {
            serde_json::from_str(&tags_json).map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to deserialize tags: {}", e),
            })?
        } else {
            return Err(ActivityError::DataError {
                error_details: format!(
                    "Pre-activity metadata not found for payment_id: {}",
                    payment_id
                ),
            });
        };

        // Add new tags, avoiding duplicates
        for tag in tags_to_add {
            if !current_tags.contains(tag) {
                current_tags.push(tag.clone());
            }
        }

        // Update with merged tags
        let updated_tags_json =
            serde_json::to_string(&current_tags).map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to serialize tags: {}", e),
            })?;

        self.conn
            .execute(
                "UPDATE pre_activity_metadata SET tags = ?1 WHERE payment_id = ?2",
                [&updated_tags_json, payment_id],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to update tags: {}", e),
            })?;

        Ok(())
    }

    /// Remove specific tags from pre-activity metadata for an onchain address or lightning invoice
    pub fn remove_pre_activity_metadata_tags(
        &mut self,
        payment_id: &str,
        tags_to_remove: &[String],
    ) -> Result<(), ActivityError> {
        // Get current metadata
        let current_tags_json: Option<String> = self
            .conn
            .query_row(
                "SELECT tags FROM pre_activity_metadata WHERE payment_id = ?1",
                [payment_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to get current tags: {}", e),
            })?;

        if let Some(tags_json) = current_tags_json {
            let mut current_tags: Vec<String> =
                serde_json::from_str(&tags_json).map_err(|e| ActivityError::DataError {
                    error_details: format!("Failed to deserialize tags: {}", e),
                })?;

            // Remove tags
            current_tags.retain(|tag| !tags_to_remove.contains(tag));

            // Update with new tags
            let updated_tags_json =
                serde_json::to_string(&current_tags).map_err(|e| ActivityError::DataError {
                    error_details: format!("Failed to serialize tags: {}", e),
                })?;

            self.conn
                .execute(
                    "UPDATE pre_activity_metadata SET tags = ?1 WHERE payment_id = ?2",
                    [&updated_tags_json, payment_id],
                )
                .map_err(|e| ActivityError::DataError {
                    error_details: format!("Failed to update tags: {}", e),
                })?;
        }

        Ok(())
    }

    /// Reset (clear all tags) from pre-activity metadata for an onchain address or lightning invoice
    pub fn reset_pre_activity_metadata_tags(
        &mut self,
        payment_id: &str,
    ) -> Result<(), ActivityError> {
        // Check if row exists first
        let exists: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pre_activity_metadata WHERE payment_id = ?1)",
                [payment_id],
                |row| row.get(0),
            )
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to check if metadata exists: {}", e),
            })?;

        if !exists {
            // Row doesn't exist, nothing to reset
            return Ok(());
        }

        let empty_tags_json =
            serde_json::to_string(&Vec::<String>::new()).map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to serialize empty tags: {}", e),
            })?;

        self.conn
            .execute(
                "UPDATE pre_activity_metadata SET tags = ?1 WHERE payment_id = ?2",
                [&empty_tags_json, payment_id],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to reset pre-activity metadata tags: {}", e),
            })?;

        Ok(())
    }

    /// Delete all pre-activity metadata for an onchain address or lightning invoice
    pub fn delete_pre_activity_metadata(&mut self, payment_id: &str) -> Result<(), ActivityError> {
        self.conn
            .execute(
                "DELETE FROM pre_activity_metadata WHERE payment_id = ?1",
                [payment_id],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete pre-activity metadata: {}", e),
            })?;

        Ok(())
    }

    /// Bulk upsert pre-activity metadata for backup/restore
    pub fn upsert_pre_activity_metadata(
        &mut self,
        pre_activity_metadata: &[PreActivityMetadata],
    ) -> Result<(), ActivityError> {
        if pre_activity_metadata.is_empty() {
            return Ok(());
        }

        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO pre_activity_metadata (payment_id, tags, payment_hash, tx_id, address, is_receive, fee_rate, is_transfer, channel_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
            ).map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

            for metadata in pre_activity_metadata {
                let tags_json = serde_json::to_string(&metadata.tags).map_err(|e| {
                    ActivityError::DataError {
                        error_details: format!("Failed to serialize tags: {}", e),
                    }
                })?;

                stmt.execute(rusqlite::params![
                    &metadata.payment_id,
                    &tags_json,
                    &metadata.payment_hash,
                    &metadata.tx_id,
                    &metadata.address,
                    metadata.is_receive,
                    metadata.fee_rate as i64,
                    metadata.is_transfer,
                    &metadata.channel_id,
                    metadata.created_at as i64,
                ])
                .map_err(|e| ActivityError::DataError {
                    error_details: format!("Failed to insert pre-activity metadata: {}", e),
                })?;
            }
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Get pre-activity metadata for a specific payment_id or address
    pub fn get_pre_activity_metadata(
        &self,
        search_key: &str,
        search_by_address: bool,
    ) -> Result<Option<PreActivityMetadata>, ActivityError> {
        let sql = if search_by_address {
            "
            SELECT
                payment_id, tags, payment_hash, tx_id, address, is_receive, fee_rate, is_transfer, channel_id, created_at
            FROM pre_activity_metadata
            WHERE address = ?1"
        } else {
            "
            SELECT
                payment_id, tags, payment_hash, tx_id, address, is_receive, fee_rate, is_transfer, channel_id, created_at
            FROM pre_activity_metadata
            WHERE payment_id = ?1"
        };

        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        match stmt.query_row([search_key], |row| {
            let payment_id_val: String = row.get(0)?;
            let tags_json: String = row.get(1)?;
            let payment_hash: Option<String> = row.get(2)?;
            let tx_id: Option<String> = row.get(3)?;
            let address: Option<String> = row.get(4)?;
            let is_receive: bool = row.get(5)?;
            let fee_rate: i64 = row.get(6)?;
            let is_transfer: bool = row.get(7)?;
            let channel_id: Option<String> = row.get(8)?;
            let created_at: i64 = row.get(9)?;

            let tags: Vec<String> =
                serde_json::from_str(&tags_json).map_err(|_e: serde_json::Error| {
                    rusqlite::Error::InvalidColumnType(
                        1,
                        "tags".to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?;
            let created_at_u64 = created_at as u64;

            Ok(PreActivityMetadata {
                payment_id: payment_id_val,
                tags,
                payment_hash,
                tx_id,
                address,
                is_receive,
                fee_rate: fee_rate as u64,
                is_transfer,
                channel_id,
                created_at: created_at_u64,
            })
        }) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ActivityError::RetrievalError {
                error_details: format!("Failed to get pre-activity metadata: {}", e),
            }),
        }
    }

    /// Get all pre-activity metadata for backup
    pub fn get_all_pre_activity_metadata(&self) -> Result<Vec<PreActivityMetadata>, ActivityError> {
        let mut stmt = self.conn.prepare(
            "SELECT payment_id, tags, payment_hash, tx_id, address, is_receive, fee_rate, is_transfer, channel_id, created_at FROM pre_activity_metadata ORDER BY payment_id"
        ).map_err(|e| ActivityError::RetrievalError {
            error_details: format!("Failed to prepare statement: {}", e),
        })?;

        let rows: Vec<(
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            bool,
            i64,
            bool,
            Option<String>,
            i64,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            })
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to execute query: {}", e),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to process rows: {}", e),
            })?;

        let mut result: Vec<PreActivityMetadata> = Vec::new();

        for (
            payment_id,
            tags_json,
            payment_hash,
            tx_id,
            address,
            is_receive,
            fee_rate,
            is_transfer,
            channel_id,
            created_at,
        ) in rows
        {
            let tags: Vec<String> =
                serde_json::from_str(&tags_json).map_err(|e| ActivityError::DataError {
                    error_details: format!("Failed to deserialize tags: {}", e),
                })?;
            let created_at_u64 = created_at as u64;

            result.push(PreActivityMetadata {
                payment_id,
                tags,
                payment_hash,
                tx_id,
                address,
                is_receive,
                fee_rate: fee_rate as u64,
                is_transfer,
                channel_id,
                created_at: created_at_u64,
            });
        }

        // Sort for consistent output
        result.sort_by(|a, b| a.payment_id.cmp(&b.payment_id));

        Ok(result)
    }

    fn transfer_pre_activity_metadata_to_activity(
        &mut self,
        search_key: &str,
        activity_id: &str,
        search_by_address: bool,
    ) -> Result<Vec<String>, ActivityError> {
        let metadata = match self.get_pre_activity_metadata(search_key, search_by_address)? {
            Some(m) => m,
            None => return Ok(Vec::new()),
        };

        let tags = metadata.tags;

        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        if let Some(address) = &metadata.address {
            if !address.is_empty() {
                tx.execute(
                    "UPDATE onchain_activity SET address = ?1 WHERE id = ?2",
                    [address, activity_id],
                )
                .map_err(|e| ActivityError::DataError {
                    error_details: format!("Failed to update address: {}", e),
                })?;
            }
        }

        if metadata.fee_rate > 0 {
            tx.execute(
                "UPDATE onchain_activity SET fee_rate = ?1 WHERE id = ?2",
                rusqlite::params![metadata.fee_rate as i64, activity_id],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to update fee_rate: {}", e),
            })?;
        }

        if metadata.is_transfer {
            tx.execute(
                "UPDATE onchain_activity SET is_transfer = ?1 WHERE id = ?2",
                rusqlite::params![metadata.is_transfer, activity_id],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to update is_transfer: {}", e),
            })?;
        }

        if let Some(channel_id) = &metadata.channel_id {
            if !channel_id.is_empty() {
                tx.execute(
                    "UPDATE onchain_activity SET channel_id = ?1 WHERE id = ?2",
                    [channel_id, activity_id],
                )
                .map_err(|e| ActivityError::DataError {
                    error_details: format!("Failed to update channel_id: {}", e),
                })?;
            }
        }

        for tag in &tags {
            tx.execute(
                "INSERT OR IGNORE INTO activity_tags (activity_id, tag) VALUES (?1, ?2)",
                [activity_id, tag],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to insert tag: {}", e),
            })?;
        }

        if search_by_address {
            tx.execute(
                "DELETE FROM pre_activity_metadata WHERE address = ?1 AND is_receive = 1",
                [search_key],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete pre-activity metadata: {}", e),
            })?;
        } else {
            tx.execute(
                "DELETE FROM pre_activity_metadata WHERE payment_id = ?1",
                [search_key],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete pre-activity metadata: {}", e),
            })?;
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(tags)
    }

    pub fn upsert_closed_channel(
        &mut self,
        channel: &ClosedChannelDetails,
    ) -> Result<(), ActivityError> {
        if channel.channel_id.is_empty() {
            return Err(ActivityError::DataError {
                error_details: "Channel ID cannot be empty".to_string(),
            });
        }

        self.conn
            .execute(
                UPSERT_CLOSED_CHANNEL_SQL,
                rusqlite::params![
                    &channel.channel_id,
                    &channel.counterparty_node_id,
                    &channel.funding_txo_txid,
                    channel.funding_txo_index as i64,
                    channel.channel_value_sats as i64,
                    channel.closed_at as i64,
                    channel.outbound_capacity_msat as i64,
                    channel.inbound_capacity_msat as i64,
                    channel.counterparty_unspendable_punishment_reserve as i64,
                    channel.unspendable_punishment_reserve as i64,
                    channel.forwarding_fee_proportional_millionths as i64,
                    channel.forwarding_fee_base_msat as i64,
                    &channel.channel_name,
                    &channel.channel_closure_reason,
                ],
            )
            .map_err(|e| ActivityError::InsertError {
                error_details: format!("Failed to insert closed channel: {}", e),
            })?;

        Ok(())
    }

    pub fn upsert_closed_channels(
        &mut self,
        channels: &[ClosedChannelDetails],
    ) -> Result<(), ActivityError> {
        if channels.is_empty() {
            return Ok(());
        }

        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        {
            let mut stmt =
                tx.prepare(UPSERT_CLOSED_CHANNEL_SQL)
                    .map_err(|e| ActivityError::DataError {
                        error_details: format!("Failed to prepare statement: {}", e),
                    })?;

            for channel in channels {
                if channel.channel_id.is_empty() {
                    return Err(ActivityError::DataError {
                        error_details: "Channel ID cannot be empty".to_string(),
                    });
                }

                stmt.execute(rusqlite::params![
                    &channel.channel_id,
                    &channel.counterparty_node_id,
                    &channel.funding_txo_txid,
                    channel.funding_txo_index as i64,
                    channel.channel_value_sats as i64,
                    channel.closed_at as i64,
                    channel.outbound_capacity_msat as i64,
                    channel.inbound_capacity_msat as i64,
                    channel.counterparty_unspendable_punishment_reserve as i64,
                    channel.unspendable_punishment_reserve as i64,
                    channel.forwarding_fee_proportional_millionths as i64,
                    channel.forwarding_fee_base_msat as i64,
                    &channel.channel_name,
                    &channel.channel_closure_reason,
                ])
                .map_err(|e| ActivityError::InsertError {
                    error_details: format!(
                        "Failed to insert closed channel {}: {}",
                        channel.channel_id, e
                    ),
                })?;
            }
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    pub fn get_closed_channel_by_id(
        &self,
        channel_id: &str,
    ) -> Result<Option<ClosedChannelDetails>, ActivityError> {
        let sql = "
            SELECT
                channel_id, counterparty_node_id, funding_txo_txid, funding_txo_index,
                channel_value_sats, closed_at, outbound_capacity_msat, inbound_capacity_msat,
                counterparty_unspendable_punishment_reserve, unspendable_punishment_reserve,
                forwarding_fee_proportional_millionths, forwarding_fee_base_msat,
                channel_name, channel_closure_reason
            FROM closed_channels
            WHERE channel_id = ?1";

        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        match stmt.query_row([channel_id], |row| {
            let channel_value_sats: i64 = row.get(4)?;
            let outbound_capacity_msat: i64 = row.get(6)?;
            let inbound_capacity_msat: i64 = row.get(7)?;
            let counterparty_unspendable_punishment_reserve: i64 = row.get(8)?;

            Ok(ClosedChannelDetails {
                channel_id: row.get(0)?,
                counterparty_node_id: row.get(1)?,
                funding_txo_txid: row.get(2)?,
                funding_txo_index: row.get::<_, i64>(3)? as u32,
                channel_value_sats: channel_value_sats as u64,
                closed_at: row.get::<_, i64>(5)? as u64,
                outbound_capacity_msat: outbound_capacity_msat as u64,
                inbound_capacity_msat: inbound_capacity_msat as u64,
                counterparty_unspendable_punishment_reserve:
                    counterparty_unspendable_punishment_reserve as u64,
                unspendable_punishment_reserve: row.get::<_, i64>(9)? as u64,
                forwarding_fee_proportional_millionths: row.get::<_, i64>(10)? as u32,
                forwarding_fee_base_msat: row.get::<_, i64>(11)? as u32,
                channel_name: row.get(12)?,
                channel_closure_reason: row.get(13)?,
            })
        }) {
            Ok(channel) => Ok(Some(channel)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ActivityError::RetrievalError {
                error_details: format!("Failed to get closed channel: {}", e),
            }),
        }
    }

    pub fn get_all_closed_channels(
        &self,
        sort_direction: Option<SortDirection>,
    ) -> Result<Vec<ClosedChannelDetails>, ActivityError> {
        let direction = sort_direction.unwrap_or_default();
        let sql = format!(
            "
            SELECT
                channel_id, counterparty_node_id, funding_txo_txid, funding_txo_index,
                channel_value_sats, closed_at, outbound_capacity_msat, inbound_capacity_msat,
                counterparty_unspendable_punishment_reserve, unspendable_punishment_reserve,
                forwarding_fee_proportional_millionths, forwarding_fee_base_msat,
                channel_name, channel_closure_reason
            FROM closed_channels
            ORDER BY closed_at {}
            ",
            Self::sort_direction_to_sql(direction)
        );

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let channels = stmt
            .query_map([], |row| {
                let channel_value_sats: i64 = row.get(4)?;
                let outbound_capacity_msat: i64 = row.get(6)?;
                let inbound_capacity_msat: i64 = row.get(7)?;
                let counterparty_unspendable_punishment_reserve: i64 = row.get(8)?;

                Ok(ClosedChannelDetails {
                    channel_id: row.get(0)?,
                    counterparty_node_id: row.get(1)?,
                    funding_txo_txid: row.get(2)?,
                    funding_txo_index: row.get::<_, i64>(3)? as u32,
                    channel_value_sats: channel_value_sats as u64,
                    closed_at: row.get::<_, i64>(5)? as u64,
                    outbound_capacity_msat: outbound_capacity_msat as u64,
                    inbound_capacity_msat: inbound_capacity_msat as u64,
                    counterparty_unspendable_punishment_reserve:
                        counterparty_unspendable_punishment_reserve as u64,
                    unspendable_punishment_reserve: row.get::<_, i64>(9)? as u64,
                    forwarding_fee_proportional_millionths: row.get::<_, i64>(10)? as u32,
                    forwarding_fee_base_msat: row.get::<_, i64>(11)? as u32,
                    channel_name: row.get(12)?,
                    channel_closure_reason: row.get(13)?,
                })
            })
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to execute query: {}", e),
            })?
            .collect::<Result<Vec<ClosedChannelDetails>, _>>()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to process rows: {}", e),
            })?;

        Ok(channels)
    }

    /// Helper function to convert PaymentType to string
    fn payment_type_to_string(payment_type: &PaymentType) -> &'static str {
        match payment_type {
            PaymentType::Sent => "sent",
            PaymentType::Received => "received",
        }
    }

    /// Helper function to convert PaymentState to string
    fn payment_state_to_string(state: &PaymentState) -> &'static str {
        match state {
            PaymentState::Pending => "pending",
            PaymentState::Succeeded => "succeeded",
            PaymentState::Failed => "failed",
        }
    }

    /// Helper function to parse PaymentType from row
    fn parse_payment_type(row: &rusqlite::Row, index: usize) -> rusqlite::Result<PaymentType> {
        match row.get::<_, String>(index)?.as_str() {
            "sent" => Ok(PaymentType::Sent),
            "received" => Ok(PaymentType::Received),
            _ => Err(rusqlite::Error::InvalidColumnType(
                index,
                "tx_type".to_string(),
                rusqlite::types::Type::Text,
            )),
        }
    }

    /// Helper function to parse PaymentState from row
    fn parse_payment_state(row: &rusqlite::Row, index: usize) -> rusqlite::Result<PaymentState> {
        match row.get::<_, String>(index)?.as_str() {
            "pending" => Ok(PaymentState::Pending),
            "succeeded" => Ok(PaymentState::Succeeded),
            "failed" => Ok(PaymentState::Failed),
            _ => Err(rusqlite::Error::InvalidColumnType(
                index,
                "status".to_string(),
                rusqlite::types::Type::Text,
            )),
        }
    }

    /// Helper function to convert SortDirection to SQL string
    fn sort_direction_to_sql(direction: SortDirection) -> &'static str {
        match direction {
            SortDirection::Asc => "ASC",
            SortDirection::Desc => "DESC",
        }
    }

    /// Wipes all closed channels from the database
    pub fn wipe_all_closed_channels(&mut self) -> Result<(), ActivityError> {
        self.conn
            .execute("DELETE FROM closed_channels", [])
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete all closed channels: {}", e),
            })?;

        Ok(())
    }

    pub fn remove_closed_channel_by_id(&mut self, channel_id: &str) -> Result<bool, ActivityError> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM closed_channels WHERE channel_id = ?1",
                [channel_id],
            )
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete closed channel: {}", e),
            })?;

        Ok(rows > 0)
    }

    /// Upserts transaction details for one or more onchain transactions.
    pub fn upsert_transaction_details(
        &mut self,
        details_list: &[TransactionDetails],
    ) -> Result<(), ActivityError> {
        if details_list.is_empty() {
            return Ok(());
        }

        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO transaction_details (tx_id, amount_sats, inputs, outputs) VALUES (?1, ?2, ?3, ?4)"
            ).map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

            for details in details_list {
                if details.tx_id.is_empty() {
                    return Err(ActivityError::DataError {
                        error_details: "Transaction ID cannot be empty".to_string(),
                    });
                }

                let inputs_json = serde_json::to_string(&details.inputs).map_err(|e| {
                    ActivityError::DataError {
                        error_details: format!("Failed to serialize inputs: {}", e),
                    }
                })?;

                let outputs_json = serde_json::to_string(&details.outputs).map_err(|e| {
                    ActivityError::DataError {
                        error_details: format!("Failed to serialize outputs: {}", e),
                    }
                })?;

                stmt.execute(rusqlite::params![
                    &details.tx_id,
                    details.amount_sats,
                    &inputs_json,
                    &outputs_json,
                ])
                .map_err(|e| ActivityError::InsertError {
                    error_details: format!("Failed to upsert transaction details: {}", e),
                })?;
            }
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Retrieves transaction details by transaction ID.
    pub fn get_transaction_details(
        &self,
        tx_id: &str,
    ) -> Result<Option<TransactionDetails>, ActivityError> {
        let sql =
            "SELECT tx_id, amount_sats, inputs, outputs FROM transaction_details WHERE tx_id = ?1";

        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        match stmt.query_row([tx_id], |row| {
            let tx_id: String = row.get(0)?;
            let amount_sats: i64 = row.get(1)?;
            let inputs_json: String = row.get(2)?;
            let outputs_json: String = row.get(3)?;

            let inputs: Vec<TxInput> = serde_json::from_str(&inputs_json).map_err(|_| {
                rusqlite::Error::InvalidColumnType(
                    2,
                    "inputs".to_string(),
                    rusqlite::types::Type::Text,
                )
            })?;

            let outputs: Vec<TxOutput> = serde_json::from_str(&outputs_json).map_err(|_| {
                rusqlite::Error::InvalidColumnType(
                    3,
                    "outputs".to_string(),
                    rusqlite::types::Type::Text,
                )
            })?;

            Ok(TransactionDetails {
                tx_id,
                amount_sats,
                inputs,
                outputs,
            })
        }) {
            Ok(details) => Ok(Some(details)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ActivityError::RetrievalError {
                error_details: format!("Failed to get transaction details: {}", e),
            }),
        }
    }

    /// Retrieves all transaction details.
    pub fn get_all_transaction_details(&self) -> Result<Vec<TransactionDetails>, ActivityError> {
        let sql =
            "SELECT tx_id, amount_sats, inputs, outputs FROM transaction_details ORDER BY tx_id";

        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let rows = stmt
            .query_map([], |row| {
                let tx_id: String = row.get(0)?;
                let amount_sats: i64 = row.get(1)?;
                let inputs_json: String = row.get(2)?;
                let outputs_json: String = row.get(3)?;

                let inputs: Vec<TxInput> = serde_json::from_str(&inputs_json).map_err(|_| {
                    rusqlite::Error::InvalidColumnType(
                        2,
                        "inputs".to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?;

                let outputs: Vec<TxOutput> = serde_json::from_str(&outputs_json).map_err(|_| {
                    rusqlite::Error::InvalidColumnType(
                        3,
                        "outputs".to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?;

                Ok(TransactionDetails {
                    tx_id,
                    amount_sats,
                    inputs,
                    outputs,
                })
            })
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to execute query: {}", e),
            })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to process row: {}", e),
            })?);
        }

        Ok(results)
    }

    /// Deletes transaction details by transaction ID.
    pub fn delete_transaction_details(&mut self, tx_id: &str) -> Result<bool, ActivityError> {
        let rows = self
            .conn
            .execute("DELETE FROM transaction_details WHERE tx_id = ?1", [tx_id])
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete transaction details: {}", e),
            })?;

        Ok(rows > 0)
    }

    /// Wipes all transaction details from the database.
    pub fn wipe_all_transaction_details(&mut self) -> Result<(), ActivityError> {
        self.conn
            .execute("DELETE FROM transaction_details", [])
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete all transaction details: {}", e),
            })?;

        Ok(())
    }

    /// Wipes all activity data from the database
    /// This deletes all activities, which cascades to delete all activity_tags due to foreign key constraints.
    /// Also deletes all pre_activity_metadata and closed_channels.
    pub fn wipe_all(&mut self) -> Result<(), ActivityError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        // Delete from activities table (this will cascade to delete activity_tags due to foreign key constraints)
        tx.execute("DELETE FROM activities", [])
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete all activities: {}", e),
            })?;

        // Delete all pre-activity metadata
        tx.execute("DELETE FROM pre_activity_metadata", [])
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete all pre-activity metadata: {}", e),
            })?;

        // Delete all closed channels
        tx.execute("DELETE FROM closed_channels", [])
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete all closed channels: {}", e),
            })?;

        // Delete all transaction details
        tx.execute("DELETE FROM transaction_details", [])
            .map_err(|e| ActivityError::DataError {
                error_details: format!("Failed to delete all transaction details: {}", e),
            })?;

        tx.commit().map_err(|e| ActivityError::DataError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Check if an address has been used (has any activities associated with it).
    /// This checks for any onchain activities where the address appears, regardless
    /// of whether it's a sent or received transaction.
    pub fn is_address_used(&self, address: &str) -> Result<bool, ActivityError> {
        let count: i64 = self
            .conn
            .query_row(
                "
            SELECT COUNT(*)
            FROM activities a
            JOIN onchain_activity o ON a.id = o.id
            WHERE o.address = ?1 AND a.activity_type = 'onchain'
            ",
                [address],
                |row| row.get(0),
            )
            .map_err(|e| ActivityError::RetrievalError {
                error_details: format!("Failed to check address usage: {}", e),
            })?;

        Ok(count > 0)
    }
}
