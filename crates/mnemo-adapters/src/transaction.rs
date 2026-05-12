use async_trait::async_trait;
use mnemo_ports::{DurableBundle, StorageError, TransactionContext};

use crate::store::SqliteStore;

pub struct SqliteTransaction;

impl TransactionContext for SqliteTransaction {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[async_trait]
impl DurableBundle for SqliteStore {
    async fn begin(&self) -> Result<Box<dyn TransactionContext>, StorageError> {
        Ok(Box::new(SqliteTransaction))
    }
    async fn commit(&self, _tx: Box<dyn TransactionContext>) -> Result<(), StorageError> {
        Ok(())
    }
    async fn rollback(&self, _tx: Box<dyn TransactionContext>) -> Result<(), StorageError> {
        Ok(())
    }
}
