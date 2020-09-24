use crate::connection::*;
use parking_lot::Mutex;
use static_events::prelude_async::*;
use std::collections::HashMap;
use std::sync::Arc;
use sylphie_core::errors::*;
use tokio::runtime::Handle;

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
    /// The name of the migration set that is stored internally in the database.
    ///
    /// As this should not change, and should not conflict with any other migration set, this
    /// should contain something unique such as an UUID.
    pub migration_id: &'static str,
    /// The name of the migration set that is disabled to the user.
    pub migration_set_name: &'static str,
    /// Whether this migration set is for the transient database.
    pub is_transient: bool,
    /// The current schema version.
    pub target_version: u32,
    /// A list of migrations for this migration set.
    ///
    /// Each migration is checked in order, and if the current version matches the current version,
    /// it will be applied. Therefore, scripts should be sorted in the order you want them to be
    /// applied in.
    pub scripts: &'static [MigrationScriptData],
}
impl MigrationData {
    pub async fn execute(&'static self, target: &Handler<impl Events>) -> Result<()> {
        target.get_service::<MigrationManager>().execute_migration(self).await
    }
    pub fn execute_sync(&'static self, target: &Handler<impl Events>) -> Result<()> {
        target.get_service::<MigrationManager>().execute_migration_sync(self)
    }
}

/// Defines a migration script.
#[macro_export]
macro_rules! migration_script_ff344e40783a4f25b33f98135991d80f {
    ($from:expr, $to:expr, $source:expr $(,)?) => {
        $crate::migrations::MigrationScriptData {
            from: $from,
            to: $to,
            script_name: $source,
            script_data: include_str!($source),
        }
    };
}

#[doc(inline)]
pub use crate::{migration_script_ff344e40783a4f25b33f98135991d80f as migration_script};

pub struct MigrationManager {
    pool: Database,
    data: Arc<Mutex<MigrationManagerState>>,
}
impl MigrationManager {
    pub(in super) fn new(pool: Database) -> Self {
        MigrationManager {
            pool,
            data: Arc::new(Mutex::new(MigrationManagerState {
                tables_created: false,
                repeat_transaction_watch: HashMap::new(),
            })),
        }
    }

    pub async fn execute_migration(&self, migration: &'static MigrationData) -> Result<()> {
        let pool = self.pool.clone();
        let data = self.data.clone();
        Handle::current().spawn_blocking(move || -> Result<()> {
            let mut connection = pool.connect_sync()?;
            data.lock().execute_migration(&mut connection, migration)?;
            Ok(())
        }).await?
    }

    pub fn execute_migration_sync(&self, migration: &'static MigrationData) -> Result<()> {
        let mut connection = self.pool.connect_sync()?;
        self.data.lock().execute_migration(&mut connection, migration)?;
        Ok(())
    }
}

struct MigrationManagerState {
    tables_created: bool,
    repeat_transaction_watch: HashMap<&'static str, &'static MigrationData>,
}
impl MigrationManagerState {
    fn create_migrations_table(&mut self, conn: &mut DbSyncConnection) -> Result<()> {
        if !self.tables_created {
            conn.execute_batch(create_migrations_table_sql(false))?;
            conn.execute_batch(create_migrations_table_sql(true))?;
            self.tables_created = true;
        }
        Ok(())
    }

    fn execute_migration(
        &mut self, conn: &mut DbSyncConnection, migration: &'static MigrationData
    ) -> Result<()> {
        self.create_migrations_table(conn)?;
        if let Some(data) = self.repeat_transaction_watch.get(&migration.migration_id) {
            let data_off = data as *const _ as usize;
            let migration_off = migration as *const _ as usize;
            if data_off == migration_off {
                warn!(
                    "Migration set {} has been executed more than once!",
                    migration.migration_id,
                );
            } else {
                warn!(
                    "Migration set id {} conflicts! ({} at 0x{:x}, {} at 0x{:x})",
                    migration.migration_id, migration.migration_set_name, migration_off,
                    data.migration_set_name, data_off,
                )
            }
        }

        trace!("Running migration set {}", migration.migration_set_name);

        let mut transaction = conn.transaction_with_type(TransactionType::Exclusive)?;
        let start_version: u32 = transaction.query_row(
            query_migrations_table_sql(migration.is_transient),
            migration.migration_id,
        )?.unwrap_or(0);
        let mut current_version = start_version;
        for script in migration.scripts {
            if current_version == script.from {
                debug!(
                    "Running migration {}/{}",
                    migration.migration_set_name,
                    script.script_name.rsplit('/').next().unwrap(),
                );
                transaction.execute_batch(script.script_data)?;
                transaction.execute(
                    replace_migrations_table_sql(migration.is_transient),
                    (migration.migration_id, script.to),
                )?;
                current_version = script.to;
            }
        }
        if migration.target_version != current_version {
            error!(
                "Could not apply migration {} to version {}. (got from {} to {})",
                migration.migration_set_name, migration.target_version,
                start_version, current_version,
            );
            bail!("Could not successfully apply migration.");
        }
        transaction.commit()?;

        self.repeat_transaction_watch.insert(migration.migration_id, migration);

        Ok(())
    }
}
fn create_migrations_table_sql(is_transient: bool) -> String {
    format!(
        "\
            CREATE TABLE IF NOT EXISTS {}sylphie_db_migrations_tracking ( \
                migration_name TEXT NOT NULL PRIMARY KEY, \
                current_version INTEGER NOT NULL \
            ) WITHOUT ROWID; \
        ",
        if is_transient { "transient." } else { "" },
    )
}
fn query_migrations_table_sql(is_transient: bool) -> String {
    format!(
        "\
            SELECT current_version FROM {}sylphie_db_migrations_tracking \
                WHERE migration_name = ?; \
        ",
        if is_transient { "transient." } else { "" },
    )
}
fn replace_migrations_table_sql(is_transient: bool) -> String {
    format!(
        "\
            REPLACE INTO {}sylphie_db_migrations_tracking \
                (migration_name, current_version) \
                VALUES(?, ?); \
        ",
        if is_transient { "transient." } else { "" },
    )
}