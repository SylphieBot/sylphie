#[macro_use] extern crate tracing;

pub mod migrations; // this goes early because there are macros we use in here

pub mod connection;
pub mod kvs;
pub mod serializable;

use std::path::PathBuf;
use sylphie_core::core::EarlyInitEvent;
use sylphie_core::derives::*;
use sylphie_core::prelude::*;

/// The module that handles database connections and migrations.
///
/// This should be a part of the module tree for database connections and migrations to work
/// correctly.
#[derive(Events)]
pub struct DatabaseModule {
    #[service] database: connection::Database,
    #[service] migrations: migrations::MigrationManager,
}
impl DatabaseModule {
    pub fn new(path: PathBuf, transient_path: PathBuf) -> Result<Self> {
        let database = connection::Database::new(path, transient_path)?;
        Ok(DatabaseModule {
            database: database.clone(),
            migrations: migrations::MigrationManager::new(database),
        })
    }
}
#[events_impl]
impl DatabaseModule {
    #[event_handler(EvInit)]
    fn init_database(target: &Handler<impl Events>, _: &EarlyInitEvent) {
        crate::kvs::init_kvs(target);
    }
}