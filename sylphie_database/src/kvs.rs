use bincode;
use crate::serializable::*;
use serde::*;
use serde::de::DeserializeOwned;
use serde_cbor;
use static_events::prelude_async::*;
use std::collections::HashMap;
use std::marker::PhantomData;
use sylphie_core::derives::*;
use sylphie_core::prelude::*;
use std::hash::Hash;

mod private {
    use super::*;
    pub trait Sealed: 'static {
        fn serialize_value(v: &impl Serialize) -> Result<Vec<u8>>;
        fn deserialize_value<D: DeserializeOwned>(v: &[u8]) -> Result<D>;
    }
}

/// A marker trait for a type of KVS store.
pub trait KvsType: private::Sealed { }

/// Marks a persistent KVS store.
pub enum PersistentKvsType { }
impl private::Sealed for PersistentKvsType {
    fn serialize_value(v: &impl Serialize) -> Result<Vec<u8>> {
        Ok(serde_cbor::to_vec(v)?)
    }
    fn deserialize_value<D: DeserializeOwned>(v: &[u8]) -> Result<D> {
        Ok(serde_cbor::from_slice(v)?)
    }
}
impl KvsType for PersistentKvsType { }

/// Marks a transient KVS store.
pub enum TransientKvsType { }
impl private::Sealed for TransientKvsType {
    fn serialize_value(v: &impl Serialize) -> Result<Vec<u8>> {
        Ok(bincode::serialize(v)?)
    }
    fn deserialize_value<D: DeserializeOwned>(v: &[u8]) -> Result<D> {
        Ok(bincode::deserialize(v)?)
    }
}
impl KvsType for TransientKvsType { }

struct InitKvsEvent {

}
simple_event!(InitKvsEvent);
pub(crate) fn init_kvs(target: &Handler<impl Events>) {
    target.dispatch_sync(InitKvsEvent {

    });
}

#[derive(Module)]
#[module(component)]
pub struct BaseKvsStore<K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> {
    #[module_info] info: ModuleInfo,
    data: HashMap<K, V>, // TODO: Temp
    phantom: PhantomData<fn(& &mut T)>,
}
impl <K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> BaseKvsStore<K, V, T> {

}