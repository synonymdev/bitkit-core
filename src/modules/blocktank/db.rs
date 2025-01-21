use rusqlite::{Connection, OptionalExtension};
use rust_blocktank_client::*;
use tokio::sync::Mutex;
use std::result::Result;
use crate::modules::blocktank::{BlocktankDB, BlocktankError};
use crate::modules::blocktank::models::*;
pub const DEFAULT_BLOCKTANK_URL: &str = "https://api1.blocktank.to/api";

impl BlocktankDB {
    pub async fn new(db_path: &str, blocktank_url: Option<&str>) -> Result<BlocktankDB, BlocktankError> {
        let conn = Connection::open(db_path).map_err(|e| BlocktankError::InitializationError {
            error_details: format!("Error opening database: {}", e),
        })?;

        let client = BlocktankClient::new(blocktank_url.unwrap_or(DEFAULT_BLOCKTANK_URL))
            .map_err(|e| BlocktankError::InitializationError {
                error_details: format!("Failed to initialize Blocktank client: {}", e),
            })?;

        let db = BlocktankDB {
            conn: Mutex::new(conn),
            client,
        };
        db.initialize().await?;
        Ok(db)
    }

    async fn initialize(&self) -> Result<(), BlocktankError> {
        let conn = self.conn.lock().await;

        // Create enum tables
        for create_stmt in CREATE_ENUM_TABLES {
            conn.execute(create_stmt, []).map_err(|e| BlocktankError::InitializationError {
                error_details: format!("Failed to create enum table: {}", e),
            })?;
        }

        // Create main tables
        conn.execute(CREATE_ORDERS_TABLE, []).map_err(|e| BlocktankError::InitializationError {
            error_details: format!("Failed to create orders table: {}", e),
        })?;

        conn.execute(CREATE_INFO_TABLE, []).map_err(|e| BlocktankError::InitializationError {
            error_details: format!("Failed to create info table: {}", e),
        })?;

        conn.execute(CREATE_CJIT_ENTRIES_TABLE, []).map_err(|e| BlocktankError::InitializationError {
            error_details: format!("Failed to create CJIT entries table: {}", e),
        })?;

        // Create triggers
        for trigger_stmt in TRIGGER_STATEMENTS {
            conn.execute(trigger_stmt, []).map_err(|e| BlocktankError::InitializationError {
                error_details: format!("Failed to create trigger: {}", e),
            })?;
        }

        // Create indexes
        for index_stmt in INDEX_STATEMENTS {
            conn.execute(index_stmt, []).map_err(|e| BlocktankError::InitializationError {
                error_details: format!("Failed to create index: {}", e),
            })?;
        }

        Ok(())
    }

    /// Updates the BlocktankClient URL.
    pub async fn update_blocktank_url(&mut self, new_url: &str) -> Result<(), BlocktankError> {
        // Validate the new URL (optional but recommended)
        if new_url.is_empty() {
            return Err(BlocktankError::InitializationError {
                error_details: "The new Blocktank URL cannot be empty.".to_string(),
            });
        }

        // Attempt to create a new BlocktankClient with the new URL
        let new_client = BlocktankClient::new(new_url).map_err(|e| BlocktankError::InitializationError {
            error_details: format!("Failed to initialize Blocktank client with the new URL: {}", e),
        })?;

        // Update the client instance
        self.client = new_client;

        Ok(())
    }

    pub async fn upsert_info(&self, info: &IBtInfo) -> Result<(), BlocktankError> {
        let conn = self.conn.lock().await;

        // Convert complex objects to JSON strings
        let nodes_json = serde_json::to_string(&info.nodes).map_err(|e| BlocktankError::SerializationError {
            error_details: format!("Failed to serialize nodes: {}", e),
        })?;

        let options_json = serde_json::to_string(&info.options).map_err(|e| BlocktankError::SerializationError {
            error_details: format!("Failed to serialize options: {}", e),
        })?;

        let versions_json = serde_json::to_string(&info.versions).map_err(|e| BlocktankError::SerializationError {
            error_details: format!("Failed to serialize versions: {}", e),
        })?;

        let onchain_json = serde_json::to_string(&info.onchain).map_err(|e| BlocktankError::SerializationError {
            error_details: format!("Failed to serialize onchain: {}", e),
        })?;

        // Mark the current version as not current before inserting new version
        conn.execute(
            "UPDATE info SET is_current = 0 WHERE is_current = 1",
            [],
        ).map_err(|e| BlocktankError::DatabaseError {
            error_details: format!("Failed to update existing info records: {}", e),
        })?;

        // Insert or replace the new info
        conn.execute(
            "INSERT OR REPLACE INTO info (
            version, nodes, options, versions, onchain, is_current
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, 1
        )",
            (
                &info.version,
                &nodes_json,
                &options_json,
                &versions_json,
                &onchain_json,
            ),
        ).map_err(|e| BlocktankError::InsertError {
            error_details: format!("Failed to insert info: {}", e),
        })?;

        Ok(())
    }

    /// Retrieves the current service information from the database
    pub async fn get_info(&self) -> Result<Option<IBtInfo>, BlocktankError> {
        let conn = self.conn.lock().await;

        let result = conn.query_row(
            "SELECT version, nodes, options, versions, onchain
             FROM info
             WHERE is_current = 1",
            [],
            |row| {
                let version: i32 = row.get(0)?;
                let nodes_json: String = row.get(1)?;
                let options_json: String = row.get(2)?;
                let versions_json: String = row.get(3)?;
                let onchain_json: String = row.get(4)?;

                let nodes: Vec<ILspNode> = serde_json::from_str(&nodes_json)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    ))?;

                let options: IBtInfoOptions = serde_json::from_str(&options_json)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    ))?;

                let versions: IBtInfoVersions = serde_json::from_str(&versions_json)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    ))?;

                let onchain: IBtInfoOnchain = serde_json::from_str(&onchain_json)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    ))?;

                Ok(IBtInfo {
                    version,
                    nodes,
                    options,
                    versions,
                    onchain,
                })
            }
        ).optional().map_err(|e| BlocktankError::DataError {
            error_details: format!("Failed to fetch info from database: {}", e),
        })?;

        Ok(result)
    }
}