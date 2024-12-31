use rusqlite::{Connection, OptionalExtension};
use crate::activity::{Activity, ActivityError, ActivityFilter, LightningActivity, OnchainActivity, PaymentState, PaymentType, SortDirection};

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
        updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
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
    "CREATE INDEX IF NOT EXISTS idx_activity_tags_tag_activity ON activity_tags(tag, activity_id)"
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
     END"
];

impl ActivityDB {
    /// Creates a new ActivityDB instance with the specified database path.
    /// Initializes the database schema if it doesn't exist.
    pub fn new(db_path: &str) -> Result<ActivityDB, ActivityError> {
        // Create the directory if it doesn't exist
        if let Some(dir_path) = std::path::Path::new(db_path).parent() {
            if !dir_path.exists() {
                std::fs::create_dir_all(dir_path).map_err(|e| ActivityError::InitializationError {
                    message: format!("Failed to create directory: {}", e),
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
                return Err(ActivityError::InitializationError{
                    message: format!("Error opening database: {}", e),
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
                message: format!("Error creating activities table: {}", e),
            });
        }

        // Create onchain table
        if let Err(e) = self.conn.execute(CREATE_ONCHAIN_TABLE, []) {
            return Err(ActivityError::InitializationError {
                message: format!("Error creating onchain_activity table: {}", e),
            });
        }

        // Create lightning table
        if let Err(e) = self.conn.execute(CREATE_LIGHTNING_TABLE, []) {
            return Err(ActivityError::InitializationError {
                message: format!("Error creating lightning_activity table: {}", e),
            });
        }

        // Create tags table
        if let Err(e) = self.conn.execute(CREATE_TAGS_TABLE, []) {
            return Err(ActivityError::InitializationError {
                message: format!("Error creating tags table: {}", e),
            });
        }

        // Create indexes
        for statement in INDEX_STATEMENTS {
            if let Err(e) = self.conn.execute(statement, []) {
                return Err(ActivityError::InitializationError {
                    message: format!("Error creating index: {}", e),
                });
            }
        }

        // Create triggers
        for statement in TRIGGER_STATEMENTS {
            if let Err(e) = self.conn.execute(statement, []) {
                return Err(ActivityError::InitializationError {
                    message: format!("Error creating trigger: {}", e),
                });
            }
        }

        Ok(())
    }

    pub fn upsert_activity(&mut self, activity: &Activity) -> Result<(), ActivityError> {
        match activity {
            Activity::Onchain(onchain) => {
                match self.update_onchain_activity_by_id(&onchain.id, onchain) {
                    Ok(_) => Ok(()),
                    Err(ActivityError::DataError { message }) if message == "No activity found with given ID" => {
                        self.insert_onchain_activity(onchain)
                    }
                    Err(e) => Err(e),
                }
            },
            Activity::Lightning(lightning) => {
                match self.update_lightning_activity_by_id(&lightning.id, lightning) {
                    Ok(_) => Ok(()),
                    Err(ActivityError::DataError { message }) if message == "No activity found with given ID" => {
                        self.insert_lightning_activity(lightning)
                    }
                    Err(e) => Err(e),
                }
            },
        }
    }

    /// Inserts a new onchain activity into the database.
    pub fn insert_onchain_activity(&mut self, activity: &OnchainActivity) -> Result<(), ActivityError> {
        if activity.id.is_empty() {
            return Err(ActivityError::DataError {
                message: "Activity ID cannot be empty".to_string(),
            });
        }

        let tx = match self.conn.transaction().map_err(|e| ActivityError::DataError {
            message: format!("Failed to start transaction: {}", e),
        }) {
            Ok(tx) => tx,
            Err(e) => return Err(e),
        };

        let activities_sql = "
            INSERT INTO activities (
                id, activity_type, tx_type, timestamp
            ) VALUES (
                ?1, 'onchain', ?2, ?3
            )";

        tx.execute(
            activities_sql,
            (
                &activity.id,
                Self::payment_type_to_string(&activity.tx_type),
                activity.timestamp,
            ),
        ).map_err(|e| ActivityError::InsertError {
            message: format!("Failed to insert into activities: {}", e),
        })?;

        let onchain_sql = "
            INSERT INTO onchain_activity (
                id, tx_id, address, confirmed, value, fee, fee_rate, is_boosted,
                is_transfer, does_exist, confirm_timestamp,
                channel_id, transfer_tx_id
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
            )";

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
                activity.is_transfer,
                activity.does_exist,
                activity.confirm_timestamp,
                &activity.channel_id,
                &activity.transfer_tx_id,
            ),
        ).map_err(|e| ActivityError::InsertError {
            message: format!("Failed to insert into onchain_activity: {}", e),
        })?;

        tx.commit().map_err(|e| ActivityError::DataError {
            message: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Inserts a new lightning activity into the database.
    pub fn insert_lightning_activity(&mut self, activity: &LightningActivity) -> Result<(), ActivityError> {
        let tx = self.conn.transaction().map_err(|e| ActivityError::DataError {
            message: format!("Failed to start transaction: {}", e),
        })?;

        let activities_sql = "
            INSERT INTO activities (
                id, activity_type, tx_type, timestamp
            ) VALUES (
                ?1, 'lightning', ?2, ?3
            )";

        tx.execute(
            activities_sql,
            (
                &activity.id,
                Self::payment_type_to_string(&activity.tx_type),
                activity.timestamp,
            ),
        ).map_err(|e| ActivityError::InsertError {
            message: format!("Failed to insert into activities: {}", e),
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
        ).map_err(|e| ActivityError::InsertError {
            message: format!("Failed to insert into lightning_activity: {}", e),
        })?;

        tx.commit().map_err(|e| ActivityError::DataError {
            message: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    pub fn get_activities(
        &self,
        filter: ActivityFilter,
        limit: Option<u32>,
        sort_direction: Option<SortDirection>
    ) -> Result<Vec<Activity>, ActivityError> {
        let direction = sort_direction.unwrap_or_default();
        let filter_clause = match filter {
            ActivityFilter::All => "",
            ActivityFilter::Lightning => "WHERE a.activity_type = 'lightning'",
            ActivityFilter::Onchain => "WHERE a.activity_type = 'onchain'",
        };

        let sql = format!(
            "SELECT
                a.id,
                a.activity_type,
                a.tx_type,
                a.timestamp,
                a.created_at,
                a.updated_at,

                -- Onchain columns
                o.tx_id AS onchain_tx_id,
                o.value AS onchain_value,
                o.fee AS onchain_fee,
                o.fee_rate AS onchain_fee_rate,
                o.address AS onchain_address,
                o.confirmed AS onchain_confirmed,
                o.is_boosted AS onchain_is_boosted,
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
                l.preimage AS ln_preimage

            FROM activities a
            LEFT JOIN onchain_activity o ON a.id = o.id AND a.activity_type = 'onchain'
            LEFT JOIN lightning_activity l ON a.id = l.id AND a.activity_type = 'lightning'
            {filter_clause}
            ORDER BY a.timestamp {direction}
            {limit_clause}",
            filter_clause = filter_clause,
            direction = Self::sort_direction_to_sql(direction),
            limit_clause = limit.map_or(String::new(), |n| format!("LIMIT {}", n))
        );

        let mut stmt = self.conn.prepare(&sql).map_err(|e| ActivityError::RetrievalError {
            message: format!("Failed to prepare statement: {}", e),
        })?;

        let activity_iter = stmt.query_map([], |row| {
            let activity_type: String = row.get(1)?;
            match activity_type.as_str() {
                "onchain" => {
                    let timestamp: i64 = row.get(3)?;
                    let created_at: Option<i64> = row.get(4)?;
                    let updated_at: Option<i64> = row.get(5)?;
                    let value: i64 = row.get(7)?;
                    let fee: i64 = row.get(8)?;
                    let fee_rate: i64 = row.get(9)?;
                    let confirm_timestamp: Option<i64> = row.get(15)?;

                    Ok(Activity::Onchain(OnchainActivity {
                        id: row.get(0)?,
                        tx_type: Self::parse_payment_type(row, 2)?,
                        timestamp: timestamp as u64,
                        created_at: created_at.map(|t| t as u64),
                        updated_at: updated_at.map(|t| t as u64),
                        tx_id: row.get(6)?,
                        value: value as u64,
                        fee: fee as u64,
                        fee_rate: fee_rate as u64,
                        address: row.get(10)?,
                        confirmed: row.get(11)?,
                        is_boosted: row.get(12)?,
                        is_transfer: row.get(13)?,
                        does_exist: row.get(14)?,
                        confirm_timestamp: confirm_timestamp.map(|t| t as u64),
                        channel_id: row.get(16)?,
                        transfer_tx_id: row.get(17)?,
                    }))
                }
                "lightning" => {
                    let timestamp: i64 = row.get(3)?;
                    let created_at: Option<i64> = row.get(4)?;
                    let updated_at: Option<i64> = row.get(5)?;
                    let value: i64 = row.get(19)?;
                    let fee: Option<i64> = row.get(21)?;

                    Ok(Activity::Lightning(LightningActivity {
                        id: row.get(0)?,
                        tx_type: Self::parse_payment_type(row, 2)?,
                        timestamp: timestamp as u64,
                        created_at: created_at.map(|t| t as u64),
                        updated_at: updated_at.map(|t| t as u64),
                        invoice: row.get(18)?,
                        value: value as u64,
                        status: Self::parse_payment_state(row, 20)?,
                        fee: fee.map(|f| f as u64),
                        message: row.get(22)?,
                        preimage: row.get(23)?,
                    }))
                }
                _ => Err(rusqlite::Error::InvalidColumnType(
                    1,
                    "activity_type".to_string(),
                    rusqlite::types::Type::Text,
                )),
            }
        }).map_err(|e| ActivityError::RetrievalError {
            message: format!("Failed to execute query: {}", e),
        })?;

        let mut activities = Vec::new();
        for activity_res in activity_iter {
            let activity = activity_res.map_err(|e| ActivityError::DataError {
                message: format!("Failed to process row: {}", e),
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
        Err(e) => return Err(ActivityError::RetrievalError {
            message: format!("Failed to get activity type: {}", e),
        }),
    };

    match activity_type.as_str() {
        "onchain" => {
            let sql = "
                SELECT
                    a.id, a.tx_type, o.tx_id, o.value, o.fee, o.fee_rate,
                    o.address, o.confirmed, a.timestamp, o.is_boosted,
                    o.is_transfer, o.does_exist, o.confirm_timestamp,
                    o.channel_id, o.transfer_tx_id, a.created_at, a.updated_at
                FROM activities a
                JOIN onchain_activity o ON a.id = o.id
                WHERE a.id = ?1";

            let mut stmt = self.conn.prepare(sql).map_err(|e| ActivityError::RetrievalError {
                message: format!("Failed to prepare statement: {}", e),
            })?;

            let activity = match stmt.query_row([activity_id], |row| {
                let value: i64 = row.get(3)?;
                let fee: i64 = row.get(4)?;
                let fee_rate: i64 = row.get(5)?;
                let timestamp: i64 = row.get(8)?;
                let confirm_timestamp: Option<i64> = row.get(12)?;
                let created_at: Option<i64> = row.get(15)?;
                let updated_at: Option<i64> = row.get(16)?;

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
                    is_transfer: row.get(10)?,
                    does_exist: row.get(11)?,
                    confirm_timestamp: confirm_timestamp.map(|t| t as u64),
                    channel_id: row.get(13)?,
                    transfer_tx_id: row.get(14)?,
                    created_at: created_at.map(|t| t as u64),
                    updated_at: updated_at.map(|t| t as u64),
                }))
            }) {
                Ok(activity) => Ok(Some(activity)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(ActivityError::RetrievalError {
                    message: format!("Failed to get onchain activity: {}", e),
                }),
            };
            activity
        },
        "lightning" => {
            let sql = "
                SELECT
                    a.id, a.tx_type, l.status, l.value, l.fee,
                    l.invoice, l.message, a.timestamp,
                    l.preimage, a.created_at, a.updated_at
                FROM activities a
                JOIN lightning_activity l ON a.id = l.id
                WHERE a.id = ?1";

            let mut stmt = self.conn.prepare(sql).map_err(|e| ActivityError::RetrievalError {
                message: format!("Failed to prepare statement: {}", e),
            })?;

            let activity = stmt.query_row([activity_id], |row| {
                let value: i64 = row.get(3)?;
                let fee: Option<i64> = row.get(4)?;
                let timestamp: i64 = row.get(7)?;
                let created_at: Option<i64> = row.get(9)?;
                let updated_at: Option<i64> = row.get(10)?;

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
                    created_at: created_at.map(|t| t as u64),
                    updated_at: updated_at.map(|t| t as u64),
                }))
            }).map_err(|e| ActivityError::RetrievalError {
                message: format!("Failed to get lightning activity: {}", e),
            });

            Ok(Some(activity?))
        },
        _ => Ok(None),
    }
}

    /// Updates an existing onchain activity by ID.
    pub fn update_onchain_activity_by_id(&mut self, activity_id: &str, activity: &OnchainActivity) -> Result<(), ActivityError> {
        let tx = self.conn.transaction().map_err(|e| ActivityError::DataError {
            message: format!("Failed to start transaction: {}", e),
        })?;

        let activities_sql = "
            UPDATE activities SET
                tx_type = ?1,
                timestamp = ?2
            WHERE id = ?3 AND activity_type = 'onchain'";

        let rows = tx.execute(
            activities_sql,
            (
                Self::payment_type_to_string(&activity.tx_type),
                activity.timestamp,
                activity_id,
            ),
        ).map_err(|e| ActivityError::DataError {
            message: format!("Failed to update activities: {}", e),
        })?;

        if rows == 0 {
            return Err(ActivityError::DataError {
                message: "No activity found with given ID".to_string(),
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
                is_transfer = ?8,
                does_exist = ?9,
                confirm_timestamp = ?10,
                channel_id = ?11,
                transfer_tx_id = ?12
            WHERE id = ?13";

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
                activity.is_transfer,
                activity.does_exist,
                activity.confirm_timestamp,
                &activity.channel_id,
                &activity.transfer_tx_id,
                activity_id,
            ),
        ).map_err(|e| ActivityError::DataError {
            message: format!("Failed to update onchain_activity: {}", e),
        })?;

        tx.commit().map_err(|e| ActivityError::DataError {
            message: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Updates an existing lightning activity by ID.
    pub fn update_lightning_activity_by_id(&mut self, activity_id: &str, activity: &LightningActivity) -> Result<(), ActivityError> {
        let tx = self.conn.transaction().map_err(|e| ActivityError::DataError {
            message: format!("Failed to start transaction: {}", e),
        })?;

        let activities_sql = "
            UPDATE activities SET
                tx_type = ?1,
                timestamp = ?2
            WHERE id = ?3 AND activity_type = 'lightning'";

        let rows = tx.execute(
            activities_sql,
            (
                Self::payment_type_to_string(&activity.tx_type),
                activity.timestamp,
                activity_id,
            ),
        ).map_err(|e| ActivityError::DataError {
            message: format!("Failed to update activities: {}", e),
        })?;

        if rows == 0 {
            return Err(ActivityError::DataError {
                message: "No activity found with given ID".to_string(),
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
        ).map_err(|e| ActivityError::DataError {
            message: format!("Failed to update lightning_activity: {}", e),
        })?;

        tx.commit().map_err(|e| ActivityError::DataError {
            message: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Deletes an activity and associated data.
    pub fn delete_activity_by_id(&mut self, activity_id: &str) -> Result<bool, ActivityError> {
        let tx = self.conn.transaction().map_err(|e| ActivityError::DataError {
            message: format!("Failed to start transaction: {}", e),
        })?;

        // Delete from activities table (this will cascade to other tables)
        let rows = match tx.execute(
            "DELETE FROM activities WHERE id = ?1",
            [activity_id],
        ) {
            Ok(rows) => rows,
            Err(e) => {
                tx.rollback().ok();
                return Err(ActivityError::DataError {
                    message: format!("Failed to delete activity: {}", e),
                });
            }
        };

        tx.commit().map_err(|e| ActivityError::DataError {
            message: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(rows > 0)
    }

    /// Add tags to an activity
    pub fn add_tags(&mut self, activity_id: &str, tags: &[String]) -> Result<(), ActivityError> {
        // Verify the activity exists
        let exists = self.conn.query_row(
            "SELECT 1 FROM activities WHERE id = ?1",
            [activity_id],
            |_| Ok(true)
        ).optional().map_err(|e| ActivityError::DataError {
            message: format!("Failed to check activity existence: {}", e),
        })?.unwrap_or(false);

        if !exists {
            return Err(ActivityError::DataError {
                message: format!("Activity {} does not exist", activity_id),
            });
        }

        let tx = self.conn.transaction().map_err(|e| ActivityError::DataError {
            message: format!("Failed to start transaction: {}", e),
        })?;

        for tag in tags {
            tx.execute(
                "INSERT OR IGNORE INTO activity_tags (activity_id, tag) VALUES (?1, ?2)",
                [activity_id, tag],
            ).map_err(|e| ActivityError::DataError {
                message: format!("Failed to insert tag: {}", e),
            })?;
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            message: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Remove tags from an activity
    pub fn remove_tags(&mut self, activity_id: &str, tags: &[String]) -> Result<(), ActivityError> {
        let tx = self.conn.transaction().map_err(|e| ActivityError::DataError {
            message: format!("Failed to start transaction: {}", e),
        })?;

        for tag in tags {
            tx.execute(
                "DELETE FROM activity_tags WHERE activity_id = ?1 AND tag = ?2",
                [activity_id, tag],
            ).map_err(|e| ActivityError::DataError {
                message: format!("Failed to remove tag: {}", e),
            })?;
        }

        tx.commit().map_err(|e| ActivityError::DataError {
            message: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    /// Get all tags for an activity
    pub fn get_tags(&self, activity_id: &str) -> Result<Vec<String>, ActivityError> {
        // Verify the activity exists
        let exists = self.conn.query_row(
            "SELECT 1 FROM activities WHERE id = ?1",
            [activity_id],
            |_| Ok(true)
        ).optional().map_err(|e| ActivityError::DataError {
            message: format!("Failed to check activity existence: {}", e),
        })?.unwrap_or(false);

        if !exists {
            return Ok(Vec::new());
        }

        let mut stmt = self.conn.prepare(
            "SELECT tag FROM activity_tags WHERE activity_id = ?1",
        ).map_err(|e| ActivityError::RetrievalError {
            message: format!("Failed to prepare statement: {}", e),
        })?;

        let tags = stmt.query_map([activity_id], |row| row.get(0))
            .map_err(|e| ActivityError::RetrievalError {
                message: format!("Failed to execute query: {}", e),
            })?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| ActivityError::DataError {
                message: format!("Failed to process rows: {}", e),
            })?;

        Ok(tags)
    }

    /// Get activities by tag with optional limit
    pub fn get_activities_by_tag(&self, tag: &str, limit: Option<u32>, sort_direction: Option<SortDirection>) -> Result<Vec<Activity>, ActivityError> {
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

        let mut stmt = self.conn.prepare(&sql).map_err(|e| ActivityError::RetrievalError {
            message: format!("Failed to prepare statement: {}", e),
        })?;

        let rows = match stmt.query_map([tag], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }) {
            Ok(rows) => rows,
            Err(e) => return Err(ActivityError::RetrievalError {
                message: format!("Failed to execute query: {}", e),
            })
        };

        let mut activities = Vec::new();
        for row in rows {
            let (id, _) = row.map_err(|e| ActivityError::DataError {
                message: format!("Failed to process row: {}", e),
            })?;

            if let Some(activity) = self.get_activity_by_id(&id)? {
                activities.push(activity);
            }
        }

        Ok(activities)
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
            SortDirection::Desc => "DESC"
        }
    }
}