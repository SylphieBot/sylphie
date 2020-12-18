#![feature(const_type_name, const_fn, const_fn_fn_ptr_basics, const_type_id)]

#[macro_use] extern crate tracing;

pub mod migrations; // this goes early because there are macros we use in here

pub mod config;
mod interner;
pub mod connection;
pub mod kvs;
pub mod serializable;
pub mod singleton;

/// Contains misc types that involve the database.
///
/// These are merged into `sylphie::utils` in the wrapper library.
pub mod utils {
    pub use crate::interner::{ScopeId, StringId};
}

use std::fs;
use sylphie_core::core::{EarlyInitEvent, BotInfo};
use sylphie_core::derives::*;
use sylphie_core::interface::SetupLoggerEvent;
use sylphie_core::prelude::*;
use tokio::runtime::Handle;

/// The event called to initialize the database.
pub struct InitDbEvent(());
failable_event!(InitDbEvent, (), Error);

#[derive(Events)]
struct InnerHandler {
    #[service] #[subhandler] config: config::ConfigManager,
    #[service] interner: interner::Interner,
    #[service] database: connection::Database,
    #[service] migrations: migrations::MigrationManager,
}
impl InnerHandler {
    fn new() -> Self {
        let database = connection::Database::new();
        InnerHandler {
            config: Default::default(),
            interner: Default::default(),
            database: database.clone(),
            migrations: migrations::MigrationManager::new(database),
        }
    }
}

/// The module that handles database connections and migrations.
///
/// This should be a part of the module tree for database connections and migrations to work
/// correctly.
#[derive(Module)]
pub struct DatabaseModule {
    #[module_info] info: ModuleInfo,
    #[subhandler] #[init_with { InnerHandler::new() }] inner: InnerHandler,
    #[submodule] #[service] store: singleton::SingletonDataStore,
}
#[module_impl]
impl DatabaseModule {
    #[event_handler(EvInit)]
    fn init_database(&self, target: &Handler<impl Events>, _: &EarlyInitEvent) {
        let handle = Handle::current();
        if let Err(e) = handle.block_on(self.early_init_db(target)) {
            e.report_error();
            panic!("Error occurred during early database initialization.");
        }
        if let Err(e) = handle.block_on(target.dispatch_async(InitDbEvent(()))) {
            e.report_error();
            panic!("Error occurred during database initialization.");
        }
    }

    async fn early_init_db(&self, target: &Handler<impl Events>) -> Result<()> {
        self.init_db_paths(target)?;
        self.init_serializers(target).await?;
        Ok(())
    }

    fn init_db_paths(&self, target: &Handler<impl Events>) -> Result<()> {
        let info = target.get_service::<BotInfo>();

        let mut db_path = info.root_path().to_owned();
        db_path.push("db");
        fs::create_dir_all(&db_path)?;

        let mut persistent_path = db_path.to_owned();
        persistent_path.push(format!("{}.db", info.bot_name()));

        let mut transient_path = db_path.to_owned();
        transient_path.push(format!("{}.transient.db", info.bot_name()));

        self.inner.database.set_paths(persistent_path, transient_path);
        Ok(())
    }

    async fn init_serializers(&self, target: &Handler<impl Events>) -> Result<()> {
        crate::interner::init_interner(target).await?;
        crate::kvs::init_kvs(target).await?;
        crate::config::init_config(target).await?;
        Ok(())
    }

    #[event_handler]
    fn setup_logger(ev: &mut SetupLoggerEvent) {
        ev.add_console_directive("sylphie_database=debug");
    }
}