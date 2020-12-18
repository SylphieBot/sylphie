use arc_swap::ArcSwapOption;
use fxhash::FxHashMap;
use serde::*;
use static_events::prelude_async::*;
use std::sync::Arc;
use sylphie_core::core::InitEvent;
use sylphie_core::derives::*;
use sylphie_core::prelude::*;
use sylphie_utils::scopes::{Scope, ScopeArgs};
use sylphie_database::config::*;
use sylphie_database::serializable::*;
use sylphie_database::singleton::SingletonStore;
use sylphie_utils::strings::InternString;
use tokio::sync::RwLock;

mod types;
pub use types::*;

/// The internal identifier of a connection.
#[derive(Serialize, Deserialize, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct ConnectionId(u64);

#[derive(Serialize, Deserialize, Clone)]
struct ConnectionInfo {
    id: ConnectionId,
    name: Arc<str>,
    kind: Arc<str>,
}
impl ConnectionInfo {
    fn scope(&self) -> Scope {
        Scope::new("sylphie_connections:connection", ScopeArgs::Long(self.id.0))
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct ConnectionState {
    by_name: FxHashMap<Arc<str>, ConnectionId>,
    by_id: FxHashMap<ConnectionId, ConnectionInfo>,
    current_id: u64,
}
impl DbSerializable for ConnectionState {
    type Format = CborFormat;
    const ID: &'static str = "sylphie_connections::ConnectionState";
    const SCHEMA_VERSION: u32 = 0;
}
impl ConnectionState {
    async fn add_connection(
        &mut self, name: &str, kind: &ConnectionType,
    ) -> Result<()> {
        let id = ConnectionId(self.current_id);
        self.current_id += 1;

        if self.by_name.contains_key(name) {
            cmd_error!("Another connection named '{}' already exists.", name);
        }

        let name: Arc<str> = name.into();
        self.by_id.insert(id, ConnectionInfo {
            id,
            name: name.clone(),
            kind: kind.arc_name(),
        });
        self.by_name.insert(name, id);

        Ok(())
    }
    async fn remove_connection(
        &mut self, name: &str,
    ) -> Result<()> {
        if let Some(id) = self.by_name.remove(name) {
            self.by_id.remove(&id);
        } else {
            cmd_error!("No such connection named '{}' exists.", name);
        }
        Ok(())
    }
}

#[derive(Default)]
struct ConnectionLiveState {
    current: ConnectionState,
    instances: FxHashMap<ConnectionId, ConnectionInstance>,
}

/// An event fired to initialize the list of connection types.
///
/// This is only ever fired once in early initialization, and is never called again.
pub struct InitConnectionTypesEvent {
    types: FxHashMap<Arc<str>, ConnectionType>,
}
impl InitConnectionTypesEvent {
    /// Adds a new connection type.
    pub fn add_type<E: Events>(
        &mut self, target: &Handler<E>, name: &str, type_impl: impl ConnectionFactory<E>
    ) -> Result<()> {
        if self.types.contains_key(name) {
            bail!("Duplicate connection type name '{}'", name);
        }

        let name = name.intern();
        let conn_type = ConnectionType::new(name.clone(), target, type_impl);
        self.types.insert(name, conn_type);
        Ok(())
    }
}
failable_self_event!(InitConnectionTypesEvent, Error);

#[derive(Module)]
#[service]
pub struct ConnectionManager {
    #[module_info] info: ModuleInfo,
    #[submodule] state: SingletonStore<ConnectionState>,
    live_state: RwLock<ConnectionLiveState>,
    types: ArcSwapOption<FxHashMap<Arc<str>, ConnectionType>>,
}
#[module_impl]
impl ConnectionManager {
    #[event_handler]
    async fn init(&self, target: &Handler<impl Events>, _: &InitEvent) -> Result<()> {
        let types = target.dispatch_sync(InitConnectionTypesEvent {
            types: Default::default(),
        })?.types;
        self.types.store(Some(Arc::new(types)));
        self.update(target).await
    }

    async fn update(&self, target: &Handler<impl Events>) -> Result<()> {
        let state = self.state.get().await;
        let mut live_state = self.live_state.write().await;
        let live_state = &mut *live_state;
        let types = self.types.load();
        let types = types.as_ref().expect("ConnectionManager not yet initialized.");

        // Set the current connection state.
        live_state.current = state;

        // Deletes connections whose type were changed or that no longer exist.
        let mut to_remove = Vec::new();
        for (id, data) in &live_state.current.by_id {
            if let Some(current) = live_state.instances.get_mut(id) {
                if current.conn_type().name() != &*data.kind {
                    to_remove.push(*id);
                }
            }
        }
        for (id, _) in &live_state.instances {
            if !live_state.current.by_id.contains_key(id) {
                to_remove.push(*id);
            }
        }
        for id in to_remove {
            if let Err(err) = live_state.instances.remove(&id).unwrap().destroy(target).await {
                err.report_error();
            }
        }

        // Creates/recreates connections that are needed.
        for (id, data) in &live_state.current.by_id {
            if !live_state.instances.contains_key(&id) {
                let conn_type = match types.get(&data.kind) {
                    Some(t) => t,
                    None => bail!("Connection type '{}' does not exist!", data.kind),
                };
                live_state.instances.insert(
                    *id, conn_type.new_connection(target, *id, data.scope()).await?,
                );
            }
        }

        Ok(())
    }

    /// Creates a new connection.
    pub async fn add_connection(
        &mut self, target: &Handler<impl Events>, name: &str, kind: &ConnectionType,
    ) -> Result<()> {
        let mut state = self.state.get_mut().await?;
        state.add_connection(name, kind).await?;
        state.commit().await?;
        self.update(target).await?;
        Ok(())
    }

    /// Removes an old connection.
    pub async fn remove_connection(
        &mut self, target: &Handler<impl Events>, name: &str,
    ) -> Result<()> {
        let mut state = self.state.get_mut().await?;
        state.remove_connection(name).await?;
        state.commit().await?;
        self.update(target).await?;
        Ok(())
    }
}