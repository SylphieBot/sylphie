use arc_swap::*;
use crate::connection::*;
use crate::migrations::*;
use crate::interner::*;
use crate::serializable::*;
use serde_bytes::ByteBuf;
use static_events::prelude_async::*;
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;
use sylphie_core::derives::*;
use sylphie_core::prelude::*;

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
pub struct ConfigKey<V: ConfigType> {
    id: TypeId,
    phantom: PhantomData<V>
}
impl <V: ConfigType> ConfigKey<V> {
    pub const fn new(id: TypeId) -> Self {
        ConfigKey { id, phantom: PhantomData }
    }
}

/// Creates a new config option.
#[macro_export]
macro_rules! config_option_ff344e40783a4f25b33f98135991d80f {
    () => {{
        enum LocalType { }
        let type_id = core::any::TypeId::of::<LocalType>();
        $crate::config::ConfigKey::new(type_id)
    }};
}

#[doc(inline)]
pub use crate::{config_option_ff344e40783a4f25b33f98135991d80f as config_option};

/// A type that can be used for configuration.
pub trait ConfigType: DbSerializable {
    /// Displays this config option in user-readable text.
    fn ui_fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result;

    /// Returns a [`Display`] that shows this config type to the user.
    fn display(&self) -> DisplayConfigType<'_, Self> {
        DisplayConfigType { config: self }
    }

    /// Parses this config option from text.
    fn ui_parse(text: &str) -> Result<Self>;
}

/// A type used to display configuration text.
pub struct DisplayConfigType<'a, T: ConfigType> {
    config: &'a T,
}
impl <'a, T: ConfigType> fmt::Display for DisplayConfigType<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.config.ui_fmt(f)
    }
}

#[derive(Events, Default)]
pub struct ConfigManager {

}
#[module_impl]
impl ConfigManager {

}
