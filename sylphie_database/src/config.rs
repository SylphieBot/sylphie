use crate::connection::*;
use crate::migrations::*;
use crate::interner::*;
use crate::serializable::*;
use serde_bytes::ByteBuf;
use static_events::prelude_async::*;
use std::any::{Any, TypeId};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;
use sylphie_core::derives::*;
use sylphie_core::prelude::*;
use sylphie_utils::cache::LruCache;
use sylphie_utils::locks::LockSet;
use sylphie_utils::strings::InternString;
use sylphie_utils::scopes::Scope;

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
    storage_name: &'static str,
    default_value: fn() -> V,
    phantom: PhantomData<V>,
}
impl <V: ConfigType> ConfigKey<V> {
    pub const fn new(
        storage_name: &'static str, id: TypeId, default: fn() -> V,
    ) -> Self {
        ConfigKey { storage_name, id, default_value: default, phantom: PhantomData }
    }
}

/// Creates a new config option.
#[macro_export]
macro_rules! config_option_ff344e40783a4f25b33f98135991d80f {
    ($storage_name:expr $(,)?) => {{
        $crate::config_option_ff344e40783a4f25b33f98135991d80f!(
            $storage_name, || core::default::Default::default(),
        )
    }};
    ($storage_name:expr, $default:expr $(,)?) => {{
        enum LocalType { }
        let type_id = core::any::TypeId::of::<LocalType>();
        $crate::config::ConfigKey::new($storage_name, type_id, $default)
    }};
}

#[doc(inline)]
pub use crate::{config_option_ff344e40783a4f25b33f98135991d80f as config_option};

/// A type that can be used for configuration.
pub trait ConfigType: DbSerializable + Any {
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

#[derive(Events)]
#[service]
pub struct ConfigManager {
    locks: LockSet<(Scope, TypeId)>,
    cache: LruCache<(Scope, TypeId), Option<Arc<dyn Any + Send + Sync>>>,
}
#[module_impl]
impl ConfigManager {
    pub async fn get<'a, T: ConfigType>(
        &'a self, target: &'a Handler<impl Events>, scope: Scope, key: &'static ConfigKey<T>,
    ) -> Result<T> {
        let scope = scope.intern();
        let val = self.cache.cached_async((scope.clone(), key.id), async {
            let mut conn = target.connect_db().await?;
            let result: Option<(Option<ByteBuf>, Option<u32>, Option<u32>)> = conn.query_row(
                "SELECT val, val_schema_id, val_schema_version FROM sylphie_db_configuration \
                 WHERE scope = ? AND key_id = ?;",
                (ByteBuf::from(BincodeFormat::serialize(&scope)?), key.storage_name),
            ).await?;
            if let Some((Some(data), Some(id), Some(version))) = result {
                let id_name = target.get_service::<StringInterner>().lock().lookup_id(id);
                if &*id_name == T::ID && version == T::SCHEMA_VERSION {
                    Ok(Some(
                        Arc::new(T::Format::deserialize(&data)?)
                            as Arc<dyn Any + Send + Sync>,
                    ))
                } else if T::can_migrate_from(&id_name, version) {
                    Ok(Some(
                        Arc::new(T::do_migration(&id_name, version, &data)?)
                            as Arc<dyn Any + Send + Sync>,
                    ))
                } else {
                    bail!("Cannot migrate configuration option properly.")
                }
            } else {
                Ok(None)
            }
        }).await?;
        if val.is_none() {
            Ok((key.default_value)())
        } else {
            Ok(val.unwrap().downcast_ref::<T>().unwrap().clone())
        }
    }
    pub async fn set<'a, T: ConfigType>(
        &'a self, target: &'a Handler<impl Events>, scope: Scope,
        key: &'static ConfigKey<T>, value: T,
    ) -> Result<()> {
        let scope = scope.intern();
        let _guard = self.locks.lock((scope.clone(), key.id)).await;
        let mut conn = target.connect_db().await?;
        conn.execute(
            "INSERT INTO sylphie_db_configuration \
             (scope, key_id, val, val_schema_id, val_schema_version),\
             VALUES (?, ?, ?, ?, ?)",
            (
                ByteBuf::from(BincodeFormat::serialize(&scope)?),
                key.storage_name,
                ByteBuf::from(T::Format::serialize(&value)?),
                target.get_service::<StringInterner>().lock().lookup_name(T::ID),
                T::SCHEMA_VERSION,
            ),
        ).await?;
        self.cache.insert((scope.clone(), key.id), Some(Arc::new(value)));
        Ok(())
    }
    pub async fn remove<'a, T: ConfigType>(
        &'a self, target: &'a Handler<impl Events>, scope: Scope, key: &'static ConfigKey<T>,
    ) -> Result<()> {
        let scope = scope.intern();
        let _guard = self.locks.lock((scope.clone(), key.id)).await;
        let mut conn = target.connect_db().await?;
        conn.execute(
            "DELETE FROM sylphie_db_configuration \
             WHERE scope = ? AND key_id = ?;",
            (ByteBuf::from(BincodeFormat::serialize(&scope)?), key.storage_name),
        ).await?;
        self.cache.invalidate(&(scope.clone(), key.id));
        Ok(())
    }
}
impl Default for ConfigManager {
    fn default() -> Self {
        ConfigManager {
            locks: LockSet::new(),
            cache: LruCache::new(1024),
        }
    }
}