use serde::*;
use serde::de::DeserializeOwned;
use sylphie_core::prelude::*;

/// A format that can be used to serialize database values.
pub trait SerializationFormat {
    fn serialize(val: &impl DbSerializable) -> Result<Vec<u8>>;
    fn deserialize<T: DbSerializable>(val: &[u8]) -> Result<T>;
}

/// A [`SerializationFormat`] that serializes in a combat non-self-describing binary form.
pub enum BincodeFormat { }
impl SerializationFormat for BincodeFormat {
    fn serialize(val: &impl DbSerializable) -> Result<Vec<u8>> {
        Ok(bincode::serialize(val)?)
    }
    fn deserialize<T: DbSerializable>(val: &[u8]) -> Result<T> {
        Ok(bincode::deserialize(val)?)
    }
}

/// A [`SerializationFormat`] that serializes a value as CBOR.
pub enum CborFormat { }
impl SerializationFormat for CborFormat {
    fn serialize(val: &impl DbSerializable) -> Result<Vec<u8>> {
        Ok(serde_cbor::to_vec(val)?)
    }
    fn deserialize<T: DbSerializable>(val: &[u8]) -> Result<T> {
        Ok(serde_cbor::from_slice(val)?)
    }
}

/// A trait for types that can be serialized into database columns.
pub trait DbSerializable: Sized + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The serialization format that will be used for this trait.
    type Format: SerializationFormat;

    /// An ID used to determine if a type in a serialized data structure has been replaced
    /// entirely.
    const ID: &'static str = "default";

    /// The schema version of this particular type.
    ///
    /// This is used to allow for manual migrations.
    const SCHEMA_VERSION: usize;

    /// Returns whether a given id/version combination can be migrated to the current one.
    fn can_migrate_from(_from_id: &'static str, _from_version: usize) -> bool {
        false
    }

    /// Loads a value from a outdated KVS store
    fn do_migration(
        _from_id: &'static str, _from_version: usize, _data: &[u8],
    ) -> Result<Self> {
        bail!("Migration not supported.")
    }
}

/// A simple wrapper that implements [`DbSerializable`] over any compatible type.
///
/// This does not support migrations and serializes using a non self-describing format.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug, Hash, Default)]
#[derive(Serialize)]
#[serde(transparent)]
pub struct SimpleSerialize<T: Serialize + DeserializeOwned + Send + Sync + 'static>(T);
impl <T: Serialize + DeserializeOwned + Send + Sync + 'static> From<T> for SimpleSerialize<T> {
    fn from(t: T) -> Self {
        SimpleSerialize(t)
    }
}
impl <T: Serialize + DeserializeOwned + Send + Sync + 'static>
    DbSerializable for SimpleSerialize<T>
{
    type Format = BincodeFormat;
    const SCHEMA_VERSION: usize = 0;
}
impl <'de, T: Serialize + DeserializeOwned + Send + Sync + 'static, >
    Deserialize<'de> for SimpleSerialize<T>
{
    fn deserialize<D>(deser: D) -> StdResult<Self, D::Error> where D: Deserializer<'de> {
        T::deserialize(deser).map(SimpleSerialize)
    }
}