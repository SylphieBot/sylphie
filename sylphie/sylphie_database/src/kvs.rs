use arc_swap::*;
use crate::connection::*;
use crate::migrations::*;
use crate::interner::*;
use crate::serializable::*;
use static_events::prelude_async::*;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use sylphie_core::derives::*;
use sylphie_core::prelude::*;
use sylphie_utils::cache::LruCache;
use sylphie_utils::locks::{LockSet, LockSetGuard};

mod private {
    pub trait Sealed: 'static {
        const IS_TRANSIENT: bool;
    }
}

/// A marker trait for a type of KVS store.
pub trait KvsType: private::Sealed { }

/// Marks a persistent KVS store.
pub enum PersistentKvsType { }
impl private::Sealed for PersistentKvsType {
    const IS_TRANSIENT: bool = false;
}
impl KvsType for PersistentKvsType { }

/// Marks a transient KVS store.
pub enum TransientKvsType { }
impl private::Sealed for TransientKvsType {
    const IS_TRANSIENT: bool = true;
}
impl KvsType for TransientKvsType { }

#[derive(Eq, PartialEq, Hash)]
struct KvsTarget {
    module_path: String,
    is_transient: bool,
}
struct KvsMetadata {
    table_name: String,
    key_id: StringId,
    key_version: u32,
    is_used: bool,
}

struct InitKvsEvent {
    found_modules: HashSet<String>,
    used_table_names: HashSet<String>,

    module_metadata: HashMap<KvsTarget, KvsMetadata>,
    conn: DbConnection,
}
failable_self_event!(InitKvsEvent, Error);
impl InitKvsEvent {
    async fn init_module<'a>(
        &'a mut self, target: &'a Handler<impl Events>,
        key_id: &'static str, key_version: u32, module: &'a ModuleInfo, is_transient: bool,
    ) -> Result<()> {
        let interner = target.get_service::<Interner>().lock();

        let mod_name = module.name();
        if self.found_modules.contains(mod_name) {
            bail!("Duplicate KVS module name found: {}", mod_name);
        } else {
            self.found_modules.insert(mod_name.to_string());
        }

        if let Some(existing_metadata) = self.module_metadata.get_mut(&KvsTarget {
            module_path: module.name().to_string(),
            is_transient,
        }) {
            existing_metadata.is_used = true;

            let exist_name =
                interner.get_str_id_rev(&mut self.conn, existing_metadata.key_id).await?;
            let key_id_matches = key_id == &*exist_name;
            let key_version_matches = key_version == existing_metadata.key_version;

            if key_id_matches && key_version_matches {
                // all is OK
            } else {
                // we have a mismatch!
                todo!("Conversions for mismatched kvs key versions.")
            }
        } else {
            // we need to create the table.
            let table_name = self.create_table_name(module.name());
            self.create_kvs_table(
                &interner, module.name().to_string(), table_name,
                key_id, key_version, is_transient,
            ).await?;
        }

        Ok(())
    }

    fn strip_to_alphanumeric(value: &str) -> String {
        let mut str = String::new();
        for char in value.chars() {
            match char {
                '0'..='9' | 'a'..='z' => str.push(char),
                'A'..='Z' => str.push((char as u8 - b'A') as char),
                _ => { }
            }
        }
        str
    }
    fn create_table_name(&self, module_name: &str) -> String {
        let parsed_name: Vec<_> = module_name.split('.').collect();
        let name_frag = match parsed_name.as_slice() {
            &[name] => Self::strip_to_alphanumeric(name),
            &[.., parent, name] => format!(
                "{}_{}",
                Self::strip_to_alphanumeric(parent),
                Self::strip_to_alphanumeric(name),
            ),
            _ => unreachable!(),
        };

        let mut unique_id = 0u32;
        loop {
            let hash = blake3::hash(format!("{}|{}", unique_id, module_name).as_bytes()).to_hex();
            let hash = &hash.as_str()[0..4];
            let table_name = format!(
                "sylphie_db_{}_{}",
                hash,
                name_frag
            );
            if !self.used_table_names.contains(&table_name) {
                return table_name;
            }
            unique_id += 1;
        }
    }

    async fn create_kvs_table<'a>(
        &'a mut self, interner: &'a InternerLock, module_path: String, table_name: String,
        key_id: &'static str, key_version: u32, is_transient: bool,
    ) -> Result<()> {
        debug!("Creating table for KVS store '{}'...", table_name);

        let str_id = interner.get_str_id(&mut self.conn, key_id).await?;
        let mut transaction = self.conn.transaction_with_type(TransactionType::Exclusive).await?;
        let target_transient = if is_transient { "transient." } else { "" };
        transaction.execute_batch(format!(
            "CREATE TABLE {}{} (\
                key BLOB PRIMARY KEY, \
                value BLOB NOT NULL, \
                value_schema_id INTEGER NOT NULL, \
                value_schema_ver INTEGER NOT NULL \
            )",
            target_transient, table_name,
        )).await?;
        transaction.execute(
            format!(
                "INSERT INTO {}sylphie_db_kvs_info \
                     (module_path, table_name, kvs_schema_version, key_id, key_version)\
                 VALUES (?, ?, ?, ?, ?)",
                target_transient,
            ),
            (
                module_path.clone(), table_name.clone(), 0u32,
                str_id, key_version,
            ),
        ).await?;
        transaction.commit().await?;

        self.used_table_names.insert(table_name.to_string());
        self.module_metadata.insert(
            KvsTarget { module_path, is_transient },
            KvsMetadata {
                table_name,
                key_id: interner.get_str_id(&mut self.conn, key_id).await?,
                key_version,
                is_used: true,
            },
        );
        Ok(())
    }

    async fn load_kvs_metadata(&mut self, is_transient: bool) -> Result<()> {
        let values: Vec<(String, String, u32, StringId, u32)> = self.conn.query_vec_nullary(
            format!(
                "SELECT module_path, table_name, kvs_schema_version, key_id, key_version \
                 FROM {}sylphie_db_kvs_info",
                if is_transient { "transient." } else { "" },
            ),
        ).await?;
        for (module_path, table_name, schema_version, key_id, key_version) in values {
            assert_eq!(
                schema_version, 0u32,
                "This database was created with a future version of Sylphie.",
            );
            self.used_table_names.insert(table_name.clone());
            self.module_metadata.insert(
                KvsTarget { module_path, is_transient },
                KvsMetadata { table_name, key_id, key_version, is_used: false }
            );
        }
        Ok(())
    }
}

struct InitKvsLate {
    module_metadata: HashMap<KvsTarget, KvsMetadata>,
}
failable_event!(InitKvsLate, (), Error);

static PERSISTENT_KVS_MIGRATIONS: MigrationData = MigrationData {
    migration_id: "kvs persistent ebc80f22-f8e8-4c0f-b09c-6fd12e3c853b",
    migration_set_name: "kvs_persistent",
    is_transient: false,
    target_version: 1,
    scripts: &[
        migration_script!(0, 1, "sql/kvs_persistent_0_to_1.sql"),
    ],
};
static TRANSIENT_KVS_MIGRATIONS: MigrationData = MigrationData {
    migration_id: "kvs transient e9031b35-e448-444d-b161-e75245b30bd8",
    migration_set_name: "kvs_transient",
    is_transient: true,
    target_version: 1,
    scripts: &[
        migration_script!(0, 1, "sql/kvs_transient_0_to_1.sql"),
    ],
};
pub(crate) async fn init_kvs(target: &Handler<impl Events>) -> Result<()> {
    PERSISTENT_KVS_MIGRATIONS.execute(target).await?;
    TRANSIENT_KVS_MIGRATIONS.execute(target).await?;

    // initialize the state for init KVS
    let mut event = InitKvsEvent {
        found_modules: Default::default(),
        used_table_names: Default::default(),
        module_metadata: HashMap::new(),
        conn: target.connect_db().await?,
    };

    // load kvs metadata
    event.load_kvs_metadata(false).await?;
    event.load_kvs_metadata(true).await?;

    // check that everything is OK, and create tables/etc
    let event = target.dispatch_async(event).await?;

    // unpack event
    let module_metadata = event.module_metadata;
    let mut conn = event.conn;

    // drop unused transient tables
    for (key, metadata) in &module_metadata {
        if !metadata.is_used && key.is_transient {
            conn.execute_nullary(format!(
                "DROP TABLE {}{}",
                if key.is_transient { "transient." } else { "" },
                metadata.table_name,
            )).await?;
        }
    }

    // Drop our connection.
    std::mem::drop(conn);

    // initialize the actual kvs stores' internal state
    target.dispatch_async(InitKvsLate { module_metadata }).await?;

    Ok(())
}

struct BaseKvsStoreInfo {
    db: Database,
    interner: InternerLock,
    value_id: StringId,
    queries: KvsStoreQueries,
}
impl BaseKvsStoreInfo {
    async fn new<'a>(
        target: &'a Handler<impl Events>,
        module: &'a str, is_transient: bool, late: &'a InitKvsLate, value_id: &'static str,
    ) -> Result<Self> {
        let metadata = late.module_metadata.get(&KvsTarget {
            module_path: module.to_string(),
            is_transient,
        }).unwrap();
        let interner = target.get_service::<Interner>().lock();
        let value_id = StringId::intern(target, value_id).await?;
        Ok(BaseKvsStoreInfo {
            db: target.get_service::<Database>().clone(),
            interner,
            value_id,
            queries: KvsStoreQueries::new(&format!(
                "{}{}",
                if is_transient { "transient." } else { "" },
                metadata.table_name,
            )),
        })
    }
}

struct KvsStoreQueries {
    store_query: Arc<str>,
    delete_query: Arc<str>,
    load_query: Arc<str>,
}
impl KvsStoreQueries {
    fn new(table_name: &str) -> Self {
        KvsStoreQueries {
            store_query: format!(
                "REPLACE INTO {} (key, value, value_schema_id, value_schema_ver) \
                 VALUES (?, ?, ?, ?)",
                table_name,
            ).into(),
            delete_query: format!("DELETE FROM {} WHERE key = ?;", table_name).into(),
            load_query: format!(
                "SELECT value, value_schema_id, value_schema_ver FROM {} WHERE key = ?;",
                table_name,
            ).into(),
        }
    }

    async fn store_value<K: DbSerializable, V: DbSerializable>(
        &self, conn: &mut DbConnection, key: &K, value: &V, value_schema_id: StringId,
    ) -> Result<()> {
        conn.execute(
            self.store_query.clone(),
            (
                K::Format::serialize(key)?,
                V::Format::serialize(value)?,
                value_schema_id, V::SCHEMA_VERSION,
            ),
        ).await?;
        Ok(())
    }
    async fn delete_value<K: DbSerializable>(
        &self, conn: &mut DbConnection, key: &K,
    ) -> Result<()> {
        conn.execute(
            self.delete_query.clone(),
            K::Format::serialize(key)?,
        ).await?;
        Ok(())
    }
    async fn load_value<'a, K: DbSerializable, V: DbSerializable>(
        &'a self, conn: &'a mut DbConnection, key: &K, store_info: &'a BaseKvsStoreInfo,
        value_schema_id: StringId, is_migration_mandatory: bool,
    ) -> Result<Option<V>> {
        let result: Option<(SerializeValue, StringId, u32)> = conn.query_row(
            self.load_query.clone(),
            K::Format::serialize(key)?,
        ).await?;
        if let Some((value, schema_id, schema_ver)) = result {
            if schema_id == value_schema_id && V::SCHEMA_VERSION == schema_ver {
                Ok(Some(V::Format::deserialize(value)?))
            } else {
                let schema_name = store_info.interner.get_str_id_rev(conn, schema_id).await?;
                if V::can_migrate_from(&schema_name, schema_ver) {
                    Ok(Some(V::do_migration(&schema_name, schema_ver, value)?))
                } else if !is_migration_mandatory {
                    Ok(None)
                } else {
                    bail!(
                        "Could not migrate value to current schema version! ({}:{} -> {}:{})",
                        schema_name, schema_ver, V::ID, V::SCHEMA_VERSION,
                    );
                }
            }
        } else {
            Ok(None)
        }
    }
}

/// The base type for KVS stores backed by the database.
///
/// This is a module, and should be used by attaching it to the your module as a submodule.
///
/// You should generally prefer [`KvsStore`] or [`TransientKvsStore`] as convenience wrappers
/// over this type.
#[derive(Module)]
#[module(component)]
pub struct BaseKvsStore<K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> {
    #[module_info] info: ModuleInfo,
    data: ArcSwapOption<BaseKvsStoreInfo>,
    // TODO: Figure out a better way to do the LruCache capacity.
    #[init_with { LruCache::new(1024) }] cache: LruCache<K, Option<V>>,
    lock_set: LockSet<K>,
    phantom: PhantomData<fn(& &mut T)>,
}
#[module_impl]
impl <K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> BaseKvsStore<K, V, T> {
    #[event_handler]
    async fn init_kvs(
        &self, target: &Handler<impl Events>, ev: &mut InitKvsEvent,
    ) -> Result<()> {
        ev.init_module(target, K::ID, K::SCHEMA_VERSION, &self.info, T::IS_TRANSIENT).await?;
        Ok(())
    }

    #[event_handler]
    async fn init_kvs_late(&self, target: &Handler<impl Events>, ev: &InitKvsLate) -> Result<()> {
        self.data.store(Some(Arc::new(BaseKvsStoreInfo::new(
            target, self.info.name(), T::IS_TRANSIENT, ev, V::ID,
        ).await?)));
        Ok(())
    }

    fn load_data(&self) -> Arc<BaseKvsStoreInfo> {
        self.data.load().as_ref().expect("BaseKvsStore not yet initialized.").clone()
    }
    async fn connect_db(&self, data: &BaseKvsStoreInfo) -> Result<DbConnection> {
        data.db.connect().await
    }

    async fn get_db(&self, data: &BaseKvsStoreInfo, k: K) -> Result<Option<V>> {
        data.queries.load_value(
            &mut self.connect_db(&data).await?, &k, &data, data.value_id, !T::IS_TRANSIENT,
        ).await
    }
    async fn get_0(&self, data: &BaseKvsStoreInfo, k: K) -> Result<Option<V>> {
        self.cache.cached_async(k.clone(), self.get_db(data, k)).await
    }
    async fn set_0(&self, data: &BaseKvsStoreInfo, k: K, v: V) -> Result<()> {
        data.queries.store_value(&mut self.connect_db(&data).await?, &k, &v, data.value_id).await?;
        self.cache.insert(k, Some(v));
        Ok(())
    }
    async fn remove_0(&self, data: &BaseKvsStoreInfo, k: K) -> Result<()> {
        data.queries.delete_value(&mut self.connect_db(&data).await?, &k).await?;
        self.cache.insert(k, None);
        Ok(())
    }
    async fn get_mut_0<'a>(
        &'a self, guard: LockSetGuard<'a, K>, k: K, make_default: impl FnOnce() -> Result<V>,
    ) -> Result<KvsMutGuard<'a, K, V, T>> {
        let data = self.load_data();
        let value = self.get_0(&data, k.clone()).await?;
        Ok(KvsMutGuard {
            kvs_parent: self,
            _guard: guard,
            ul_key: k,
            ul_value: match value {
                Some(v) => v,
                None => make_default()?,
            },
            ul_data: data,
        })
    }

    /// Retrieves a value from a KVS store in the database.
    pub async fn get(&self, k: K) -> Result<Option<V>> {
        self.get_0(&self.load_data(), k).await
    }

    /// Stores a value from the KVS store in the database.
    ///
    /// If another task is already writing to this database, this function will temporarily block.
    pub async fn set(&self, k: K, v: V) -> Result<()> {
        let _guard = self.lock_set.lock(k.clone()).await;
        self.set_0(&self.load_data(), k, v).await
    }

    /// Removes a value from the KVS store in the database.
    ///
    /// If another task is already writing to this database, this function will temporarily block.
    pub async fn remove(&self, k: K) -> Result<()> {
        let _guard = self.lock_set.lock(k.clone()).await;
        self.remove_0(&self.load_data(), k).await
    }

    /// Returns a mutable handle to a value on the KVS store. If the value does not already exist,
    /// it is initialized with [`Default::default`].
    ///
    /// If the value does not already exist, it is initialized with the given closure.
    ///
    /// If another task is already writing to this database, this function will temporarily block.
    ///
    /// You must call [`KvsMutGuard::commit`] to actually write the new changed value to the
    /// database. All changes are lost if you simply drop the value.
    pub async fn get_mut(
        &self, k: K, default: impl FnOnce() -> Result<V>,
    ) -> Result<KvsMutGuard<'_, K, V, T>> {
        let guard = self.lock_set.lock(k.clone()).await;
        self.get_mut_0(guard, k, default).await
    }

    /// Tries to return a mutable handle to a value on the KVS store. If another task is writing
    /// to this key, this function will return `None`.
    ///
    /// If the value does not already exist, it is initialized with the given closure.
    ///
    /// You must call [`KvsMutGuard::commit`] to actually write the new changed value to the
    /// database. All changes are lost if you simply drop the value.
    pub async fn try_get_mut(
        &self, k: K, default: impl FnOnce() -> Result<V>,
    ) -> Result<Option<KvsMutGuard<'_, K, V, T>>> {
        match self.lock_set.try_lock(k.clone()) {
            Some(guard) => Ok(Some(self.get_mut_0(guard, k, default).await?)),
            None => Ok(None),
        }
    }

    /// Returns a mutable handle to a value on the KVS store.
    ///
    /// If the value does not already exist, it is initialized with [`Default::default`].
    ///
    /// If another task is already writing to this database, this function will temporarily block.
    ///
    /// You must call [`KvsMutGuard::commit`] to actually write the new changed value to the
    /// database. All changes are lost if you simply drop the value.
    pub async fn get_mut_default(&self, k: K) -> Result<KvsMutGuard<'_, K, V, T>>
        where V: Default,
    {
        self.get_mut(k, || Ok(Default::default())).await
    }

    /// Tries to return a mutable handle to a value on the KVS store. If another task is writing
    /// to this key, this function will return `None`.
    ///
    /// If the value does not already exist, it is initialized with [`Default::default`].
    ///
    /// You must call [`KvsMutGuard::commit`] to actually write the new changed value to the
    /// database. All changes are lost if you simply drop the value.
    pub async fn try_get_mut_default(&self, k: K) -> Result<Option<KvsMutGuard<'_, K, V, T>>>
        where V: Default,
    {
        self.try_get_mut(k, || Ok(Default::default())).await
    }
}

/// The base type for KVS stores backed by the database.
///
/// This is a module, and should be used by attaching it to the your module as a submodule.
pub type KvsStore<K, V> = BaseKvsStore<K, V, PersistentKvsType>;

/// The base type for KVS stores backed by the transient database.
///
/// This is a module, and should be used by attaching it to the your module as a submodule.
pub type TransientKvsStore<K, V> = BaseKvsStore<K, V, TransientKvsType>;

/// A guard for mutating values in the KVS as a mutable object.
pub struct KvsMutGuard<'a, K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> {
    kvs_parent: &'a BaseKvsStore<K, V, T>,
    _guard: LockSetGuard<'a, K>,
    ul_key: K,
    ul_value: V,
    ul_data: Arc<BaseKvsStoreInfo>,
}
impl <'a, K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> KvsMutGuard<'a, K, V, T> {
    /// Commit the changed KVS value to the database.
    pub async fn commit(self) -> Result<()> {
        self.kvs_parent.set_0(&self.ul_data, self.ul_key, self.ul_value).await
    }

    /// Deletes the KVS value from the database.
    pub async fn remove(self) -> Result<()> {
        self.kvs_parent.remove_0(&self.ul_data, self.ul_key).await
    }
}
impl <'a, K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType>
    Deref for KvsMutGuard<'a, K, V, T>
{
    type Target = V;
    fn deref(&self) -> &Self::Target {
        &self.ul_value
    }
}
impl <'a, K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType>
    DerefMut for KvsMutGuard<'a, K, V, T>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ul_value
    }
}