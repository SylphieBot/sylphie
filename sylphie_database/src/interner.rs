use arc_swap::*;
use crate::connection::*;
use crate::serializable::*;
use crate::migrations::*;
use fxhash::FxHashMap;
use serde::*;
use static_events::prelude_async::*;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use sylphie_core::prelude::*;
use sylphie_utils::cache::LruCache;
use sylphie_utils::locks::LockSet;
use sylphie_utils::scopes::Scope;
use sylphie_utils::strings::{StringWrapper, InternString};

static INTERNER_MIGRATIONS: MigrationData = MigrationData {
    migration_id: "interner b7a62621-ae52-4247-bda6-49d297de20d9",
    migration_set_name: "interner",
    is_transient: false,
    target_version: 1,
    scripts: &[
        migration_script!(0, 1, "sql/interner_0_to_1.sql"),
    ],
};

#[derive(Copy, Clone)]
#[repr(u32)]
enum HiveId {
    SerialIds = 0,
    Scopes = 1,
    Other = 2,
}

struct InternerHive<T: DbSerializable + Eq + Hash> {
    hive_id: u32,
    cache: LruCache<T, u64>,
    rev_cache: LruCache<u64, T>,
    new_value_lock: LockSet<T>,
    max_value: AtomicU64,
}
impl <T: DbSerializable + Eq + Hash> InternerHive<T> {
    fn from_db(hive_id: HiveId, conn: &mut DbSyncConnection) -> Result<InternerHive<T>> {
        let max_value: u64 = conn.query_row(
            "SELECT MAX(int_id) FROM sylphie_db_interner WHERE hive = ?;",
            hive_id as u32,
        )?.flatten().unwrap_or(0);
        Ok(InternerHive {
            hive_id: hive_id as u32,
            cache: LruCache::new(512),
            rev_cache: LruCache::new(512),
            new_value_lock: LockSet::new(),
            max_value: AtomicU64::new(max_value + 1),
        })
    }

    fn intern_query_sync(&self, conn: &mut DbSyncConnection, value: T) -> Result<u64> {
        let result: Option<u64> = conn.query_row(
            "SELECT int_id FROM sylphie_db_interner WHERE hive = ? AND name = ?;",
            (self.hive_id, T::Format::serialize(&value)?),
        )?;
        match result {
            Some(res) => Ok(res),
            None => {
                let new_id = self.max_value.fetch_add(1, Ordering::Relaxed);
                conn.execute(
                    "INSERT INTO sylphie_db_interner (hive, name, int_id) VALUES (?, ?, ?);",
                    (self.hive_id, T::Format::serialize(&value)?, new_id),
                )?;
                Ok(new_id)
            }
        }
    }
    fn query_all(
        &self, conn: &mut DbSyncConnection, mut consume: impl FnMut(T, u64) -> Result<()>,
    ) -> Result<()> {
        let result: Vec<(SerializeValue, u64)> = conn.query_vec(
            "SELECT name, int_id FROM sylphie_db_interner WHERE hive = ?;",
            self.hive_id,
        )?;
        for (value, id) in result {
            consume(T::Format::deserialize(value)?, id)?;
        }
        Ok(())
    }

    async fn intern_query(&self, conn: &mut DbConnection, value: T) -> Result<u64> {
        self.cache.cached_async(value.clone(), async {
            let result: Option<u64> = conn.query_row(
                "SELECT int_id FROM sylphie_db_interner WHERE hive = ? AND name = ?;",
                (self.hive_id, T::Format::serialize(&value)?),
            ).await?;
            Ok(result.unwrap_or(0))
        }).await
    }
    async fn intern(&self, conn: &mut DbConnection, value: T) -> Result<u64> {
        let id = self.intern_query(conn, value.clone()).await?;
        if id == 0 {
            let _guard = self.new_value_lock.lock(value.clone()).await;
            let current_val = self.intern_query(conn, value.clone()).await?;
            if current_val != 0 {
                Ok(current_val)
            } else {
                let new_id = self.max_value.fetch_add(1, Ordering::Relaxed);
                conn.execute(
                    "INSERT INTO sylphie_db_interner (hive, name, int_id) VALUES (?, ?, ?);",
                    (self.hive_id, T::Format::serialize(&value)?, new_id),
                ).await?;
                self.cache.insert(value, new_id);
                Ok(new_id)
            }
        } else {
            Ok(id)
        }
    }
    async fn rev_intern(
        &self, conn: &mut DbConnection, value: u64, intern: impl FnOnce(T) -> T,
    ) -> Result<T> {
        self.rev_cache.cached_async(value.clone(), async {
            let result: SerializeValue = conn.query_row(
                "SELECT name FROM sylphie_db_interner WHERE hive = ? AND int_id = ?;",
                (self.hive_id, value),
            ).await?.internal_err(|| "Invalid interned value.")?;
            Ok(intern(T::Format::deserialize(result)?))
        }).await
    }
}

pub struct InitInterner<'a> {
    hive: &'a InternerHive<StringWrapper>,
    map_forward: &'a mut FxHashMap<usize, u64>,
    map_backward: &'a mut FxHashMap<u64, StringWrapper>,
    conn: &'a mut DbSyncConnection,
}
failable_event!(['a] InitInterner<'a>, (), Error);
impl <'a> InitInterner<'a> {
    pub fn intern_id(&mut self, name: &'static str) -> Result<()> {
        let raw_id = self.hive.intern_query_sync(&mut *self.conn, StringWrapper::Static(name))?;
        self.map_forward.insert(name.as_ptr() as usize, raw_id);
        self.map_backward.insert(raw_id, StringWrapper::Static(name));
        Ok(())
    }
}

struct InternerData {
    hive_scopes: InternerHive<Scope>,
    hive_other: InternerHive<Arc<str>>,
    map_forward: FxHashMap<usize, u64>,
    map_backward: FxHashMap<u64, StringWrapper>,
}

pub struct InternerLock {
    data: Arc<InternerData>,
}
impl InternerLock {
    pub fn get_ser_id(&self, name: &'static str) -> u64 {
        *self.data.map_forward.get(&(name.as_ptr() as usize)).expect("Name does not exist.")
    }
    pub fn get_ser_id_rev(&self, id: u64) -> Arc<str> {
        self.data.map_backward.get(&id).expect("ID does not exist.").as_arc()
    }

    pub async fn get_scope_id(&self, conn: &mut DbConnection, name: Scope) -> Result<ScopeId> {
        Ok(ScopeId(self.data.hive_scopes.intern(conn, name.intern()).await?))
    }
    pub async fn get_scope_id_rev(&self, conn: &mut DbConnection, id: ScopeId) -> Result<Scope> {
        self.data.hive_scopes.rev_intern(conn, id.0, |x| x.intern()).await
    }

    pub async fn get_str_id(&self, conn: &mut DbConnection, str: &str) -> Result<StringId> {
        Ok(StringId(self.data.hive_other.intern(conn, str.intern()).await?))
    }
    pub async fn get_str_id_rev(&self, conn: &mut DbConnection, id: StringId) -> Result<Arc<str>> {
        self.data.hive_other.rev_intern(conn, id.0, |x| x.intern()).await
    }
}

#[derive(Default)]
pub struct Interner {
    data: ArcSwapOption<InternerData>,
}
impl Interner {
    pub fn lock(&self) -> InternerLock {
        InternerLock {
            data: self.data.load().as_ref().expect("SchemaCache is not initialized").clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Default, Debug)]
#[serde(transparent)]
pub struct StringId(u64);
impl StringId {
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub async fn intern(target: &Handler<impl Events>, str: &str) -> Result<StringId> {
        target.get_service::<Interner>().lock().get_str_id(
            &mut target.connect_db().await?, str,
        ).await
    }
    pub async fn extract(&self, target: &Handler<impl Events>) -> Result<Arc<str>> {
        target.get_service::<Interner>().lock().get_str_id_rev(
            &mut target.connect_db().await?, *self,
        ).await
    }
}

#[derive(Serialize, Deserialize)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Default, Debug)]
#[serde(transparent)]
pub struct ScopeId(u64);
impl ScopeId {
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub async fn intern(target: &Handler<impl Events>, scope: Scope) -> Result<ScopeId> {
        target.get_service::<Interner>().lock().get_scope_id(
            &mut target.connect_db().await?, scope,
        ).await
    }
    pub async fn extract(&self, target: &Handler<impl Events>) -> Result<Scope> {
        target.get_service::<Interner>().lock().get_scope_id_rev(
            &mut target.connect_db().await?, *self,
        ).await
    }
}

pub(crate) fn init_interner(target: &Handler<impl Events>) -> Result<()> {
    INTERNER_MIGRATIONS.execute_sync(target)?;

    let mut conn = target.connect_db_sync()?;

    let hive = InternerHive::from_db(HiveId::SerialIds, &mut conn)?;
    let mut map_forward = Default::default();
    let mut map_backward = Default::default();

    let ev = InitInterner {
        hive: &hive,
        map_forward: &mut map_forward,
        map_backward: &mut map_backward,
        conn: &mut conn,
    };
    target.dispatch_sync(ev)?;
    hive.query_all(&mut conn, |name, id| {
        if !map_backward.contains_key(&id) {
            map_backward.insert(id, name.intern());
        }
        Ok(())
    })?;

    let hive_scopes = InternerHive::from_db(HiveId::Scopes, &mut conn)?;
    let hive_other = InternerHive::from_db(HiveId::Other, &mut conn)?;
    target.get_service::<Interner>().data.store(Some(Arc::new(InternerData {
        hive_scopes,
        hive_other,
        map_forward,
        map_backward,
    })));

    Ok(())
}
