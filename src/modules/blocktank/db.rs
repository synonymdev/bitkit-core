use crate::modules::blocktank::models::*;
use crate::modules::blocktank::{BlocktankDB, BlocktankError};
use rusqlite::{Connection, OptionalExtension};
use rust_blocktank_client::*;
use std::result::Result;
use tokio::sync::Mutex;
pub const DEFAULT_BLOCKTANK_URL: &str = "https://api1.blocktank.to/api";

impl BlocktankDB {
    pub async fn new(
        db_path: &str,
        blocktank_url: Option<&str>,
    ) -> Result<BlocktankDB, BlocktankError> {
        let conn = Connection::open(db_path).map_err(|e| BlocktankError::InitializationError {
            error_details: format!("Error opening database: {}", e),
        })?;

        let url = blocktank_url.unwrap_or(DEFAULT_BLOCKTANK_URL);
        let client =
            BlocktankClient::new(Some(url)).map_err(|e| BlocktankError::InitializationError {
                error_details: format!("Failed to initialize Blocktank client: {}", e),
            })?;

        let db = BlocktankDB {
            conn: Mutex::new(conn),
            client,
            blocktank_url: url.to_string(),
        };
        db.initialize().await?;
        Ok(db)
    }

    async fn initialize(&self) -> Result<(), BlocktankError> {
        let conn = self.conn.lock().await;

        // Create enum tables
        for create_stmt in CREATE_ENUM_TABLES {
            conn.execute(create_stmt, [])
                .map_err(|e| BlocktankError::InitializationError {
                    error_details: format!("Failed to create enum table: {}", e),
                })?;
        }

        // Create main tables
        conn.execute(CREATE_ORDERS_TABLE, [])
            .map_err(|e| BlocktankError::InitializationError {
                error_details: format!("Failed to create orders table: {}", e),
            })?;

        conn.execute(CREATE_INFO_TABLE, [])
            .map_err(|e| BlocktankError::InitializationError {
                error_details: format!("Failed to create info table: {}", e),
            })?;

        conn.execute(CREATE_CJIT_ENTRIES_TABLE, []).map_err(|e| {
            BlocktankError::InitializationError {
                error_details: format!("Failed to create CJIT entries table: {}", e),
            }
        })?;

        // Populate enum tables
        // Order states
        for state in ["Created", "Expired", "Open", "Closed"] {
            conn.execute(
                "INSERT OR IGNORE INTO order_states (state, description) VALUES (?1, ?1)",
                [state],
            )
            .map_err(|e| BlocktankError::InitializationError {
                error_details: format!("Failed to insert order state {}: {}", state, e),
            })?;
        }

        // Payment states
        for state in [
            "Created",
            "PartiallyPaid",
            "Paid",
            "Refunded",
            "RefundAvailable",
        ] {
            conn.execute(
                "INSERT OR IGNORE INTO payment_states (state, description) VALUES (?1, ?1)",
                [state],
            )
            .map_err(|e| BlocktankError::InitializationError {
                error_details: format!("Failed to insert payment state {}: {}", state, e),
            })?;
        }

        // CJIT states
        for state in ["Created", "Completed", "Expired", "Failed"] {
            conn.execute(
                "INSERT OR IGNORE INTO cjit_states (state, description) VALUES (?1, ?1)",
                [state],
            )
            .map_err(|e| BlocktankError::InitializationError {
                error_details: format!("Failed to insert cjit state {}: {}", state, e),
            })?;
        }

        // Create triggers
        for trigger_stmt in TRIGGER_STATEMENTS {
            conn.execute(trigger_stmt, [])
                .map_err(|e| BlocktankError::InitializationError {
                    error_details: format!("Failed to create trigger: {}", e),
                })?;
        }

        // Create indexes
        for index_stmt in INDEX_STATEMENTS {
            conn.execute(index_stmt, [])
                .map_err(|e| BlocktankError::InitializationError {
                    error_details: format!("Failed to create index: {}", e),
                })?;
        }

        Ok(())
    }

    /// Updates the BlocktankClient URL.
    pub async fn update_blocktank_url(&mut self, new_url: &str) -> Result<(), BlocktankError> {
        // Validate the new URL
        if new_url.is_empty() {
            return Err(BlocktankError::InitializationError {
                error_details: "The new Blocktank URL cannot be empty.".to_string(),
            });
        }

        // Attempt to create a new BlocktankClient with the new URL
        let new_client = BlocktankClient::new(Some(new_url)).map_err(|e| {
            BlocktankError::InitializationError {
                error_details: format!(
                    "Failed to initialize Blocktank client with the new URL: {}",
                    e
                ),
            }
        })?;

        // Update both the client and URL
        self.client = new_client;
        self.blocktank_url = new_url.to_string();

        Ok(())
    }

    pub async fn upsert_info(&self, info: &IBtInfo) -> Result<(), BlocktankError> {
        let conn = self.conn.lock().await;

        let nodes_json =
            serde_json::to_string(&info.nodes).map_err(|e| BlocktankError::SerializationError {
                error_details: format!("Failed to serialize nodes: {}", e),
            })?;

        let options_json = serde_json::to_string(&info.options).map_err(|e| {
            BlocktankError::SerializationError {
                error_details: format!("Failed to serialize options: {}", e),
            }
        })?;

        let versions_json = serde_json::to_string(&info.versions).map_err(|e| {
            BlocktankError::SerializationError {
                error_details: format!("Failed to serialize versions: {}", e),
            }
        })?;

        let onchain_json = serde_json::to_string(&info.onchain).map_err(|e| {
            BlocktankError::SerializationError {
                error_details: format!("Failed to serialize onchain: {}", e),
            }
        })?;

        conn.execute("UPDATE info SET is_current = 0 WHERE is_current = 1", [])
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to update existing info records: {}", e),
            })?;

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
        )
        .map_err(|e| BlocktankError::InsertError {
            error_details: format!("Failed to insert info: {}", e),
        })?;

        Ok(())
    }

    /// Retrieves the current service information from the database
    pub async fn get_info(&self) -> Result<Option<IBtInfo>, BlocktankError> {
        let conn = self.conn.lock().await;

        let result = conn
            .query_row(
                "SELECT version, nodes, options, versions, onchain
             FROM info
             WHERE is_current = 1",
                [],
                |row| {
                    let version: u32 = row.get(0)?;
                    let nodes_json: String = row.get(1)?;
                    let options_json: String = row.get(2)?;
                    let versions_json: String = row.get(3)?;
                    let onchain_json: String = row.get(4)?;

                    let nodes: Vec<ILspNode> = serde_json::from_str(&nodes_json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                    let options: IBtInfoOptions =
                        serde_json::from_str(&options_json).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?;

                    let versions: IBtInfoVersions =
                        serde_json::from_str(&versions_json).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?;

                    let onchain: IBtInfoOnchain =
                        serde_json::from_str(&onchain_json).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?;

                    Ok(IBtInfo {
                        version,
                        nodes,
                        options,
                        versions,
                        onchain,
                    })
                },
            )
            .optional()
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to fetch info from database: {}", e),
            })?;

        Ok(result)
    }

    pub async fn upsert_order(&self, order: &IBtOrder) -> Result<(), BlocktankError> {
        let conn = self.conn.lock().await;

        let params = Self::build_order_params(order)?;

        let mut stmt =
            conn.prepare(INSERT_ORDER_SQL)
                .map_err(|e| BlocktankError::DatabaseError {
                    error_details: format!("Failed to prepare statement: {}", e),
                })?;

        stmt.execute(rusqlite::params![
            params.id,
            params.state,
            params.state2,
            params.fee_sat,
            params.network_fee_sat,
            params.service_fee_sat,
            params.lsp_balance_sat,
            params.client_balance_sat,
            params.zero_conf,
            params.zero_reserve,
            params.client_node_id,
            params.channel_expiry_weeks,
            params.channel_expires_at,
            params.order_expires_at,
            params.lnurl,
            params.coupon_code,
            params.source,
            params.channel_json,
            params.lsp_node_json,
            params.payment_json,
            params.discount_json,
            params.updated_at,
            params.created_at,
        ])
        .map_err(|e| BlocktankError::InsertError {
            error_details: format!("Failed to insert order: {}", e),
        })?;

        Ok(())
    }

    pub async fn upsert_orders(&self, orders: &[IBtOrder]) -> Result<(), BlocktankError> {
        let mut conn = self.conn.lock().await;
        let tx = conn
            .transaction()
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        {
            let mut stmt =
                tx.prepare(INSERT_ORDER_SQL)
                    .map_err(|e| BlocktankError::DatabaseError {
                        error_details: format!("Failed to prepare statement: {}", e),
                    })?;

            for order in orders {
                let params = Self::build_order_params(order)?;

                stmt.execute(rusqlite::params![
                    params.id,
                    params.state,
                    params.state2,
                    params.fee_sat,
                    params.network_fee_sat,
                    params.service_fee_sat,
                    params.lsp_balance_sat,
                    params.client_balance_sat,
                    params.zero_conf,
                    params.zero_reserve,
                    params.client_node_id,
                    params.channel_expiry_weeks,
                    params.channel_expires_at,
                    params.order_expires_at,
                    params.lnurl,
                    params.coupon_code,
                    params.source,
                    params.channel_json,
                    params.lsp_node_json,
                    params.payment_json,
                    params.discount_json,
                    params.updated_at,
                    params.created_at,
                ])
                .map_err(|e| BlocktankError::InsertError {
                    error_details: format!("Failed to insert order {}: {}", params.id, e),
                })?;
            }
        }

        tx.commit().map_err(|e| BlocktankError::DatabaseError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    fn build_order_params(order: &IBtOrder) -> Result<OrderInsertParams, BlocktankError> {
        let channel_json = if let Some(channel) = &order.channel {
            Some(serde_json::to_string(channel).map_err(|e| {
                BlocktankError::SerializationError {
                    error_details: format!("Failed to serialize channel: {}", e),
                }
            })?)
        } else {
            None
        };

        let lsp_node_json = serde_json::to_string(&order.lsp_node).map_err(|e| {
            BlocktankError::SerializationError {
                error_details: format!("Failed to serialize lsp_node: {}", e),
            }
        })?;

        let payment_json = serde_json::to_string(&order.payment).map_err(|e| {
            BlocktankError::SerializationError {
                error_details: format!("Failed to serialize payment: {}", e),
            }
        })?;

        let discount_json = if let Some(discount) = &order.discount {
            Some(serde_json::to_string(discount).map_err(|e| {
                BlocktankError::SerializationError {
                    error_details: format!("Failed to serialize discount: {}", e),
                }
            })?)
        } else {
            None
        };

        Ok(OrderInsertParams {
            id: order.id.clone(),
            state: format!("{:?}", order.state),
            state2: order
                .state2
                .as_ref()
                .map(|s| format!("{:?}", s))
                .unwrap_or_else(|| "".to_string()),
            fee_sat: order.fee_sat,
            network_fee_sat: order.network_fee_sat,
            service_fee_sat: order.service_fee_sat,
            lsp_balance_sat: order.lsp_balance_sat,
            client_balance_sat: order.client_balance_sat,
            zero_conf: order.zero_conf,
            zero_reserve: order.zero_reserve,
            client_node_id: order.client_node_id.clone(),
            channel_expiry_weeks: order.channel_expiry_weeks,
            channel_expires_at: order.channel_expires_at.clone(),
            order_expires_at: order.order_expires_at.clone(),
            lnurl: order.lnurl.clone(),
            coupon_code: order.coupon_code.clone(),
            source: order.source.clone(),
            channel_json,
            lsp_node_json,
            payment_json,
            discount_json,
            updated_at: order.updated_at.clone(),
            created_at: order.created_at.clone(),
        })
    }

    fn build_cjit_params(entry: &ICJitEntry) -> Result<CJitInsertParams, BlocktankError> {
        let channel_json = if let Some(channel) = &entry.channel {
            Some(serde_json::to_string(channel).map_err(|e| {
                BlocktankError::SerializationError {
                    error_details: format!("Failed to serialize channel: {}", e),
                }
            })?)
        } else {
            None
        };

        let invoice_json = serde_json::to_string(&entry.invoice).map_err(|e| {
            BlocktankError::SerializationError {
                error_details: format!("Failed to serialize invoice: {}", e),
            }
        })?;

        let lsp_node_json = serde_json::to_string(&entry.lsp_node).map_err(|e| {
            BlocktankError::SerializationError {
                error_details: format!("Failed to serialize lsp_node: {}", e),
            }
        })?;

        let discount_json = if let Some(discount) = &entry.discount {
            Some(serde_json::to_string(discount).map_err(|e| {
                BlocktankError::SerializationError {
                    error_details: format!("Failed to serialize discount: {}", e),
                }
            })?)
        } else {
            None
        };

        Ok(CJitInsertParams {
            id: entry.id.clone(),
            state: format!("{:?}", entry.state),
            fee_sat: entry.fee_sat,
            network_fee_sat: entry.network_fee_sat,
            service_fee_sat: entry.service_fee_sat,
            channel_size_sat: entry.channel_size_sat,
            channel_expiry_weeks: entry.channel_expiry_weeks,
            channel_open_error: entry.channel_open_error.clone(),
            node_id: entry.node_id.clone(),
            coupon_code: entry.coupon_code.clone(),
            source: entry.source.clone(),
            expires_at: entry.expires_at.clone(),
            invoice_json,
            channel_json,
            lsp_node_json,
            discount_json,
            updated_at: entry.updated_at.clone(),
            created_at: entry.created_at.clone(),
        })
    }

    pub async fn get_orders(
        &self,
        order_ids: Option<&[String]>,
        filter: Option<BtOrderState2>,
    ) -> Result<Vec<IBtOrder>, BlocktankError> {
        let conn = self.conn.lock().await;

        let mut query = String::from(
            "SELECT id, state, state2, fee_sat, network_fee_sat, service_fee_sat,
                    lsp_balance_sat, client_balance_sat, zero_conf, zero_reserve,
                    client_node_id, channel_expiry_weeks, channel_expires_at,
                    order_expires_at, lnurl, coupon_code, source, channel_data,
                    lsp_node_data, payment_data, discount_data, updated_at, created_at
             FROM orders WHERE 1=1",
        );

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ids) = order_ids {
            query.push_str(" AND id IN (");
            query.push_str(
                &std::iter::repeat("?")
                    .take(ids.len())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            query.push(')');
            params.extend(
                ids.iter()
                    .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::ToSql>),
            );
        }

        if let Some(state) = filter {
            query.push_str(" AND state2 = ?");
            params.push(Box::new(format!("{:?}", state)));
        }

        query.push_str(" ORDER BY created_at DESC");

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let orders = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                let channel_json: Option<String> = row.get(17)?;
                let lsp_node_json: String = row.get(18)?;
                let payment_json: String = row.get(19)?;
                let discount_json: Option<String> = row.get(20)?;

                let channel = if let Some(json) = channel_json {
                    Some(serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?)
                } else {
                    None
                };

                let lsp_node: ILspNode = serde_json::from_str(&lsp_node_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                let payment: IBtPayment = serde_json::from_str(&payment_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                let discount = if let Some(json) = discount_json {
                    Some(serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?)
                } else {
                    None
                };

                Ok(IBtOrder {
                    id: row.get(0)?,
                    state: row.get::<_, String>(1)?.parse().unwrap(),
                    state2: {
                        let state2_str: String = row.get(2)?;
                        if state2_str.is_empty() {
                            None
                        } else {
                            Some(state2_str.parse().unwrap())
                        }
                    },
                    fee_sat: row.get(3)?,
                    network_fee_sat: row.get(4)?,
                    service_fee_sat: row.get(5)?,
                    lsp_balance_sat: row.get(6)?,
                    client_balance_sat: row.get(7)?,
                    zero_conf: row.get(8)?,
                    zero_reserve: row.get(9)?,
                    client_node_id: row.get(10)?,
                    channel_expiry_weeks: row.get(11)?,
                    channel_expires_at: row.get(12)?,
                    order_expires_at: row.get(13)?,
                    channel,
                    lsp_node: Some(lsp_node),
                    lnurl: row.get(14)?,
                    payment: Some(payment),
                    coupon_code: row.get(15)?,
                    source: row.get(16)?,
                    discount,
                    updated_at: row.get(21)?,
                    created_at: row.get(22)?,
                })
            })
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to execute query: {}", e),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to process results: {}", e),
            })?;

        Ok(orders)
    }

    pub async fn get_active_orders(&self) -> Result<Vec<IBtOrder>, BlocktankError> {
        let conn = self.conn.lock().await;

        let query = String::from(
            "SELECT id, state, state2, fee_sat, network_fee_sat, service_fee_sat,
                lsp_balance_sat, client_balance_sat, zero_conf, zero_reserve,
                client_node_id, channel_expiry_weeks, channel_expires_at,
                order_expires_at, lnurl, coupon_code, source, channel_data,
                lsp_node_data, payment_data, discount_data, updated_at, created_at
         FROM orders
         WHERE state2 IN ('Created', 'Paid')
         ORDER BY created_at DESC",
        );

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let orders = stmt
            .query_map([], |row| {
                let channel_json: Option<String> = row.get(17)?;
                let lsp_node_json: String = row.get(18)?;
                let payment_json: String = row.get(19)?;
                let discount_json: Option<String> = row.get(20)?;

                let channel = if let Some(json) = channel_json {
                    Some(serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?)
                } else {
                    None
                };

                let lsp_node: ILspNode = serde_json::from_str(&lsp_node_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                let payment: IBtPayment = serde_json::from_str(&payment_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                let discount = if let Some(json) = discount_json {
                    Some(serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?)
                } else {
                    None
                };

                Ok(IBtOrder {
                    id: row.get(0)?,
                    state: row.get::<_, String>(1)?.parse().unwrap(),
                    state2: {
                        let state2_str: String = row.get(2)?;
                        if state2_str.is_empty() {
                            None
                        } else {
                            Some(state2_str.parse().unwrap())
                        }
                    },
                    fee_sat: row.get(3)?,
                    network_fee_sat: row.get(4)?,
                    service_fee_sat: row.get(5)?,
                    lsp_balance_sat: row.get(6)?,
                    client_balance_sat: row.get(7)?,
                    zero_conf: row.get(8)?,
                    zero_reserve: row.get(9)?,
                    client_node_id: row.get(10)?,
                    channel_expiry_weeks: row.get(11)?,
                    channel_expires_at: row.get(12)?,
                    order_expires_at: row.get(13)?,
                    channel,
                    lsp_node: Some(lsp_node),
                    lnurl: row.get(14)?,
                    payment: Some(payment),
                    coupon_code: row.get(15)?,
                    source: row.get(16)?,
                    discount,
                    updated_at: row.get(21)?,
                    created_at: row.get(22)?,
                })
            })
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to execute query: {}", e),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to process results: {}", e),
            })?;

        Ok(orders)
    }

    pub async fn upsert_cjit_entry(&self, entry: &ICJitEntry) -> Result<(), BlocktankError> {
        let conn = self.conn.lock().await;

        let params = Self::build_cjit_params(entry)?;

        let mut stmt =
            conn.prepare(INSERT_CJIT_SQL)
                .map_err(|e| BlocktankError::DatabaseError {
                    error_details: format!("Failed to prepare statement: {}", e),
                })?;

        stmt.execute(rusqlite::params![
            params.id,
            params.state,
            params.fee_sat,
            params.network_fee_sat,
            params.service_fee_sat,
            params.channel_size_sat,
            params.channel_expiry_weeks,
            params.channel_open_error,
            params.node_id,
            params.coupon_code,
            params.source,
            params.expires_at,
            params.invoice_json,
            params.channel_json,
            params.lsp_node_json,
            params.discount_json,
            params.updated_at,
            params.created_at,
        ])
        .map_err(|e| BlocktankError::InsertError {
            error_details: format!("Failed to insert CJIT entry: {}", e),
        })?;

        Ok(())
    }

    pub async fn upsert_cjit_entries(&self, entries: &[ICJitEntry]) -> Result<(), BlocktankError> {
        let mut conn = self.conn.lock().await;
        let tx = conn
            .transaction()
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        {
            let mut stmt =
                tx.prepare(INSERT_CJIT_SQL)
                    .map_err(|e| BlocktankError::DatabaseError {
                        error_details: format!("Failed to prepare statement: {}", e),
                    })?;

            for entry in entries {
                let params = Self::build_cjit_params(entry)?;

                stmt.execute(rusqlite::params![
                    params.id,
                    params.state,
                    params.fee_sat,
                    params.network_fee_sat,
                    params.service_fee_sat,
                    params.channel_size_sat,
                    params.channel_expiry_weeks,
                    params.channel_open_error,
                    params.node_id,
                    params.coupon_code,
                    params.source,
                    params.expires_at,
                    params.invoice_json,
                    params.channel_json,
                    params.lsp_node_json,
                    params.discount_json,
                    params.updated_at,
                    params.created_at,
                ])
                .map_err(|e| BlocktankError::InsertError {
                    error_details: format!("Failed to insert CJIT entry {}: {}", params.id, e),
                })?;
            }
        }

        tx.commit().map_err(|e| BlocktankError::DatabaseError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }

    pub async fn get_cjit_entries(
        &self,
        entry_ids: Option<&[String]>,
        filter: Option<CJitStateEnum>,
    ) -> Result<Vec<ICJitEntry>, BlocktankError> {
        let conn = self.conn.lock().await;

        let mut query = String::from(
            "SELECT id, state, fee_sat, network_fee_sat, service_fee_sat,
                channel_size_sat, channel_expiry_weeks, channel_open_error,
                node_id, coupon_code, source, expires_at, invoice_data,
                channel_data, lsp_node_data, discount_data,
                updated_at, created_at
         FROM cjit_entries WHERE 1=1",
        );

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ids) = entry_ids {
            query.push_str(" AND id IN (");
            query.push_str(
                &std::iter::repeat("?")
                    .take(ids.len())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            query.push(')');
            params.extend(
                ids.iter()
                    .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::ToSql>),
            );
        }

        if let Some(state) = filter {
            query.push_str(" AND state = ?");
            params.push(Box::new(format!("{:?}", state)));
        }

        query.push_str(" ORDER BY created_at DESC");

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let entries = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                let invoice_json: String = row.get(12)?;
                let channel_json: Option<String> = row.get(13)?;
                let lsp_node_json: String = row.get(14)?;
                let discount_json: Option<String> = row.get(15)?;

                let invoice: IBtBolt11Invoice =
                    serde_json::from_str(&invoice_json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let channel = if let Some(json) = channel_json {
                    Some(serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?)
                } else {
                    None
                };

                let lsp_node: ILspNode = serde_json::from_str(&lsp_node_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                let discount = if let Some(json) = discount_json {
                    Some(serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?)
                } else {
                    None
                };

                Ok(ICJitEntry {
                    id: row.get(0)?,
                    state: row
                        .get::<_, String>(1)?
                        .parse::<CJitStateEnum>()
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                1,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?,
                    fee_sat: row.get(2)?,
                    network_fee_sat: row.get(3)?,
                    service_fee_sat: row.get(4)?,
                    channel_size_sat: row.get(5)?,
                    channel_expiry_weeks: row.get(6)?,
                    channel_open_error: row.get(7)?,
                    node_id: row.get(8)?,
                    coupon_code: row.get(9)?,
                    source: row.get(10)?,
                    expires_at: row.get(11)?,
                    invoice,
                    channel,
                    lsp_node,
                    discount,
                    updated_at: row.get(16)?,
                    created_at: row.get(17)?,
                })
            })
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to execute query: {}", e),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to process results: {}", e),
            })?;

        Ok(entries)
    }

    pub async fn get_active_cjit_entries(&self) -> Result<Vec<ICJitEntry>, BlocktankError> {
        let conn = self.conn.lock().await;

        let query = String::from(
            "SELECT id, state, fee_sat, network_fee_sat, service_fee_sat,
                channel_size_sat, channel_expiry_weeks, channel_open_error,
                node_id, coupon_code, source, expires_at, invoice_data,
                channel_data, lsp_node_data, discount_data,
                updated_at, created_at
             FROM cjit_entries
             WHERE state IN ('Created', 'Failed')
             ORDER BY created_at DESC",
        );

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to prepare statement: {}", e),
            })?;

        let entries = stmt
            .query_map([], |row| {
                let invoice_json: String = row.get(12)?;
                let channel_json: Option<String> = row.get(13)?;
                let lsp_node_json: String = row.get(14)?;
                let discount_json: Option<String> = row.get(15)?;

                let invoice: IBtBolt11Invoice =
                    serde_json::from_str(&invoice_json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let channel = if let Some(json) = channel_json {
                    Some(serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?)
                } else {
                    None
                };

                let lsp_node: ILspNode = serde_json::from_str(&lsp_node_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                let discount = if let Some(json) = discount_json {
                    Some(serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?)
                } else {
                    None
                };

                Ok(ICJitEntry {
                    id: row.get(0)?,
                    state: row
                        .get::<_, String>(1)?
                        .parse::<CJitStateEnum>()
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                1,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?,
                    fee_sat: row.get(2)?,
                    network_fee_sat: row.get(3)?,
                    service_fee_sat: row.get(4)?,
                    channel_size_sat: row.get(5)?,
                    channel_expiry_weeks: row.get(6)?,
                    channel_open_error: row.get(7)?,
                    node_id: row.get(8)?,
                    coupon_code: row.get(9)?,
                    source: row.get(10)?,
                    expires_at: row.get(11)?,
                    invoice,
                    channel,
                    lsp_node,
                    discount,
                    updated_at: row.get(16)?,
                    created_at: row.get(17)?,
                })
            })
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to execute query: {}", e),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to process results: {}", e),
            })?;

        Ok(entries)
    }

    /// Removes all orders from the database
    pub async fn remove_all_orders(&self) -> Result<(), BlocktankError> {
        let conn = self.conn.lock().await;

        conn.execute("DELETE FROM orders", [])
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to delete all orders: {}", e),
            })?;

        Ok(())
    }

    /// Removes all CJIT entries from the database
    pub async fn remove_all_cjit_entries(&self) -> Result<(), BlocktankError> {
        let conn = self.conn.lock().await;

        conn.execute("DELETE FROM cjit_entries", []).map_err(|e| {
            BlocktankError::DatabaseError {
                error_details: format!("Failed to delete all CJIT entries: {}", e),
            }
        })?;

        Ok(())
    }

    /// Removes all data from all Blocktank tables
    ///
    /// This wipes:
    /// - All orders
    /// - All CJIT entries
    /// - All info entries
    ///
    /// Note: This does NOT delete the enum state tables (order_states, payment_states, cjit_states)
    /// as these contain static reference data that should persist across wipes.
    pub async fn wipe_all(&self) -> Result<(), BlocktankError> {
        let mut conn = self.conn.lock().await;

        let tx = conn
            .transaction()
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to start transaction: {}", e),
            })?;

        tx.execute("DELETE FROM orders", [])
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to delete orders: {}", e),
            })?;

        tx.execute("DELETE FROM cjit_entries", [])
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to delete CJIT entries: {}", e),
            })?;

        tx.execute("DELETE FROM info", [])
            .map_err(|e| BlocktankError::DatabaseError {
                error_details: format!("Failed to delete info entries: {}", e),
            })?;

        tx.commit().map_err(|e| BlocktankError::DatabaseError {
            error_details: format!("Failed to commit transaction: {}", e),
        })?;

        Ok(())
    }
}

struct OrderInsertParams {
    id: String,
    state: String,
    state2: String,
    fee_sat: u64,
    network_fee_sat: u64,
    service_fee_sat: u64,
    lsp_balance_sat: u64,
    client_balance_sat: u64,
    zero_conf: bool,
    zero_reserve: bool,
    client_node_id: Option<String>,
    channel_expiry_weeks: u32,
    channel_expires_at: String,
    order_expires_at: String,
    lnurl: Option<String>,
    coupon_code: Option<String>,
    source: Option<String>,
    channel_json: Option<String>,
    lsp_node_json: String,
    payment_json: String,
    discount_json: Option<String>,
    updated_at: String,
    created_at: String,
}

struct CJitInsertParams {
    id: String,
    state: String,
    fee_sat: u64,
    network_fee_sat: u64,
    service_fee_sat: u64,
    channel_size_sat: u64,
    channel_expiry_weeks: u32,
    channel_open_error: Option<String>,
    node_id: String,
    coupon_code: String,
    source: Option<String>,
    expires_at: String,
    invoice_json: String,
    channel_json: Option<String>,
    lsp_node_json: String,
    discount_json: Option<String>,
    updated_at: String,
    created_at: String,
}
