use crate::connection::{DbConnection, TransactionType, Database};
use parking_lot::Mutex;
use std::sync::Arc;
use sylphie_core::errors::*;
use rusqlite::TransactionBehavior;

/// Stores the data for a given migration.
#[derive(Copy, Clone, Debug)]
pub struct MigrationScriptData {
    /// The schema version this script migrates from.
    ///
    /// Note that version `0` represents a newly initialized database with no tables at all.
    pub from: u32,
    /// The schema version this script migrates to.
    ///
    /// Note that version `0` represents a newly initialized database with no tables at all.
    pub to: u32,
    /// The name of the migration script.
    pub script_name: &'static str,
    /// The migration script to run.
    pub script_data: &'static str,
}

/// Stores the data for a given set of migrations.
#[derive(Copy, Clone, Debug)]
pub struct MigrationData {
    /// The name of the migration set. This should not change, or else migrations will not be
    /// tracked correctly.
    pub migration_set_name: &'static str,
    /// Whether this migration set is for the transient database.
    pub is_transient: bool,
    /// The current schema version.
    pub target_version: usize,
    /// A list of
    pub debug_info: &'static [MigrationScriptData],
}

pub struct MigrationManager {
    pool: Database,
    data: Mutex<MigrationManagerState>,
}
impl MigrationManager {
    pub(in super) fn new(pool: Database) -> Self {
        MigrationManager {
            pool,
            data: Mutex::new(MigrationManagerState {
                tables_created: false,
            }),
        }
    }

    pub fn execute_migration(&self, migration: &MigrationData) -> Result<()> {
        todo!()
    }
}

struct MigrationManagerState {
    tables_created: bool,
}
impl MigrationManagerState {
    async fn create_migrations_table(&mut self, conn: &mut DbConnection) -> Result<()> {
        if !self.tables_created {
            conn.execute_batch(create_migrations_table_sql(false)).await?;
            conn.execute_batch(create_migrations_table_sql(true)).await?;
            self.tables_created = true;
        }
        Ok(())
    }

    async fn execute_migration(
        &mut self, conn: &mut DbConnection, migration: &MigrationData
    ) -> Result<()> {
        self.create_migrations_table(conn).await?;

        let transaction = conn.transaction_with_type(TransactionType::Exclusive).await?;

        transaction.commit().await?;

        Ok(())
    }

}
fn create_migrations_table_sql(is_transient: bool) -> String {
    format!(
        r"
            CREATE TABLE IF NOT EXISTS {}sylphie_db_migrations_tracking(
                migration_name TEXT NOT NULL PRIMARY KEY,
                current_version INTEGER NOT NULL
            ) WITHOUT ROWID;
        ",
        if is_transient { "transient." } else { "" },
    )
}