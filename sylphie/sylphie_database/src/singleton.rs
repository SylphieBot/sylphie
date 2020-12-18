use arc_swap::*;
use crate::kvs::*;
use crate::interner::*;
use crate::serializable::*;
use serde::*;
use static_events::prelude_async::*;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use sylphie_core::derives::*;
use sylphie_core::prelude::*;
use tokio::sync::{RwLock, RwLockWriteGuard};

#[derive(Serialize, Deserialize, Clone)]
struct SingletonData {
    value: SerializeValue,
    ser_id: StringId,
    ser_ver: u32,
    exists: bool,
}
impl DbSerializable for SingletonData {
    type Format = CborFormat;
    const ID: &'static str = "sylphie_database::singleton::SingletonData";
    const SCHEMA_VERSION: u32 = 0;
}

#[derive(Module)]
pub(crate) struct SingletonDataStore {
    #[module_info] info: ModuleInfo,
    // TODO: Figure out a way to not get screwed memorywise by the caches implied here.
    #[submodule] singletons: Arc<KvsStore<Arc<str>, SingletonData>>,
}

struct InitData {
    kvs: Arc<KvsStore<Arc<str>, SingletonData>>,
    ser_id: StringId,
}

/// The base type for KVS stores backed by the database.
///
/// This is a module, and should be used by attaching it to the your module as a submodule.
///
/// You should generally prefer [`KvsStore`] or [`TransientKvsStore`] as convenience wrappers
/// over this type.
#[derive(Module)]
#[module(component)]
pub struct SingletonStore<V: Default + DbSerializable> {
    #[module_info] info: ModuleInfo,
    cached_instance: RwLock<Option<V>>,
    data_store: ArcSwapOption<InitData>,
}
#[module_impl]
impl <V: Default + DbSerializable> SingletonStore<V> {
    #[event_handler]
    async fn init_cache(
        &self, target: &Handler<impl Events>, _: &crate::InitDbEvent,
    ) -> Result<()> {
        let underlying = target.get_service::<SingletonDataStore>().singletons.clone();
        self.data_store.store(Some(Arc::new(InitData {
            kvs: underlying,
            ser_id: StringId::intern(target, V::ID).await?,
        })));
        self.init_underlying().await?;
        self.read_db(target).await?;
        Ok(())
    }

    async fn init_underlying(&self) -> Result<()> {
        let data = self.data_store.load();
        let data = data.as_ref().unwrap();
        let mut underlying = data.kvs.get_mut(
            self.info.arc_name(), || Ok(SingletonData {
                value: SerializeValue::Null,
                ser_id: StringId::default(),
                ser_ver: 0,
                exists: false,
            }),
        ).await?;
        if !underlying.exists {
            underlying.value = V::Format::serialize(&V::default())?;
            underlying.ser_id = data.ser_id;
            underlying.ser_ver = V::SCHEMA_VERSION;
            underlying.exists = true;
            underlying.commit().await?;
        }
        Ok(())
    }
    async fn read_db(&self, target: &Handler<impl Events>) -> Result<()> {
        let mut lock = self.cached_instance.write().await;
        let data = self.data_store.load();
        let data = data.as_ref().unwrap();
        let underlying = data.kvs.get(self.info.arc_name()).await?.unwrap();
        assert!(underlying.exists);
        if underlying.ser_id == data.ser_id && underlying.ser_ver == V::SCHEMA_VERSION {
            *lock = Some(V::Format::deserialize(underlying.value)?);
        } else {
            let migration_id = underlying.ser_id.extract(target).await?;
            if V::can_migrate_from(&migration_id, underlying.ser_ver) {
                let val = V::do_migration(&migration_id, underlying.ser_ver, underlying.value)?;
                *lock = Some(val);
            } else {
                panic!("Cannot migrate from {}:{} -> {}:{}",
                       migration_id, underlying.ser_ver, V::ID, V::SCHEMA_VERSION);
            }
        }
        Ok(())
    }
    async fn set_raw(&self, value: V) -> Result<()> {
        let mut lock = self.cached_instance.write().await;
        let data = self.data_store.load();
        let data = data.as_ref().unwrap();
        let serialized = SingletonData {
            value: V::Format::serialize(&value)?,
            ser_id: data.ser_id,
            ser_ver: V::SCHEMA_VERSION,
            exists: true,
        };
        data.kvs.set(self.info.arc_name(), serialized).await?;
        *lock = Some(value);
        Ok(())
    }

    /// Returns the contained value.
    pub async fn get(&self) -> V {
        self.cached_instance.read().await.as_ref().unwrap().clone()
    }

    /// Sets the contained value.
    pub async fn set(&self, v: V) -> Result<()> {
        self.set_raw(v).await
    }

    /// Returns a mutable handle to the contained value.
    ///
    /// If another task is already writing to this database, this function will temporarily block.
    ///
    /// You must call [`SingletonMutGuard::commit`] to actually write the new changed value to the
    /// database. All changes are lost if you simply drop the value.
    pub async fn get_mut(&self) -> Result<SingletonMutGuard<'_, V>> {
        let guard = self.cached_instance.write().await;
        Ok(SingletonMutGuard {
            parent: self,
            value: guard.as_ref().unwrap().clone(),
            locked: guard,
        })
    }
}

/// A guard for mutating values in a singleton store.
pub struct SingletonMutGuard<'a, V: Default + DbSerializable> {
    parent: &'a SingletonStore<V>,
    value: V,
    locked: RwLockWriteGuard<'a, Option<V>>,
}
impl <'a, V: Default + DbSerializable> SingletonMutGuard<'a, V> {
    /// Commit the changed value to the database.
    pub async fn commit(self) -> Result<()> {
        std::mem::drop(self.locked);
        self.parent.set_raw(self.value).await
    }
}
impl <'a, V: Default + DbSerializable> Deref for SingletonMutGuard<'a, V> {
    type Target = V;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
impl <'a, V: Default + DbSerializable> DerefMut for SingletonMutGuard<'a, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}