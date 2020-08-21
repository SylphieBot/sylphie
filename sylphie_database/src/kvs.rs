use arc_swap::*;
use crate::connection::*;
use crate::migrations::*;
use crate::interner::*;
use crate::serializable::*;
use serde_bytes::ByteBuf;
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
    key_id: u32,
    key_version: u32,
    is_used: bool,
}

struct InitKvsEvent<'a> {
    found_modules: HashSet<String>,
    used_table_names: HashSet<String>,

    module_metadata: &'a mut HashMap<KvsTarget, KvsMetadata>,
    conn: &'a mut DbSyncConnection,
}
failable_event!(['a] InitKvsEvent<'a>, (), Error);
impl <'a> InitKvsEvent<'a> {
    fn init_module(
        &mut self, target: &Handler<impl Events>,
        key_id: &'static str, key_version: u32, module: &ModuleInfo, is_transient: bool,
    ) -> Result<()> {
        let interner = target.get_service::<StringInterner>().lock();

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

            let exist_name = interner.lookup_id(existing_metadata.key_id);
            let key_id_matches = key_id == &*exist_name;
            let key_version_matches = key_version == existing_metadata.key_version;

            if key_id_matches && key_version_matches {
                // all is OK
            } else {
                // we have a mismatch!
                todo!("Conversions for mismatched kvs versions.")
            }
        } else {
            // we need to create the table.
            let table_name = self.create_table_name(module.name());
            self.create_kvs_table(
                &interner, module.name().to_string(), table_name,
                key_id, key_version, is_transient,
            )?;
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

    fn create_kvs_table(
        &mut self, interner: &StringInternerLock, module_path: String, table_name: String,
        key_id: &'static str, key_version: u32, is_transient: bool,
    ) -> Result<()> {
        debug!("Creating table for KVS store '{}'...", table_name);

        let mut transaction = self.conn.transaction_with_type(TransactionType::Exclusive)?;
        let target_transient = if is_transient { "transient." } else { "" };
        transaction.execute_batch(format!(
            "CREATE TABLE {}{} (\
                key BLOB PRIMARY KEY, \
                value BLOB NOT NULL, \
                value_schema_id INTEGER NOT NULL, \
                value_schema_ver INTEGER NOT NULL \
            )",
            target_transient, table_name,
        ))?;
        transaction.execute(
            format!(
                "INSERT INTO {}sylphie_db_kvs_info \
                     (module_path, table_name, kvs_schema_version, key_id, key_version)\
                 VALUES (?, ?, ?, ?, ?)",
                target_transient,
            ),
            (
                module_path.clone(), table_name.clone(), 0,
                interner.lookup_name(key_id), key_version,
            ),
        )?;
        transaction.commit()?;

        self.used_table_names.insert(table_name.to_string());
        self.module_metadata.insert(
            KvsTarget { module_path, is_transient },
            KvsMetadata {
                table_name,
                key_id: interner.lookup_name(key_id),
                key_version,
                is_used: true,
            },
        );
        Ok(())
    }

    fn load_kvs_metadata(&mut self, is_transient: bool) -> Result<()> {
        let values: Vec<(String, String, u32, u32, u32)> = self.conn.query_vec_nullary(
            format!(
                "SELECT module_path, table_name, kvs_schema_version, key_id, key_version \
                 FROM {}sylphie_db_kvs_info",
                if is_transient { "transient." } else { "" },
            ),
        )?;
        for (module_path, table_name, schema_version, key_id, key_version) in values {
            assert_eq!(
                schema_version, 0,
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
simple_event!(InitKvsLate);

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
pub(crate) fn init_kvs(target: &Handler<impl Events>) -> Result<()> {
    PERSISTENT_KVS_MIGRATIONS.execute_sync(target)?;
    TRANSIENT_KVS_MIGRATIONS.execute_sync(target)?;

    // initialize the state for init KVS
    let mut conn = target.connect_db_sync()?;
    let mut module_metadata = HashMap::new();
    let mut event = InitKvsEvent {
        found_modules: Default::default(),
        used_table_names: Default::default(),
        module_metadata: &mut module_metadata,
        conn: &mut conn,
    };

    // load kvs metadata
    event.load_kvs_metadata(false)?;
    event.load_kvs_metadata(true)?;

    // check that everything is OK, and create tables/etc
    target.dispatch_sync(event)?;

    // drop unused transient tables
    for (key, metadata) in &module_metadata {
        if !metadata.is_used && key.is_transient {
            conn.execute_nullary(format!(
                "DROP TABLE {}{}",
                if key.is_transient { "transient." } else { "" },
                metadata.table_name,
            ))?;
        }
    }

    // Drop our connection.
    std::mem::drop(conn);

    // initialize the actual kvs stores' internal state
    target.dispatch_sync(InitKvsLate { module_metadata });

    Ok(())
}

struct BaseKvsStoreInfo {
    db: Database,
    interner: StringInternerLock,
    value_id: u32,
    queries: KvsStoreQueries,
}
impl BaseKvsStoreInfo {
    fn new(
        target: &Handler<impl Events>,
        module: &str, is_transient: bool, late: &InitKvsLate, value_id: &str,
    ) -> Self {
        let metadata = late.module_metadata.get(&KvsTarget {
            module_path: module.to_string(),
            is_transient,
        }).unwrap();
        let interner = target.get_service::<StringInterner>().lock();
        let value_id = interner.lookup_name(value_id);
        BaseKvsStoreInfo {
            db: target.get_service::<Database>().clone(),
            interner,
            value_id,
            queries: KvsStoreQueries::new(&format!(
                "{}{}",
                if is_transient { "transient." } else { "" },
                metadata.table_name,
            )),
        }
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
        &self, conn: &mut DbConnection, key: &K, value: &V, value_schema_id: u32,
    ) -> Result<()> {
        conn.execute(
            self.store_query.clone(),
            (
                ByteBuf::from(K::Format::serialize(key)?),
                ByteBuf::from(V::Format::serialize(value)?),
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
            ByteBuf::from(K::Format::serialize(key)?),
        ).await?;
        Ok(())
    }
    async fn load_value<'a, K: DbSerializable, V: DbSerializable>(
        &'a self, conn: &'a mut DbConnection, key: &K, store_info: &'a BaseKvsStoreInfo,
        is_migration_mandatory: bool,
    ) -> Result<Option<V>> {
        let result: Option<(ByteBuf, u32, u32)> = conn.query_row(
            self.load_query.clone(),
            ByteBuf::from(K::Format::serialize(key)?),
        ).await?;
        if let Some((bytes, schema_id, schema_ver)) = result {
            let schema_name = store_info.interner.lookup_id(schema_id);
            if V::ID == &*schema_name && V::SCHEMA_VERSION == schema_ver {
                Ok(Some(V::Format::deserialize(&bytes)?))
            } else if V::can_migrate_from(&schema_name, schema_ver) {
                Ok(Some(V::do_migration(&schema_name, schema_ver, &bytes)?))
            } else if !is_migration_mandatory {
                Ok(None)
            } else {
                bail!(
                    "Could not migrate value to current schema version! \
                     (old: {} v{}, new: {} v{})",
                    schema_name, schema_id, V::ID, V::SCHEMA_VERSION,
                );
            }
        } else {
            Ok(None)
        }
    }
}

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
    fn init_interner<'a>(&self, ev: &mut InitInternedStrings<'a>) -> Result<()> {
        ev.intern(K::ID)?;
        ev.intern(V::ID)?;
        Ok(())
    }

    #[event_handler]
    fn init_kvs<'a>(
        &self, target: &Handler<impl Events>, ev: &mut InitKvsEvent<'a>,
    ) -> Result<()> {
        ev.init_module(target, K::ID, K::SCHEMA_VERSION, &self.info, T::IS_TRANSIENT)?;
        Ok(())
    }

    #[event_handler]
    fn init_kvs_late(&self, target: &Handler<impl Events>, ev: &InitKvsLate) {
        self.data.store(Some(Arc::new(BaseKvsStoreInfo::new(
            target, self.info.name(), T::IS_TRANSIENT, ev, V::ID,
        ))));
    }

    fn load_data(&self) -> Arc<BaseKvsStoreInfo> {
        self.data.load().as_ref().expect("BaseKvsStore not yet initialized.").clone()
    }
    async fn connect_db(&self, data: &BaseKvsStoreInfo) -> Result<DbConnection> {
        data.db.connect().await
    }

    async fn get_db(&self, data: &BaseKvsStoreInfo, k: K) -> Result<Option<V>> {
        data.queries.load_value(
            &mut self.connect_db(&data).await?, &k, &data, !T::IS_TRANSIENT,
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
            parent: self,
            _guard: guard,
            key: k,
            value: match value {
                Some(v) => v,
                None => make_default()?,
            },
            data,
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

pub type KvsStore<K, V> = BaseKvsStore<K, V, PersistentKvsType>;
pub type TransientKvsStore<K, V> = BaseKvsStore<K, V, TransientKvsType>;

pub struct KvsMutGuard<'a, K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> {
    parent: &'a BaseKvsStore<K, V, T>,
    _guard: LockSetGuard<'a, K>,
    key: K,
    value: V,
    data: Arc<BaseKvsStoreInfo>,
}
impl <'a, K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> KvsMutGuard<'a, K, V, T> {
    /// Commit the changed KVS value to the database.
    pub async fn commit(self) -> Result<()> {
        self.parent.set_0(&self.data, self.key, self.value).await
    }

    /// Deletes the KVS value from the database.
    pub async fn remove(self) -> Result<()> {
        self.parent.remove_0(&self.data, self.key).await
    }
}
impl <'a, K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType>
    Deref for KvsMutGuard<'a, K, V, T>
{
    type Target = V;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
impl <'a, K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType>
    DerefMut for KvsMutGuard<'a, K, V, T>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}