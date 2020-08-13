use arc_swap::*;
use crate::connection::*;
use crate::migrations::*;
use fxhash::{FxHashMap, FxHashSet};
use static_events::prelude_async::*;
use std::sync::Arc;
use sylphie_core::prelude::*;

static INTERNER_MIGRATIONS: MigrationData = MigrationData {
    migration_id: "interner b7a62621-ae52-4247-bda6-49d297de20d9",
    migration_set_name: "interner",
    is_transient: false,
    target_version: 1,
    scripts: &[
        migration_script!(0, 1, "sql/interner_0_to_1.sql"),
    ],
};

#[derive(Default)]
struct StringInternerData {
    intern_data: FxHashSet<Arc<str>>,
    cache: FxHashMap<Arc<str>, u32>,
    rev_cache: FxHashMap<u32, Arc<str>>,
    next_key: u32,
}
impl StringInternerData {
    fn intern(&mut self, name: &str) -> Arc<str> {
        let arc: Arc<str> = name.to_string().into();
        if let Some(x) = self.intern_data.get(&arc) {
            x.clone()
        } else {
            self.intern_data.insert(arc.clone());
            arc
        }
    }
    fn intern_to_db(
        &mut self, conn: &mut DbSyncConnection, name: &'static str,
    ) -> Result<()> {
        let name = self.intern(name);
        if !self.cache.contains_key(&name) {
            self.next_key += 1;
            conn.execute(
                "INSERT INTO sylphie_db_interner (name, int_id) \
                 VALUES (?, ?);",
                (name.clone(), self.next_key),
            )?;
            self.cache.insert(name.clone(), self.next_key);
            self.rev_cache.insert(self.next_key, name);
        }
        Ok(())
    }

    fn register_interned(&mut self, name: &str, id: u32) {
        let name = self.intern(name);
        self.cache.insert(name.clone(), id);
        self.rev_cache.insert(id, name);
        if id > self.next_key {
            self.next_key = id;
        }
    }
    fn load_schema_key_values(&mut self, conn: &mut DbSyncConnection) -> Result<()> {
        let schema_id_values: Vec<(String, u32)> = conn.query_vec_nullary(
            "SELECT name, int_id FROM sylphie_db_interner",
        )?;
        for (name, id) in schema_id_values {
            self.register_interned(&name, id);
        }
        Ok(())
    }
}

pub struct InitInternedStrings<'a> {
    data: &'a mut StringInternerData,
    conn: DbSyncConnection,
}
failable_event!(['a] InitInternedStrings<'a>, (), Error);
impl <'a> InitInternedStrings<'a> {
    pub fn intern(&mut self, name: &'static str) -> Result<()> {
        self.data.intern_to_db(&mut self.conn, name)
    }
}

pub struct StringInternerLock {
    data: Arc<StringInternerData>,
}
impl StringInternerLock {
    pub fn lookup_id(&self, id: u32) -> Arc<str> {
        self.data.rev_cache.get(&id).expect("ID does not exist.").clone()
    }
    pub fn lookup_name(&self, name: &str) -> u32 {
        *self.data.cache.get(name).expect("Name does not exist.")
    }
}

#[derive(Default)]
pub struct StringInterner {
    data: ArcSwapOption<StringInternerData>,
}
impl StringInterner {
    pub fn lock(&self) -> StringInternerLock {
        StringInternerLock {
            data: self.data.load().as_ref().expect("SchemaCache is not initialized").clone(),
        }
    }
}

pub fn init_interner(target: &Handler<impl Events>) -> Result<()> {
    INTERNER_MIGRATIONS.execute_sync(target)?;

    let mut conn = target.connect_db_sync()?;
    let mut data = StringInternerData::default();
    data.load_schema_key_values(&mut conn)?;

    let ev = InitInternedStrings {
        data: &mut data,
        conn,
    };
    target.dispatch_sync(ev)?;
    target.get_service::<StringInterner>().data.store(Some(Arc::new(data)));

    Ok(())
}
