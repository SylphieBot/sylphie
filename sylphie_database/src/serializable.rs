use serde::*;
use serde::de::DeserializeOwned;
use sylphie_core::errors::*;

/// A serialization format that can be used with [`DbSerializable`].
pub trait SerializationFormat {

}

/// A trait for types that can be serialized into database columns.
pub trait DbSerializable: Sized + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Whether this type should be stored using an upgradable format.
    ///
    /// If this is set to `true`, this object will be stored in a format that supports attributes
    /// such as `#[serde(default)]` and `#[serde(skip_serializing_if="...")]`. Otherwise, it will
    /// use a format that does not store field name metadata for more efficient storage.
    ///
    /// In particular `serde_cobr` will be used if this is set to `true`, and `bincode` will be
    /// used if this is set to `false`.
    const UPGRADABLE: bool;

    /// The schema version of this particular type.
    ///
    /// This is used to allow for manual migrations.
    const SCHEMA_VERSION: usize;

    /// Loads a value from a outdated KVS store
    fn do_migration(_from_version: usize, _data: &[u8]) -> Result<Option<Self>> {
        Ok(None)
    }
}
