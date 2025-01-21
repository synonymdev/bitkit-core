use rust_blocktank_client::{IBtInfo};
use crate::modules::blocktank::{BlocktankDB, BlocktankError};

impl BlocktankDB {
    /// Fetches service information from Blocktank and stores it in the database.
    /// Returns the fetched information if successful.
    pub async fn fetch_and_store_info(&self) -> Result<IBtInfo, BlocktankError> {
        let info = self.client.get_info().await.map_err(|e| BlocktankError::DataError {
            error_details: format!("Failed to fetch info from Blocktank: {}", e)
        })?;

        self.upsert_info(&info).await?;
        Ok(info)
    }
}