use arc_swap::*;
use crate::connection::*;
use crate::migrations::*;
use crate::interner::*;
use crate::serializable::*;
use serde_bytes::ByteBuf;
use static_events::prelude_async::*;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;
use sylphie_core::derives::*;
use sylphie_core::prelude::*;
use std::hash::Hash;

static CONFIG_MIGRATIONS: MigrationData = MigrationData {
    migration_id: "config 7c8f3471-5ef7-4a8a-9388-36ea9e512a57",
    migration_set_name: "config",
    is_transient: false,
    target_version: 1,
    scripts: &[
        migration_script!(0, 1, "sql/config_0_to_1.sql"),
    ],
};
pub(crate) fn init_config(target: &Handler<impl Events>) -> Result<()> {
    CONFIG_MIGRATIONS.execute_sync(target)?;
    Ok(())
}

#[derive(Copy, Clone)]
pub struct ConfigKey<V: DbSerializable>(PhantomData<V>);

#[derive(Events, Default)]
pub struct ConfigManager {

}
#[module_impl]
impl ConfigManager {

}
