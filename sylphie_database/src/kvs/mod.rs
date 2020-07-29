use arc_swap::*;
use crate::connection::*;
use crate::migrations::*;
use crate::serializable::*;
use fxhash::FxHashMap;
use serde_bytes::ByteBuf;
use static_events::prelude_async::*;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;
use sylphie_core::derives::*;
use sylphie_core::prelude::*;
use std::hash::Hash;

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

#[derive(Default)]
struct SchemaCacheBuilder {
    cache: HashMap<String, u32>,
    static_cache_backward: FxHashMap<u32, String>,
    next_key: u32,
}
impl SchemaCacheBuilder {
    fn add_cached_key(&mut self, name: String, id: u32) {
        self.cache.insert(name.clone(), id);
        self.static_cache_backward.insert(id, name);
        if id > self.next_key {
            self.next_key = id;
        }
    }
    async fn add_key_if_not_exists<'a>(
        &'a mut self, conn: &'a mut DbConnection, str: &'static str,
    ) -> Result<()> {
        if !self.cache.contains_key(str) {
            self.next_key += 1;
            conn.execute(
                "INSERT INTO sylphie_db_kvs_schema_ids (schema_id_name, schema_id_key) \
                 VALUES (?, ?);",
                (str, self.next_key)
            ).await?;
            self.cache.insert(str.to_string(), self.next_key);
            self.static_cache_backward.insert(self.next_key, str.to_string());
        }
        Ok(())
    }
}

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
    conn: &'a mut DbConnection,
    key_cache: &'a mut SchemaCacheBuilder,
}
failable_event!(['a] InitKvsEvent<'a>, (), Error);
impl <'a> InitKvsEvent<'a> {
    async fn init_module<'b>(
        &'b mut self,
        key_id: &'static str, key_version: u32, value_id: &'static str,
        module: &'b ModuleInfo, is_transient: bool,
    ) -> Result<()> {
        self.key_cache.add_key_if_not_exists(&mut *self.conn, key_id).await?;
        self.key_cache.add_key_if_not_exists(&mut *self.conn, value_id).await?;

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
                self.key_cache.static_cache_backward.get(&existing_metadata.key_id).unwrap();
            let key_id_matches = key_id == exist_name;
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
                module.name().to_string(), table_name,
                key_id, key_version, is_transient,
            ).await?;
        }

        Ok(())
    }

    fn create_table_name(&self, module_name: &str) -> String {
        let mut unique_id = 0u32;
        loop {
            let hash = blake3::hash(format!("{}|{}", unique_id, module_name).as_bytes()).to_hex();
            let hash = &hash.as_str()[0..16];
            let table_name = format!("sylphie_db_kvs_{}", hash);
            if !self.used_table_names.contains(&table_name) {
                return table_name;
            }
            unique_id += 1;
        }
    }

    async fn create_kvs_table(
        &mut self,
        module_path: String, table_name: String,
        key_id: &'static str, key_version: u32, is_transient: bool,
    ) -> Result<()> {
        debug!("Creating table for KVS store '{}'...", table_name);

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
                module_path.clone(), table_name.clone(), 0,
                *self.key_cache.cache.get(key_id).unwrap(), key_version,
            ),
        ).await?;
        transaction.commit().await?;

        self.used_table_names.insert(table_name.to_string());
        self.module_metadata.insert(
            KvsTarget { module_path, is_transient },
            KvsMetadata {
                table_name,
                key_id: *self.key_cache.cache.get(key_id).unwrap(),
                key_version,
                is_used: true,
            },
        );
        Ok(())
    }

    async fn load_kvs_metadata(&mut self, is_transient: bool) -> Result<()> {
        let values: Vec<(String, String, u32, u32, u32)> = self.conn.query_vec_nullary(
            format!(
                "SELECT module_path, table_name, kvs_schema_version, key_id, key_version \
                 FROM {}sylphie_db_kvs_info",
                if is_transient { "transient." } else { "" },
            ),
        ).await?;
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

    async fn load_schema_key_values(&mut self) -> Result<()> {
        let schema_id_values: Vec<(String, u32)> = self.conn.query_vec_nullary(
            "SELECT schema_id_name, schema_id_key FROM sylphie_db_kvs_schema_ids",
        ).await?;
        for (name, id) in schema_id_values {
            self.key_cache.add_cached_key(name, id);
        }
        Ok(())
    }
}

struct InitKvsLate {
    cache: HashMap<String, u32>,
    static_cache_backward: Arc<FxHashMap<u32, String>>,
    module_metadata: HashMap<KvsTarget, KvsMetadata>,
}
simple_event!(InitKvsLate);

static PERSISTENT_KVS_MIGRATIONS: MigrationData = MigrationData {
    migration_id: "persistent ebc80f22-f8e8-4c0f-b09c-6fd12e3c853b",
    migration_set_name: "persistent_kvs",
    is_transient: false,
    target_version: 1,
    scripts: &[
        migration_script!(0, 1, "persistent_0_to_1.sql"),
    ],
};
static TRANSIENT_KVS_MIGRATIONS: MigrationData = MigrationData {
    migration_id: "transient e9031b35-e448-444d-b161-e75245b30bd8",
    migration_set_name: "transient_kvs",
    is_transient: true,
    target_version: 1,
    scripts: &[
        migration_script!(0, 1, "transient_0_to_1.sql"),
    ],
};
pub(crate) async fn init_kvs(target: &Handler<impl Events>) -> Result<()> {
    let migrations = target.get_service::<MigrationManager>();
    migrations.execute_migration(&PERSISTENT_KVS_MIGRATIONS).await?;
    migrations.execute_migration(&TRANSIENT_KVS_MIGRATIONS).await?;

    // initialize the state for init KVS
    let mut conn = target.connect_db().await?;
    let mut key_cache = SchemaCacheBuilder::default();
    let mut module_metadata = HashMap::new();
    let mut event = InitKvsEvent {
        found_modules: Default::default(),
        used_table_names: Default::default(),
        module_metadata: &mut module_metadata,
        conn: &mut conn,
        key_cache: &mut key_cache,
    };

    // load schema key values
    event.load_schema_key_values().await?;

    // load kvs metadata
    event.load_kvs_metadata(false).await?;
    event.load_kvs_metadata(true).await?;

    // check that everything is OK, and create tables/etc
    target.dispatch_async(event).await?;

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
    target.dispatch_async(InitKvsLate {
        cache: key_cache.cache,
        static_cache_backward: Arc::new(key_cache.static_cache_backward),
        module_metadata,
    }).await;

    Ok(())
}

struct BaseKvsStoreInfo {
    static_cache_backward: Arc<FxHashMap<u32, String>>,
    value_id: u32,
    queries: KvsStoreQueries,
}
impl BaseKvsStoreInfo {
    fn new(
        module: &str, is_transient: bool, late: &InitKvsLate, value_id: &str,
    ) -> Self {
        let metadata = late.module_metadata.get(&KvsTarget {
            module_path: module.to_string(),
            is_transient,
        }).unwrap();
        BaseKvsStoreInfo {
            static_cache_backward: late.static_cache_backward.clone(),
            value_id: *late.cache.get(value_id).unwrap(),
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
        &'a self, conn: &'a mut DbConnection, key: &K, cache: &'a BaseKvsStoreInfo,
        is_migration_mandatory: bool,
    ) -> Result<Option<V>> {
        let result: Option<(ByteBuf, u32, u32)> = conn.query_row(
            self.load_query.clone(),
            ByteBuf::from(K::Format::serialize(key)?),
        ).await?;
        if let Some((bytes, schema_id, schema_ver)) = result {
            let schema_name = cache.static_cache_backward.get(&schema_id)
                .expect("invalid ID in database!")
                .as_str();
            if V::ID == schema_name && V::SCHEMA_VERSION == schema_ver {
                Ok(Some(V::Format::deserialize(&bytes)?))
            } else if V::can_migrate_from(schema_name, schema_ver) {
                Ok(Some(V::do_migration(schema_name, schema_ver, &bytes)?))
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
    // TODO: Actual proper caching of some sort.
    phantom: PhantomData<(fn(& &mut T), K, V)>,
}
#[module_impl]
impl <K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> BaseKvsStore<K, V, T> {
    #[event_handler]
    async fn handle_init<'a>(&self, ev: &mut InitKvsEvent<'a>) -> Result<()> {
        ev.init_module(K::ID, K::SCHEMA_VERSION, V::ID, &self.info, T::IS_TRANSIENT).await?;
        Ok(())
    }

    #[event_handler]
    async fn handle_late_init(&self, ev: &InitKvsLate) {
        self.data.store(Some(Arc::new(BaseKvsStoreInfo::new(
            self.info.name(), T::IS_TRANSIENT, ev, V::ID,
        ))));
    }

    pub async fn get(&self, target: &Handler<impl Events>, k: &K) -> Result<Option<V>> {
        let data = self.data.load();
        let data = data.as_ref().expect("BaseKvsStore not initialized??");
        data.queries.load_value(&mut target.connect_db().await?, k, data, !T::IS_TRANSIENT).await
    }
    pub async fn set(&self, target: &Handler<impl Events>, k: &K, v: &V) -> Result<()> {
        let data = self.data.load();
        let data = data.as_ref().expect("BaseKvsStore not initialized??");
        data.queries.store_value(&mut target.connect_db().await?, k, v, data.value_id).await
    }
    pub async fn remove(&self, target: &Handler<impl Events>, k: &K) -> Result<()> {
        let data = self.data.load();
        let data = data.as_ref().expect("BaseKvsStore not initialized??");
        data.queries.delete_value(&mut target.connect_db().await?, k).await
    }
}