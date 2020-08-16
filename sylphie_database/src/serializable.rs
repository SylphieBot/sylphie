use bincode::Options;
use serde::*;
use serde::de::DeserializeOwned;
use sylphie_core::prelude::*;
use sylphie_utils::scopes::*;
use sylphie_utils::strings::StringWrapper;

/// A format that can be used to serialize database values.
pub trait SerializationFormat {
    fn serialize(val: &impl DbSerializable) -> Result<Vec<u8>>;
    fn deserialize<T: DbSerializable>(val: &[u8]) -> Result<T>;
}

/// A [`SerializationFormat`] that serializes in a combat non-self-describing binary form.
pub enum BincodeFormat { }
impl SerializationFormat for BincodeFormat {
    fn serialize(val: &impl DbSerializable) -> Result<Vec<u8>> {
        Ok(bincode::DefaultOptions::new().with_varint_encoding().serialize(val)?)
    }
    fn deserialize<T: DbSerializable>(val: &[u8]) -> Result<T> {
        Ok(bincode::DefaultOptions::new().with_varint_encoding().deserialize(val)?)
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
pub trait DbSerializable: Clone + Sized + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The serialization format that will be used for this trait.
    type Format: SerializationFormat;

    /// An ID used to determine if a type in a serialized data structure has been replaced
    /// entirely.
    const ID: &'static str = "default";

    /// The schema version of this particular type.
    ///
    /// This is used to allow for manual migrations.
    const SCHEMA_VERSION: u32;

    /// Returns whether a given id/version combination can be migrated to the current one.
    fn can_migrate_from(_from_id: &str, _from_version: u32) -> bool {
        false
    }

    /// Loads a value from a outdated KVS store
    fn do_migration(
        _from_id: &str, _from_version: u32, _data: &[u8],
    ) -> Result<Self> {
        bail!("Migration not supported.")
    }
}

impl DbSerializable for ScopeArgs {
    type Format = BincodeFormat;
    const ID: &'static str = "sylphie_core::scopes::ScopeArgs";
    const SCHEMA_VERSION: u32 = 0;
}
impl DbSerializable for Scope {
    type Format = BincodeFormat;
    const ID: &'static str = "sylphie_core::scopes::Scope";
    const SCHEMA_VERSION: u32 = 0;
}

impl DbSerializable for StringWrapper {
    type Format = BincodeFormat;
    const ID: &'static str = "std::string::String"; // fully compatible with String
    const SCHEMA_VERSION: u32 = 0;
}
impl DbSerializable for String {
    type Format = BincodeFormat;
    const ID: &'static str = "std::string::String";
    const SCHEMA_VERSION: u32 = 0;
}

// TODO: DbSerializable for integer keys.

/// A simple wrapper that implements [`DbSerializable`] over any compatible type.
///
/// This does not support migrations and serializes using a non self-describing format.
///
/// The schema ID will `"simple_serialize"` with a schema version of 0.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug, Hash, Default)]
#[derive(Serialize)]
#[serde(transparent)]
pub struct SimpleSerialize<T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static>(pub T);
impl <T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static>
    From<T> for SimpleSerialize<T>
{
    fn from(t: T) -> Self {
        SimpleSerialize(t)
    }
}
impl <T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static>
    DbSerializable for SimpleSerialize<T>
{
    type Format = BincodeFormat;

    const ID: &'static str = "simple_serialize";
    const SCHEMA_VERSION: u32 = 0;
}
impl <'de, T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static>
    Deserialize<'de> for SimpleSerialize<T>
{
    fn deserialize<D>(deser: D) -> StdResult<Self, D::Error> where D: Deserializer<'de> {
        T::deserialize(deser).map(SimpleSerialize)
    }
}