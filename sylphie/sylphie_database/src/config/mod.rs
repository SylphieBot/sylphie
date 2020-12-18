use arc_swap::ArcSwapOption;
use async_trait::*;
use crate::connection::*;
use crate::migrations::*;
use crate::interner::*;
use crate::serializable::*;
use enumset::*;
use fxhash::{FxHashMap, FxHashSet};
use static_events::prelude_async::*;
use std::any::{Any, TypeId};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;
use sylphie_core::derives::*;
use sylphie_core::prelude::*;
use sylphie_utils::cache::LruCache;
use sylphie_utils::disambiguate::*;
use sylphie_utils::locks::LockSet;
use sylphie_utils::scopes::Scope;
use tokio::runtime::Handle;

mod impls;

static CONFIG_MIGRATIONS: MigrationData = MigrationData {
    migration_id: "config 7c8f3471-5ef7-4a8a-9388-36ea9e512a57",
    migration_set_name: "config",
    is_transient: false,
    target_version: 1,
    scripts: &[
        migration_script!(0, 1, "config_0_to_1.sql"),
    ],
};
pub(crate) fn init_config(target: &Handler<impl Events>) -> Result<()> {
    CONFIG_MIGRATIONS.execute_sync(target)?;
    let handle = Handle::current();
    handle.block_on(target.get_service::<ConfigManager>().reload(target))?;
    Ok(())
}

/// Configuration flags for config options.
#[derive(EnumSetType, Debug)]
pub enum ConfigFlag {
    /// This configuration option can be set in the global scope.
    Global,
    /// This configuration option can be set in a connection scope.
    Connection,
    /// This configuration option can be set in a server scope.
    ///
    /// A server is something like a Discord guild or an IRC channel that can be expected to be
    /// managed by a single moderation team.
    Server,
    /// This configuration option can be set in a category scope.
    Category,
    /// This configuration option can be set in a channel scope.
    ///
    /// Note that on platforms like IRC, a channel and a server are the same thing.
    Channel,

    /// This configuration option can be set in any scope.
    Any,
}

pub struct ConfigKey<V: ConfigType>(&'static __macro_priv::ConfigKeyData<V>);
impl <V: ConfigType> Clone for ConfigKey<V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl <V: ConfigType> Copy for ConfigKey<V> { }

// Internal items used by the [`config_option!`] macro. Not public API.
#[doc(hidden)]
pub mod __macro_priv {
    use super::*;
    pub use enumset;

    #[derive(Copy, Clone)]
    pub struct ConfigKeyData<V: ConfigType> {
        pub(crate) id: TypeId,
        pub(crate) default_value: fn() -> V,
        pub(crate) phantom: PhantomData<V>,
        pub(crate) storage_name: &'static str,
        pub(crate) flags: EnumSet<ConfigFlag>,
    }

    pub const fn new_0<V: ConfigType>(
        name: &'static str, id: TypeId, default: fn() -> V, flags: EnumSet<ConfigFlag>,
    ) -> ConfigKeyData<V> {
        ConfigKeyData {
            id, default_value: default, phantom: PhantomData, storage_name: name, flags,
        }
    }
    pub const fn new_1<V: ConfigType>(key: &'static ConfigKeyData<V>) -> ConfigKey<V> {
        ConfigKey(key)
    }
    pub const fn type_id<A: Any>() -> TypeId {
        TypeId::of::<A>()
    }
}

/// Creates a new config option.
#[macro_export]
macro_rules! config_option_ff344e40783a4f25b33f98135991d80f {
    ($($value:path)|* $(|)*, $storage_name:expr $(,)?) => {{
        $crate::config_option_ff344e40783a4f25b33f98135991d80f!(
            $($value|)*, $storage_name, || core::default::Default::default(),
        )
    }};
    ($($value:path)|* $(|)*, $storage_name:expr, $default:expr $(,)?) => {{
        use $crate::config::__macro_priv::enumset::enum_set;
        use $crate::config::ConfigFlag;
        use $crate::config::ConfigFlag::*;

        enum LocalType { }
        $crate::config::__macro_priv::new_1(
            &$crate::config::__macro_priv::new_0(
                $storage_name,
                $crate::config::__macro_priv::type_id::<LocalType>(),
                $default,
                enum_set!($($value|)*),
            ),
        )
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

#[async_trait]
trait DynConfigType: Send + Sync + 'static {
    async fn get_display<'a>(
        &'a self, target: &'a (dyn Any + Send + Sync + 'static), scope: Scope,
    ) -> Result<String>;
    async fn set_parse<'a>(
        &'a self, target: &'a (dyn Any + Send + Sync + 'static), scope: Scope, value: &'a str,
    ) -> Result<()>;
    async fn remove<'a>(
        &'a self, target: &'a (dyn Any + Send + Sync + 'static), scope: Scope,
    ) -> Result<()>;
}

struct DynConfigKey<E: Events, T: ConfigType>(ConfigKey<T>, PhantomData<E>);
impl <E: Events, T: ConfigType> DynConfigKey<E, T> {
    fn new(_target: &Handler<E>, key: ConfigKey<T>) -> Self {
        DynConfigKey(key, PhantomData)
    }
}
#[async_trait]
impl <E: Events, T: ConfigType> DynConfigType for DynConfigKey<E, T> {
    async fn get_display<'a>(
        &'a self, target: &'a (dyn Any + Send + Sync + 'static), scope: Scope,
    ) -> Result<String> {
        let target = target.downcast_ref::<Handler<E>>().expect("Wrong Handler type passed.");
        let manager = target.get_service::<ConfigManager>();
        let val = manager.get(target, scope, self.0).await?;
        Ok(val.display().to_string())
    }
    async fn set_parse<'a>(
        &'a self, target: &'a (dyn Any + Send + Sync + 'static), scope: Scope, value: &'a str,
    ) -> Result<()> {
        let target = target.downcast_ref::<Handler<E>>().expect("Wrong Handler type passed.");
        let manager = target.get_service::<ConfigManager>();
        let parsed = T::ui_parse(value)?;
        manager.set(target, scope, self.0, parsed).await?;
        Ok(())
    }
    async fn remove<'a>(
        &'a self, target: &'a (dyn Any + Send + Sync + 'static), scope: Scope,
    ) -> Result<()> {
        let target = target.downcast_ref::<Handler<E>>().expect("Wrong Handler type passed.");
        let manager = target.get_service::<ConfigManager>();
        manager.remove(target, scope, self.0).await
    }
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

/// A dynamically loaded configuration key used for configuration purposes.
pub struct RegisteredConfig {
    entry_names: Vec<EntryName>,
    id: TypeId,
    db_id: StringId,
    dyn_config: Box<dyn DynConfigType>,
}
impl RegisteredConfig {
    pub async fn get_display(
        &self, target: &Handler<impl Events>, scope: Scope,
    ) -> Result<String> {
        self.dyn_config.get_display(target, scope).await
    }
    pub async fn set_parse(
        &self, target: &Handler<impl Events>, scope: Scope, val: &str,
    ) -> Result<()> {
        self.dyn_config.set_parse(target, scope, val).await
    }
    pub async fn remove(&self, target: &Handler<impl Events>, scope: Scope) -> Result<()> {
        self.dyn_config.remove(target, scope).await
    }
}
impl <T: ConfigType> PartialEq<ConfigKey<T>> for RegisteredConfig {
    fn eq(&self, other: &ConfigKey<T>) -> bool {
        self.id == other.0.id
    }
}
impl <T: ConfigType> PartialEq<RegisteredConfig> for ConfigKey<T> {
    fn eq(&self, other: &RegisteredConfig) -> bool {
        self.0.id == other.id
    }
}

/// The event used to register configuration options.
pub struct RegisterConfigEvent {
    configs: FxHashMap<TypeId, RegisteredConfig>,
    tid_for_storage: FxHashMap<&'static str, TypeId>,
    found_names: FxHashSet<Arc<str>>,
}
failable_self_event!(RegisterConfigEvent, Error);
impl RegisterConfigEvent {
    /// Registers a new configuration option.
    pub async fn register_config<'a, V: ConfigType>(
        &'a mut self, target: &'a Handler<impl Events>,
        module: &'a ModuleInfo, name: &'a str, key: &'static ConfigKey<V>,
    ) -> Result<()> {
        let entry_name = EntryName::new(module.name(), name);

        if self.found_names.contains(&*entry_name.lc_name) {
            error!("Duplicate configuration option '{}'.", entry_name.full_name);
            bail!("Duplicate configuration option.");
        }
        self.found_names.insert(entry_name.lc_name.clone());

        if let Some(tid) = self.tid_for_storage.get(&key.0.storage_name) {
            if *tid != key.0.id {
                error!(
                    "Different underlying IDs found for storage name '{}'.",
                    key.0.storage_name,
                );
                bail!("Different underlying IDs found for the same storage ID.");
            }
        }
        self.tid_for_storage.insert(key.0.storage_name, key.0.id);

        let db_id = StringId::intern(target, key.0.storage_name).await?;
        if let Some(config) = self.configs.get_mut(&key.0.id) {
            if config.db_id == db_id {
                warn!(
                    "Duplicate configuration option for storage name '{}'. Both configuration \
                     options will map to the same underlying value.",
                    key.0.storage_name,
                );
            } else {
                bail!("Impossible: Different storage IDs for same underlying ID.");
            }
            config.entry_names.push(entry_name);
        } else {
            self.configs.insert(key.0.id, RegisteredConfig {
                entry_names: vec![entry_name],
                id: key.0.id,
                db_id,
                dyn_config: Box::new(DynConfigKey::new(target, *key)),
            });
        }

        Ok(())
    }
}

pub struct ConfigurationChangedEvent {
    pub scope: Scope,
    pub config_key: RegisteredConfig,
}
failable_event!(ConfigurationChangedEvent, (), Error);

struct ConfigManagerData {
    disambiguate: DisambiguatedSet<Arc<RegisteredConfig>>,
}
impl ConfigManagerData {
    fn from_event(ev: RegisterConfigEvent) -> Self {
        let mut vec = Vec::new();
        for (_, config) in ev.configs {
            let config = Arc::new(config);
            for entry in &config.entry_names {
                vec.push((entry.clone(), config.clone(), config.db_id));
            }
        }
        ConfigManagerData {
            disambiguate: DisambiguatedSet::new_aliased("config option", vec),
        }
    }
}

/// The result of a config option lookup.
pub type ConfigLookupResult = LookupResult<Arc<RegisteredConfig>>;

#[derive(Events)]
#[service]
pub struct ConfigManager {
    locks: LockSet<(ScopeId, TypeId)>,
    cache: LruCache<(ScopeId, TypeId), Option<Arc<dyn Any + Send + Sync>>>,
    options: ArcSwapOption<ConfigManagerData>,
}
#[module_impl]
impl ConfigManager {
    pub async fn get<'a, T: ConfigType>(
        &'a self, target: &'a Handler<impl Events>, scope: Scope, key: ConfigKey<T>,
    ) -> Result<T> {
        let scope = ScopeId::intern(target, scope).await?;
        let val = self.cache.cached_async((scope, key.0.id), async {
            let mut conn = target.connect_db().await?;
            let res: Option<(Option<SerializeValue>, Option<u64>, Option<u32>)> = conn.query_row(
                "SELECT val, val_schema_id, val_schema_version FROM sylphie_db_configuration \
                 WHERE scope = ? AND key_id = ?;",
                (scope, StringId::intern(target, key.0.storage_name).await?),
            ).await?;
            if let Some((Some(data), Some(id), Some(version))) = res {
                let id_name = target.get_service::<Interner>().lock().get_ser_id_rev(id);
                if &*id_name == T::ID && version == T::SCHEMA_VERSION {
                    Ok(Some(
                        Arc::new(T::Format::deserialize(data)?)
                            as Arc<dyn Any + Send + Sync>,
                    ))
                } else if T::can_migrate_from(&id_name, version) {
                    Ok(Some(
                        Arc::new(T::do_migration(&id_name, version, data)?)
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
            Ok((key.0.default_value)())
        } else {
            Ok(val.unwrap().downcast_ref::<T>().unwrap().clone())
        }
    }

    pub async fn set<'a, T: ConfigType>(
        &'a self, target: &'a Handler<impl Events>, scope: Scope, key: ConfigKey<T>, value: T,
    ) -> Result<()> {
        let scope = ScopeId::intern(target, scope).await?;
        self.set_0(
            target, scope, key.0.id, key.0.storage_name,
            T::Format::serialize(&value)?, T::ID, T::SCHEMA_VERSION,
        ).await?;
        self.cache.insert((scope, key.0.id), Some(Arc::new(value)));
        Ok(())
    }
    async fn set_0<'a>(
        &'a self, target: &'a Handler<impl Events>, scope: ScopeId,
        id: TypeId, storage: &'a str,
        value: SerializeValue, schema_id: &'static str, schema_ver: u32,
    ) -> Result<()> {
        let _guard = self.locks.lock((scope, id)).await;
        let mut conn = target.connect_db().await?;
        conn.execute(
            "INSERT INTO sylphie_db_configuration \
             (scope, key_id, val, val_schema_id, val_schema_version) \
             VALUES (?, ?, ?, ?, ?)",
            (
                scope,
                StringId::intern(target, storage).await?,
                value,
                target.get_service::<Interner>().lock().get_ser_id(schema_id),
                schema_ver,
            ),
        ).await?;
        Ok(())
    }

    pub async fn remove<'a, T: ConfigType>(
        &'a self, target: &'a Handler<impl Events>, scope: Scope, key: ConfigKey<T>,
    ) -> Result<()> {
        self.remove_0(target, scope, key.0.id, key.0.storage_name).await
    }
    async fn remove_0<'a>(
        &'a self, target: &'a Handler<impl Events>, scope: Scope,
        id: TypeId, storage: &'a str,
    ) -> Result<()> {
        let scope = ScopeId::intern(target, scope).await?;
        let _guard = self.locks.lock((scope, id)).await;
        let mut conn = target.connect_db().await?;
        conn.execute(
            "DELETE FROM sylphie_db_configuration WHERE scope = ? AND key_id = ?;",
            (scope, StringId::intern(target, storage).await?),
        ).await?;
        self.cache.invalidate(&(scope.clone(), id));
        Ok(())
    }

    /// Reloads the config manager.
    pub async fn reload(&self, target: &Handler<impl Events>) -> Result<()> {
        let new_set = ConfigManagerData::from_event(target.dispatch_async(RegisterConfigEvent {
            configs: Default::default(),
            tid_for_storage: Default::default(),
            found_names: Default::default()
        }).await?);
        self.options.store(Some(Arc::new(new_set)));
        Ok(())
    }

    /// Returns a list of all options currently registered.
    pub fn option_list(&self) -> Arc<[Disambiguated<Arc<RegisteredConfig>>]> {
        self.options.load().as_ref()
            .map(|x| x.disambiguate.list_arc())
            .expect("Config manager is not loaded.")
    }

    /// Looks ups a config option.
    pub async fn lookup_option(&self, option: &str) -> Result<ConfigLookupResult> {
        let options = self.options.load();
        let options = options.as_ref().cmd_error(|| "Config manager is not loaded.")?;

        let mut valid_options = Vec::new();
        for option in options.disambiguate.resolve_iter(option)? {
            valid_options.push(option.value.clone());
        }
        Ok(ConfigLookupResult::new(valid_options))
    }
}
impl Default for ConfigManager {
    fn default() -> Self {
        ConfigManager {
            locks: LockSet::new(),
            cache: LruCache::new(1024),
            options: ArcSwapOption::new(None),
        }
    }
}