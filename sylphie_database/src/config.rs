use arc_swap::ArcSwapOption;
use async_trait::*;
use crate::connection::*;
use crate::migrations::*;
use crate::interner::*;
use crate::serializable::*;
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
use sylphie_utils::strings::InternString;
use tokio::runtime::Handle;

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
    let handle = Handle::current();
    handle.block_on(target.get_service::<ConfigManager>().reload(target))?;
    Ok(())
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

    #[derive(Copy, Clone)]
    pub struct ConfigKeyData<V: ConfigType> {
        pub(crate) id: TypeId,
        pub(crate) default_value: fn() -> V,
        pub(crate) phantom: PhantomData<V>,
        pub(crate) storage_name: &'static str,
    }

    pub const fn new_0<V: ConfigType>(
        name: &'static str, id: TypeId, default: fn() -> V,
    ) -> ConfigKeyData<V> {
        ConfigKeyData { id, default_value: default, phantom: PhantomData, storage_name: name }
    }
    pub const fn new_1<V: ConfigType>(key: &'static ConfigKeyData<V>) -> ConfigKey<V> {
        ConfigKey(key)
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
        const TYPE_ID: ;
        $crate::config::__macro_priv::new_1(
            &$crate::config::__macro_priv::new_0(
                $storage_name,
                core::any::TypeId::of::<LocalType>(),
                $default,
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
    module_names: Vec<(Arc<str>, Arc<str>, Arc<str>)>,
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
        let module_name = module.name().intern();
        let key_name = name.intern();
        let full_name = format!("{}:{}", module.name(), name).intern();

        if self.found_names.contains(&full_name) {
            error!("Duplicate configuration option '{}'.", full_name);
            bail!("Duplicate configuration option.");
        }
        self.found_names.insert(full_name.clone());

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

        let db_id = StringId::intern(target, &full_name).await?;
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
            config.module_names.push((module_name, key_name, full_name));
        } else {
            self.configs.insert(key.0.id, RegisteredConfig {
                module_names: vec![(module_name, key_name, full_name)],
                db_id,
                dyn_config: Box::new(DynConfigKey::new(target, *key)),
            });
        }

        Ok(())
    }
}

struct ConfigNameKey {
    instance: Arc<RegisteredConfig>,
    name: Arc<str>,
    module_name: Arc<str>,
    full_name: Arc<str>,
}
impl CanDisambiguate for ConfigNameKey {
    const CLASS_NAME: &'static str = "configuration option";
    fn name(&self) -> &str {
        &self.name
    }
    fn full_name(&self) -> &str {
        &self.full_name
    }
    fn module_name(&self) -> &str {
        &self.module_name
    }
}

struct ConfigManagerData {
    list: Arc<[Arc<RegisteredConfig>]>,
    disambiguate: DisambiguatedSet<ConfigNameKey>,
}
impl ConfigManagerData {
    fn from_event(ev: RegisterConfigEvent) -> Self {
        let mut list = Vec::new();
        let mut disambiguated_list = Vec::new();
        for (_, config) in ev.configs {
            let config = Arc::new(config);
            for (module_name, key_name, full_name) in &config.module_names {
                disambiguated_list.push(ConfigNameKey {
                    instance: config.clone(),
                    name: key_name.clone(),
                    module_name: module_name.clone(),
                    full_name: full_name.clone(),
                });
            }
            list.push(config);
        }

        ConfigManagerData {
            list: list.into(),
            disambiguate: DisambiguatedSet::new(disambiguated_list),
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
    pub fn option_list(&self) -> Arc<[Arc<RegisteredConfig>]> {
        self.options.load().as_ref()
            .map(|x| x.list.clone())
            .expect("Config manager is not loaded.")
    }

    /// Looks ups a config option.
    pub async fn lookup_option(&self, option: &str) -> Result<ConfigLookupResult> {
        let options = self.options.load();
        let options = options.as_ref().cmd_error(|| "Config manager is not loaded.")?;

        let mut valid_options = Vec::new();
        for option in options.disambiguate.resolve_iter(option)? {
            valid_options.push(option.value.instance.clone());
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