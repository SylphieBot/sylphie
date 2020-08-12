use arc_swap::*;
use crate::connection::*;
use crate::migrations::*;
use fxhash::{FxHashMap, FxHashSet};
use static_events::prelude_async::*;
use std::sync::Arc;
use sylphie_core::prelude::*;

static SCHEMA_ID_MIGRATIONS: MigrationData = MigrationData {
    migration_id: "schema b7a62621-ae52-4247-bda6-49d297de20d9",
    migration_set_name: "schema_ids",
    is_transient: false,
    target_version: 1,
    scripts: &[
        migration_script!(0, 1, "sql/schema_id_0_to_1.sql"),
    ],
};

#[derive(Default)]
struct SchemaCacheData {
    intern_data: FxHashSet<Arc<str>>,
    cache: FxHashMap<Arc<str>, u32>,
    rev_cache: FxHashMap<u32, Arc<str>>,
    next_key: u32,
}
impl SchemaCacheData {
    fn intern(&mut self, name: &str) -> Arc<str> {
        let arc: Arc<str> = name.to_string().into();
        if let Some(x) = self.intern_data.get(&arc) {
            x.clone()
        } else {
            self.intern_data.insert(arc.clone());
            arc
        }
    }
    fn add_cached_key(&mut self, name: &str, id: u32) {
        let name = self.intern(name);
        self.cache.insert(name.clone(), id);
        self.rev_cache.insert(id, name);
        if id > self.next_key {
            self.next_key = id;
        }
    }

    async fn cache_key<'a>(
        &'a mut self, conn: &'a mut DbConnection, name: &'static str,
    ) -> Result<()> {
        let name = self.intern(name);
        if !self.cache.contains_key(&name) {
            self.next_key += 1;
            conn.execute(
                "INSERT INTO sylphie_db_schema_ids (schema_id_name, schema_id_key) \
                 VALUES (?, ?);",
                (name.clone(), self.next_key),
            ).await?;
            self.cache.insert(name.clone(), self.next_key);
            self.rev_cache.insert(self.next_key, name);
        }
        Ok(())
    }
    async fn load_schema_key_values<'a>(&'a mut self, conn: &'a mut DbConnection) -> Result<()> {
        let schema_id_values: Vec<(String, u32)> = conn.query_vec_nullary(
            "SELECT schema_id_name, schema_id_key FROM sylphie_db_schema_ids",
        ).await?;
        for (name, id) in schema_id_values {
            self.add_cached_key(&name, id);
        }
        Ok(())
    }
}

pub struct InitSchemaData<'a> {
    data: &'a mut SchemaCacheData,
    conn: DbConnection,
}
failable_event!(['a] InitSchemaData<'a>, (), Error);
impl <'a> InitSchemaData<'a> {
    pub async fn cache_key(&mut self, name: &'static str) -> Result<()> {
        self.data.cache_key(&mut self.conn, name).await
    }
}

pub struct SchemaCacheLock {
    data: Arc<SchemaCacheData>,
}
impl SchemaCacheLock {
    pub fn lookup_id(&self, id: u32) -> Arc<str> {
        self.data.rev_cache.get(&id).expect("ID does not exist.").clone()
    }
    pub fn lookup_name(&self, name: &str) -> u32 {
        *self.data.cache.get(name).expect("Name does not exist.")
    }
}

#[derive(Default)]
pub struct SchemaCache {
    data: ArcSwapOption<SchemaCacheData>,
}
impl SchemaCache {
    pub fn lock(&self) -> SchemaCacheLock {
        SchemaCacheLock {
            data: self.data.load().as_ref().expect("SchemaCache is not initialized").clone(),
        }
    }
}

pub async fn init_schema_cache(target: &Handler<impl Events>) -> Result<()> {
    SCHEMA_ID_MIGRATIONS.execute(target).await?;

    let mut conn = target.connect_db().await?;
    let mut data = SchemaCacheData::default();
    data.load_schema_key_values(&mut conn).await?;

    let ev = InitSchemaData {
        data: &mut data,
        conn: target.connect_db().await?,
    };
    target.dispatch_async(ev).await?;
    target.get_service::<SchemaCache>().data.store(Some(Arc::new(data)));

    Ok(())
}
