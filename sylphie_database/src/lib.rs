#![feature(const_type_name)]

#[macro_use] extern crate tracing;

pub mod migrations; // this goes early because there are macros we use in here

pub mod config;
pub mod connection;
pub mod kvs;
mod schema_id;
pub mod serializable;

use std::fs;
use sylphie_core::core::{EarlyInitEvent, BotInfo};
use sylphie_core::derives::*;
use sylphie_core::interface::SetupLoggerEvent;
use sylphie_core::prelude::*;
use tokio::runtime::Handle;

/// The event called to initialize the database.
pub struct InitDbEvent(());
failable_event!(InitDbEvent, (), Error);

/// The module that handles database connections and migrations.
///
/// This should be a part of the module tree for database connections and migrations to work
/// correctly.
#[derive(Events)]
pub struct DatabaseModule {
    #[service] database: connection::Database,
    #[service] migrations: migrations::MigrationManager,
    #[service] kvs_cache: schema_id::SchemaCache,
}
impl DatabaseModule {
    pub fn new() -> Self {
        let database = connection::Database::new();
        DatabaseModule {
            database: database.clone(),
            migrations: migrations::MigrationManager::new(database),
            kvs_cache: Default::default(),
        }
    }
}
#[events_impl]
impl DatabaseModule {
    #[event_handler(EvInit)]
    fn init_database(target: &Handler<impl Events>, _: &EarlyInitEvent) {
        if let Err(e) = Handle::current().block_on(target.dispatch_async(InitDbEvent(()))) {
            e.report_error();
            panic!("Error occurred during early database initialization.");
        }
    }

    #[event_handler(EvInit)]
    fn init_db_paths(&self, target: &Handler<impl Events>, _: &InitDbEvent) -> Result<()> {
        let info = target.get_service::<BotInfo>();

        let mut db_path = info.root_path().to_owned();
        db_path.push("db");
        fs::create_dir_all(&db_path)?;

        let mut persistent_path = db_path.to_owned();
        persistent_path.push(format!("{}.db", info.bot_name()));

        let mut transient_path = db_path.to_owned();
        transient_path.push(format!("{}.transient.db", info.bot_name()));

        self.database.set_paths(persistent_path, transient_path);
        Ok(())
    }

    #[event_handler]
    async fn init_serializers(target: &Handler<impl Events>, _: &InitDbEvent) -> Result<()> {
        crate::schema_id::init_schema_cache(target).await?;
        crate::kvs::init_kvs(target).await?;
        Ok(())
    }

    #[event_handler]
    fn setup_logger(ev: &mut SetupLoggerEvent) {
        ev.add_console_directive("sylphie_database=debug");
    }
}