use async_trait::*;
use std::any::Any;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use sylphie_core::prelude::*;
use sylphie_utils::scopes::Scope;
use sylphie_database::utils::ScopeId;

/// Returns the status of the connection.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub enum ConnectionStatus {
    /// The connection is connected.
    Connected,
    /// The connection is partly connected, but there are transient connection issues for
    /// one reason or another.
    PartlyConnected,
    /// The connection is not connected.
    Disconnected,
    /// The connection is deactivated intentionally and will not attempt to connect.
    Deactivated,
}

#[async_trait]
pub trait Connection<E: Events>: Send + Sync + 'static {
    /// Returns the status of this connection.
    async fn status(&self, target: &Handler<E>) -> ConnectionStatus;

    /// An event that is triggered when this connection is updated.
    async fn update_connection(&self, target: &Handler<E>) -> Result<()>;

    /// An event that is triggered when an connection is destroyed.
    async fn destroy(&self, target: &Handler<E>) -> Result<()>;
}

#[async_trait]
pub trait ConnectionFactory<E: Events>: Send + Sync + 'static {
    type Connection: Connection<E>;

    /// Creates a new connection of this type, with the given scope name.
    async fn create(
        &self, target: &Handler<E>, id: crate::ConnectionId, scope: Scope,
    ) -> Result<Self::Connection>;
}

#[async_trait]
trait ErasedConnection: Send + Sync + 'static {
    /// Returns the status of this connection.
    async fn status(&self, target: &(dyn Any + Send + Sync)) -> ConnectionStatus;

    /// An event that is triggered when this connection is updated.
    async fn update_connection(&self, target: &(dyn Any + Send + Sync)) -> Result<()>;

    /// An event that is triggered when an event is destroyed.
    async fn destroy(&self, target: &(dyn Any + Send + Sync)) -> Result<()>;
}
struct ConnectionWrapper<E: Events, C: Connection<E>>(C, PhantomData<E>);
#[async_trait]
impl <E: Events, C: Connection<E>> ErasedConnection for ConnectionWrapper<E, C> {
    async fn status(&self, target: &(dyn Any + Send + Sync)) -> ConnectionStatus {
        let target = target.downcast_ref().expect("Wrong Dispatch type passed!");
        self.0.status(target).await
    }
    async fn update_connection(&self, target: &(dyn Any + Send + Sync)) -> Result<()> {
        let target = target.downcast_ref().expect("Wrong Dispatch type passed!");
        self.0.update_connection(target).await
    }
    async fn destroy(&self, target: &(dyn Any + Send + Sync)) -> Result<()> {
        let target = target.downcast_ref().expect("Wrong Dispatch type passed!");
        self.0.destroy(target).await
    }
}

#[async_trait]
trait ErasedConnectionFactory: Send + Sync + 'static {
    /// Creates a new connection of this type, with the given scope name.
    async fn create(
        &self, target: &(dyn Any + Send + Sync), id: crate::ConnectionId, scope: Scope,
    ) -> Result<Box<dyn ErasedConnection>>;
}
struct FactoryWrapper<E: Events, F: ConnectionFactory<E>>(F, PhantomData<E>);
#[async_trait]
impl <E: Events, F: ConnectionFactory<E>> ErasedConnectionFactory for FactoryWrapper<E, F> {
    async fn create(
        &self, target: &(dyn Any + Send + Sync), id: crate::ConnectionId, scope: Scope,
    ) -> Result<Box<dyn ErasedConnection>> {
        let target = target.downcast_ref().expect("Wrong Dispatch type passed!");
        let underlying = self.0.create(target, id, scope).await?;
        Ok(Box::new(ConnectionWrapper(underlying, PhantomData)))
    }
}

pub struct ConnectionType(Arc<ConnectionTypeData>);
struct ConnectionTypeData {
    name: Arc<str>,
    inner: Box<dyn ErasedConnectionFactory>,
}
impl Clone for ConnectionType {
    fn clone(&self) -> Self {
        ConnectionType(self.0.clone())
    }
}
impl ConnectionType {
    pub(crate) fn new<E: Events>(
        name: Arc<str>, _target: &Handler<E>, factory: impl ConnectionFactory<E>,
    ) -> Self {
        ConnectionType(Arc::new(ConnectionTypeData {
            name,
            inner: Box::new(FactoryWrapper(factory, PhantomData))
        }))
    }

    /// Returns the name of this connection type.
    pub fn name(&self) -> &str {
        &self.0.name
    }

    /// Returns the name of this connection type as an `Arc<str>`
    pub fn arc_name(&self) -> Arc<str> {
        self.0.name.clone()
    }

    pub(crate) async fn new_connection(
        &self, target: &Handler<impl Events>, id: crate::ConnectionId, scope: Scope,
    ) -> Result<ConnectionInstance> {
        let conn = self.0.inner.create(target, id, scope.clone()).await?;
        let scope_id = ScopeId::intern(target, scope.clone()).await?;
        Ok(ConnectionInstance(Arc::new(ConnectionInstanceData {
            id,
            scope,
            scope_id,
            conn_type: self.clone(),
            inner: conn,
        })))
    }
}

/// A particular instance of an connection.
pub struct ConnectionInstance(Arc<ConnectionInstanceData>);
struct ConnectionInstanceData {
    id: crate::ConnectionId,
    scope: Scope,
    scope_id: ScopeId,
    conn_type: ConnectionType,
    inner: Box<dyn ErasedConnection>,
}
impl Clone for ConnectionInstance {
    fn clone(&self) -> Self {
        ConnectionInstance(self.0.clone())
    }
}
impl ConnectionInstance {
    /// Returns the internal ID of this connection.
    pub fn id(&self) -> crate::ConnectionId {
        self.0.id
    }

    /// Returns the base scope of this connection.
    pub fn scope(&self) -> &Scope {
        &self.0.scope
    }

    /// Returns the interned scope ID of this connection.
    pub fn scope_id(&self) -> ScopeId {
        self.0.scope_id
    }

    /// Returns the type of the connection.
    pub fn conn_type(&self) -> &ConnectionType {
        &self.0.conn_type
    }

    /// Returns the status of this connection.
    pub async fn status(&self, target: &Handler<impl Events>) -> ConnectionStatus {
        self.0.inner.status(target).await
    }

    /// Updates the connection.
    ///
    /// This will generally cause the connection to check its configuration and reconnect if
    /// anything has changed.
    pub async fn update_connection(&self, target: &Handler<impl Events>) -> Result<()> {
        self.0.inner.update_connection(target).await
    }

    pub(crate) async fn destroy(&self, target: &Handler<impl Events>) -> Result<()> {
        self.0.inner.destroy(target).await
    }
}