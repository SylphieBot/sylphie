use crate::migrations::*;
use crate::serializable::*;
use static_events::prelude_async::*;
use std::collections::HashMap;
use std::marker::PhantomData;
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

struct InitKvsEvent {

}
failable_event!(InitKvsEvent, (), Error);
pub(crate) fn init_kvs(target: &Handler<impl Events>) -> Result<()> {
    let migrations = target.get_service::<MigrationManager>();
    migrations.execute_migration(&PERSISTENT_KVS_MIGRATIONS)?;
    migrations.execute_migration(&TRANSIENT_KVS_MIGRATIONS)?;
    target.dispatch_sync(InitKvsEvent {

    })?;
    Ok(())
}

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

#[derive(Module)]
#[module(component)]
pub struct BaseKvsStore<K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> {
    #[module_info] info: ModuleInfo,
    data: HashMap<K, V>, // TODO: Temp
    phantom: PhantomData<fn(& &mut T)>,
}
#[module_impl]
impl <K: DbSerializable + Hash + Eq, V: DbSerializable, T: KvsType> BaseKvsStore<K, V, T> {

}