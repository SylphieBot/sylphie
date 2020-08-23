use bincode::Options;
use serde::*;
use serde::de::{DeserializeOwned, Visitor, Error as DeError};
use serde_bytes::ByteBuf;
use std::any::Any;
use std::sync::Arc;
use sylphie_core::prelude::*;
use sylphie_utils::scopes::*;
use sylphie_utils::strings::StringWrapper;

/// The input or output of a [`SerializationFormat`].
#[derive(Clone, Debug)]
pub enum SerializeValue {
    Null,
    String(Arc<str>),
    Bytes(Arc<[u8]>),
    Integer(i64),
    Floating(f64),
}
impl SerializeValue {
    pub fn into_str(self) -> Result<Arc<str>> {
        if let SerializeValue::String(s) = self {
            Ok(s)
        } else {
            bail!("Value is not a string!")
        }
    }
    pub fn into_bytes(self) -> Result<Arc<[u8]>> {
        if let SerializeValue::Bytes(b) = self {
            Ok(b)
        } else {
            bail!("Value is not a byte array!")
        }
    }
    pub fn into_u64(self) -> Result<u64> {
        if let SerializeValue::Integer(i) = self {
            Ok(i as u64)
        } else {
            bail!("Value is not an integer!")
        }
    }
    pub fn into_i64(self) -> Result<i64> {
        if let SerializeValue::Integer(i) = self {
            Ok(i)
        } else {
            bail!("Value is not an integer!")
        }
    }
    pub fn into_f64(self) -> Result<f64> {
        if let SerializeValue::Floating(f) = self {
            Ok(f)
        } else {
            bail!("Value is not a floating point number!")
        }
    }
}
impl From<Arc<str>> for SerializeValue {
    fn from(v: Arc<str>) -> Self {
        SerializeValue::String(v)
    }
}
impl From<String> for SerializeValue {
    fn from(v: String) -> Self {
        SerializeValue::String(v.into())
    }
}
impl From<Arc<[u8]>> for SerializeValue {
    fn from(v: Arc<[u8]>) -> Self {
        SerializeValue::Bytes(v)
    }
}
impl From<Vec<u8>> for SerializeValue {
    fn from(v: Vec<u8>) -> Self {
        SerializeValue::Bytes(v.into())
    }
}

impl Serialize for SerializeValue {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error> where S: Serializer {
        match self {
            SerializeValue::Null => serializer.serialize_none(),
            SerializeValue::String(s) => serializer.serialize_str(&s),
            SerializeValue::Bytes(b) => serializer.serialize_bytes(&b),
            SerializeValue::Integer(i) => serializer.serialize_i64(*i),
            SerializeValue::Floating(f) => serializer.serialize_f64(*f),
        }
    }
}
impl <'de> Deserialize<'de> for SerializeValue {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error> where D: Deserializer<'de> {
        deserializer.deserialize_any(SerializeValueVisitor)
    }
}

struct SerializeValueVisitor;
impl <'de> Visitor<'de> for SerializeValueVisitor {
    type Value = SerializeValue;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an sqlite serializable value")
    }


    fn visit_i64<E>(self, v: i64) -> StdResult<Self::Value, E> where E: DeError {
        Ok(SerializeValue::Integer(v))
    }
    fn visit_u64<E>(self, v: u64) -> StdResult<Self::Value, E> where E: DeError {
        Ok(SerializeValue::Integer(v as i64))
    }

    fn visit_str<E>(self, v: &str) -> StdResult<Self::Value, E> where E: DeError {
        Ok(SerializeValue::String(v.into()))
    }
    fn visit_string<E>(self, v: String) -> StdResult<Self::Value, E> where E: DeError {
        Ok(SerializeValue::String(v.into()))
    }

    fn visit_bytes<E>(self, v: &[u8]) -> StdResult<Self::Value, E> where E: DeError {
        Ok(SerializeValue::Bytes(v.into()))
    }
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> StdResult<Self::Value, E> where E: DeError {
        Ok(SerializeValue::Bytes(v.into()))
    }
}

/// A format that can be used to serialize database values.
pub trait SerializationFormat<T: DbSerializable> {
    fn serialize(val: &T) -> Result<SerializeValue>;
    fn deserialize(val: SerializeValue) -> Result<T>;
}

/// A [`SerializationFormat`] that serializes in a combat non-self-describing binary form.
pub enum BincodeFormat { }
impl <T: DbSerializable> SerializationFormat<T> for BincodeFormat {
    fn serialize(val: &T) -> Result<SerializeValue> {
        Ok(bincode::DefaultOptions::new().with_varint_encoding().serialize(val)?.into())
    }
    fn deserialize(val: SerializeValue) -> Result<T> {
        Ok(bincode::DefaultOptions::new().with_varint_encoding().deserialize(&val.into_bytes()?)?)
    }
}

/// A [`SerializationFormat`] that serializes a value as CBOR.
pub enum CborFormat { }
impl <T: DbSerializable> SerializationFormat<T> for CborFormat {
    fn serialize(val: &T) -> Result<SerializeValue> {
        Ok(serde_cbor::to_vec(val)?.into())
    }
    fn deserialize(val: SerializeValue) -> Result<T> {
        Ok(serde_cbor::from_slice(&val.into_bytes()?)?)
    }
}

/// A trait for types that can be serialized into database columns.
pub trait DbSerializable: Clone + Sized + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The serialization format that will be used for this trait.
    type Format: SerializationFormat<Self>;

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
        _from_id: &str, _from_version: u32, _data: SerializeValue,
    ) -> Result<Self> {
        bail!("Migration not supported.")
    }

    /// Downcasts this to a concrete type. This is used for some more fancy formatters.
    fn downcast_ref<T: Any>(&self) -> Option<&T> {
        let as_any: &dyn Any = self;
        as_any.downcast_ref::<T>()
    }
}

mod private {
    use super::*;

    pub enum DirectFormats {}
    impl SerializationFormat<Vec<u8>> for DirectFormats {
        fn serialize(val: &Vec<u8>) -> Result<SerializeValue> {
            Ok(val.clone().into())
        }
        fn deserialize(val: SerializeValue) -> Result<Vec<u8>> {
            Ok(val.into_bytes()?.to_vec())
        }
    }
    impl SerializationFormat<ByteBuf> for DirectFormats {
        fn serialize(val: &ByteBuf) -> Result<SerializeValue> {
            Ok(val.to_vec().into())
        }
        fn deserialize(val: SerializeValue) -> Result<ByteBuf> {
            Ok(ByteBuf::from(val.into_bytes()?.to_vec()))
        }
    }
    impl SerializationFormat<String> for DirectFormats {
        fn serialize(val: &String) -> Result<SerializeValue> {
            Ok(val.clone().into())
        }
        fn deserialize(val: SerializeValue) -> Result<String> {
            Ok(val.into_str()?.to_string())
        }
    }
    impl SerializationFormat<StringWrapper> for DirectFormats {
        fn serialize(val: &StringWrapper) -> Result<SerializeValue> {
            Ok(val.as_arc().into())
        }
        fn deserialize(val: SerializeValue) -> Result<StringWrapper> {
            Ok(StringWrapper::Shared(val.into_str()?))
        }
    }

    macro_rules! integral {
        ($($num:ident)*) => {$(
            impl SerializationFormat<$num> for DirectFormats {
                fn serialize(val: &$num) -> Result<SerializeValue> {
                    Ok(SerializeValue::Integer(*val as i64))
                }
                fn deserialize(val: SerializeValue) -> Result<$num> {
                    Ok(val.into_u64()? as $num)
                }
            }
        )*};
    }
    integral!(
        u8 u16 u32 u64 u128 usize
        i8 i16 i32 i64 i128 isize
    );

    impl SerializationFormat<f32> for DirectFormats {
        fn serialize(val: &f32) -> Result<SerializeValue> {
            Ok(SerializeValue::Floating(*val as f64))
        }
        fn deserialize(val: SerializeValue) -> Result<f32> {
            Ok(val.into_f64()? as f32)
        }
    }
    impl SerializationFormat<f64> for DirectFormats {
        fn serialize(val: &f64) -> Result<SerializeValue> {
            Ok(SerializeValue::Floating(*val))
        }
        fn deserialize(val: SerializeValue) -> Result<f64> {
            Ok(val.into_f64()?)
        }
    }
}

macro_rules! basic_defs {
    (@impl_for $ty:ty, ($id:literal, $format:ty)) => {
        impl DbSerializable for $ty {
            type Format = $format;
            const ID: &'static str = $id;
            const SCHEMA_VERSION: u32 = 0;
        }
    };
    (@impl_for $ty:ty, $id:literal) => {
        basic_defs!(@impl_for $ty, ($id, BincodeFormat));
    };
    ($($ty:ty => $data:tt),* $(,)?) => {$(
        basic_defs!(@impl_for $ty, $data);
    )*};
}
basic_defs! {
    // strings
    String => ("direct_str", private::DirectFormats),
    StringWrapper => ("direct_str", private::DirectFormats),

    // byte buffers
    Vec<u8> => ("direct_bytes", private::DirectFormats),
    ByteBuf => ("direct_bytes", private::DirectFormats),

    // integers
    u8 => ("direct_int", private::DirectFormats),
    u16 => ("direct_int", private::DirectFormats),
    u32 => ("direct_int", private::DirectFormats),
    u64 => ("direct_u64", private::DirectFormats),
    u128 => "uvarint",
    usize => ("direct_int", private::DirectFormats),
    i8 => ("direct_int", private::DirectFormats),
    i16 => ("direct_int", private::DirectFormats),
    i32 => ("direct_int", private::DirectFormats),
    i64 => ("direct_int", private::DirectFormats),
    i128 => "ivarint",
    isize => ("direct_int", private::DirectFormats),

    // floating point
    f32 => ("direct_float", private::DirectFormats),
    f64 => ("direct_float", private::DirectFormats),

    // scope definitions
    Scope => "sylphie_utils::scopes::Scope",
    ScopeArgs => "sylphie_utils::scopes::ScopeArgs",
}

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