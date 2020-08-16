#![feature(const_type_name, const_fn)]

#[macro_use] extern crate tracing;

pub mod migrations; // this goes early because there are macros we use in here

pub mod config;
mod interner;
pub mod connection;
pub mod kvs;
pub mod serializable;

use std::fs;
use sylphie_core::core::{EarlyInitEvent, BotInfo};
use sylphie_core::derives::*;
use sylphie_core::interface::SetupLoggerEvent;
use sylphie_core::prelude::*;

/// The event called to initialize the database.
pub struct InitDbEvent(());
failable_event!(InitDbEvent, (), Error);

/// The module that handles database connections and migrations.
///
/// This should be a part of the module tree for database connections and migrations to work
/// correctly.
#[derive(Events)]
pub struct DatabaseModule {
    #[service] #[subhandler] config: config::ConfigManager,
    #[service] interner: interner::StringInterner,
    #[service] database: connection::Database,
    #[service] migrations: migrations::MigrationManager,
}
impl DatabaseModule {
    pub fn new() -> Self {
        let database = connection::Database::new();
        DatabaseModule {
            config: Default::default(),
            interner: Default::default(),
            database: database.clone(),
            migrations: migrations::MigrationManager::new(database),
        }
    }
}
#[events_impl]
impl DatabaseModule {
    #[event_handler(EvInit)]
    fn init_database(target: &Handler<impl Events>, _: &EarlyInitEvent) {
        if let Err(e) = target.dispatch_sync(InitDbEvent(())) {
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
    fn init_serializers(target: &Handler<impl Events>, _: &InitDbEvent) -> Result<()> {
        crate::interner::init_interner(target)?;
        crate::kvs::init_kvs(target)?;
        crate::config::init_config(target)?;
        Ok(())
    }

    #[event_handler]
    fn setup_logger(ev: &mut SetupLoggerEvent) {
        ev.add_console_directive("sylphie_database=debug");
    }
}