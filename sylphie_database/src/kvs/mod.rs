use arc_swap::*;
use crate::connection::{Database, DbConnection};
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
    pub trait Sealed: 'static { }
}

/// A marker trait for a type of KVS store.
pub trait KvsType: private::Sealed { }

/// Marks a persistent KVS store.
pub enum PersistentKvsType { }
impl private::Sealed for PersistentKvsType { }
impl KvsType for PersistentKvsType { }

/// Marks a transient KVS store.
pub enum TransientKvsType { }
impl private::Sealed for TransientKvsType { }
impl KvsType for TransientKvsType { }

#[derive(Default)]
struct SchemaCacheBuilder {
    cache: HashMap<String, u32>,
    static_cache_forward: FxHashMap<usize, u32>,
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

        let str_usize = str.as_ptr() as usize;
        let str_id = *self.cache.get(str).unwrap();

        if !self.static_cache_forward.contains_key(&str_usize) {
            self.static_cache_forward.insert(str_usize, str_id);
        }
        if !self.static_cache_backward.contains_key(&str_id) {
        }

        Ok(())
    }
}

struct KvsCommonData {
    static_cache_forward: FxHashMap<usize, u32>,
    static_cache_backward: FxHashMap<u32, String>,
}

struct QuerySet {
    table_name: Arc<str>,
    store_query: Arc<str>,
    delete_query: Arc<str>,
    load_query: Arc<str>,
}
impl QuerySet {
    fn new(table_name: &str) -> Self {
        QuerySet {
            table_name: table_name.into(),
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
    fn create_query(&self) -> String {
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                key BLOB PRIMARY KEY, \
                value BLOB NOT NULL, \
                value_schema_id INTEGER NOT NULL, \
                value_schema_ver INTEGER NOT NULL \
            );",
            self.table_name,
        )
    }
    fn drop_query(&self) -> String {
        format!("DELETE FROM {} WHERE key = ?;", self.table_name)
    }

    async fn create_table(&self, conn: &mut DbConnection) -> Result<()> {
        conn.execute_batch(self.create_query()).await
    }
    async fn drop_table(&self, conn: &mut DbConnection) -> Result<()> {
        conn.execute_batch(self.drop_query()).await
    }
    async fn store_value<K: DbSerializable, V: DbSerializable>(
        &self, conn: &mut DbConnection, key: &K, value: &V,
    ) -> Result<()> {
        conn.execute(
            self.store_query.clone(),
            (
                ByteBuf::from(K::Format::serialize(key)?),
                ByteBuf::from(V::Format::serialize(value)?),
                V::ID, V::SCHEMA_VERSION,
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
        &'a self, conn: &'a mut DbConnection, key: &K, cache: &'a KvsCommonData,
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

struct KvsMetadata {

}

struct InitKvsEvent<'a> {
    found_modules: HashSet<String>,


    conn: &'a mut DbConnection,
    key_cache: &'a mut SchemaCacheBuilder,
}
failable_event!(['a] InitKvsEvent<'a>, (), Error);
impl <'a> InitKvsEvent<'a> {
    async fn init_module<'b>(
        &'b mut self, key_id: &'static str, value_id: &'static str, module: &'b ModuleInfo,
    ) -> Result<()> {
        self.key_cache.add_key_if_not_exists(&mut *self.conn, key_id).await?;
        self.key_cache.add_key_if_not_exists(&mut *self.conn, value_id).await?;

        let mod_name = module.name();
        if self.found_modules.contains(mod_name) {
            bail!("Duplicate KVS module name found: {}", mod_name);
        } else {
            self.found_modules.insert(mod_name.to_string());
        }

        Ok(())
    }
}

struct InitKvsLate(KvsCommonData);
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

    let mut conn = target.get_service::<Database>().connect().await?;
    let mut key_cache = SchemaCacheBuilder::default();

    let schema_id_values: Vec<(String, u32)> = conn.query_vec_nullary(
        "SELECT schema_id_name, schema_id_key FROM sylphie_db_kvs_schema_ids",
    ).await?;
    for (name, id) in schema_id_values {
        key_cache.add_cached_key(name, id);
    }

    target.dispatch_async(InitKvsEvent {
        found_modules: Default::default(),
        conn: &mut conn,
        key_cache: &mut key_cache,
    }).await?;

    Ok(())
}

#[derive(Module)]
#[module(component)]
pub struct BaseKvsStore<K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> {
    #[module_info] info: ModuleInfo,
    data: HashMap<K, V>, // TODO: Temp
    // TODO: Actual proper caching of some sort.
    phantom: PhantomData<fn(& &mut T)>,
}
#[module_impl]
impl <K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> BaseKvsStore<K, V, T> {
    #[event_handler]
    async fn handle_init<'a>(&self, ev: &mut InitKvsEvent<'a>) -> Result<()> {
        ev.init_module(K::ID, V::ID, &self.info).await?;
        Ok(())
    }
}