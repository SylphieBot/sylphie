use crate::commands::ctx::CommandCtx;
use crate::errors::*;
use crate::module::*;
use derive_setters::*;
use futures::*;
use futures::future::BoxFuture;
use static_events::*;
use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

/// The metadata relating to a command.
#[derive(Debug, Setters)]
#[setters(strip_option)]
#[non_exhaustive]
pub struct CommandInfo {
    /// The name of the command.
    pub name: Cow<'static, str>,
}
impl CommandInfo {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        CommandInfo {
            name: name.into(),
        }
    }
}

/// The implementation of a command.
pub trait CommandImpl: Send + Sync + 'static {
    /// Checks if a the user can access this command.
    fn can_access<'a>(
        &'a self, _cmd: Command, _ctx: &'a CommandCtx<impl Events>,
    ) -> BoxFuture<'a, Result<bool>> {
        async { Ok(true) }.boxed()
    }

    /// Executes the actual command.
    fn execute<'a>(
        &'a self, cmd: Command, ctx: &'a CommandCtx<impl Events>,
    ) -> BoxFuture<'a, Result<()>>;
}

/// A fully resolved command.
#[derive(Clone)]
pub struct Command(Arc<CommandData>);
struct CommandData {
    module_name: Arc<str>,
    module_info: Option<ModuleInfo>,
    info: CommandInfo,
    full_name: String,
    command_impl: Box<dyn CommandImplWrapper>,
}
impl Command {
    /// Creates a new command.
    pub fn new(
        target: &Handler<impl Events>, defined_from: &impl Module, info: CommandInfo,
        command: impl CommandImpl,
    ) -> Self {
        Self::new_0(
            defined_from.info().arc_name(), Some(defined_from.info()), info,
            Self::construct_wrapper(target, command),
        )
    }

    /// Creates a new command that is not an inherent part of a module.
    pub fn new_dynamic(
        target: &Handler<impl Events>, name: impl Into<Arc<str>>, info: CommandInfo,
        command: impl CommandImpl,
    ) -> Self {
        Self::new_0(name.into(), None, info, Self::construct_wrapper(target, command))
    }

    fn construct_wrapper<E: Events>(
        target: &Handler<E>, command: impl CommandImpl,
    ) -> Box<dyn CommandImplWrapper> {
        target.dispatch(ConstructWrapperEvent(Some(command)))
            .expect("`Command`s may only be constructed in `SylphieCore`s.")
    }
    fn new_0(
        module_name: Arc<str>, module_info: Option<&ModuleInfo>, cmd_info: CommandInfo,
        command_impl: Box<dyn CommandImplWrapper>,
    ) -> Self {
        let full_name = format!("{}:{}", module_name, cmd_info.name);
        Command(Arc::new(CommandData {
            module_name,
            module_info: module_info.map(Clone::clone),
            info: cmd_info,
            full_name,
            command_impl,
        }))
    }

    /// Checks whether the command can be executed in a given context.
    pub async fn can_access(&self, ctx: &CommandCtx<impl Events>) -> Result<bool> {
        self.0.command_impl.can_access(self.clone(), ctx)?.await
    }

    /// Executes the command in a given context.
    pub async fn execute(&self, ctx: &CommandCtx<impl Events>) -> Result<()> {
        self.0.command_impl.execute(self.clone(), ctx)?.await
    }

    /// Returns the name of the module this command is defined in.
    ///
    /// This is not necessarily the module is actually defined in, or a module that actually
    /// exists. This is primarily meant for use in disambiguating commands.
    pub fn module_name(&self) -> &str {
        &self.0.module_name
    }

    /// Returns the short name of the command.
    pub fn name(&self) -> &str {
        &self.0.info.name
    }

    /// Returns the full name of the command.
    pub fn full_name(&self) -> &str {
        &self.0.full_name
    }

    /// Returns information about the module that defines this command, if one exists.
    pub fn module_info(&self) -> Option<&ModuleInfo> {
        self.0.module_info.as_ref()
    }

    /// Returns information about this command.
    pub fn info(&self) -> &CommandInfo {
        &self.0.info
    }
}
impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[Command '{}']", self.0.full_name)
    }
}

#[inline(never)] #[cold]
fn type_mismatch_0() -> Result<()> {
    bail!("Type mismatch in CommandImplWrapper! \
           Please do not mix `Command`s from different `SylphieCore`s.")
}
fn type_mismatch<T>() -> Result<T> {
    type_mismatch_0()?;
    unreachable!();
}

struct ConstructWrapperEvent<C: CommandImpl>(Option<C>);
simple_event!([C: CommandImpl] ConstructWrapperEvent<C>, Option<Box<dyn CommandImplWrapper>>);

#[derive(Events)]
pub(crate) struct CommandImplConstructor<E: Events>(pub PhantomData<E>);
#[events_impl]
impl <E: Events> CommandImplConstructor<E> {
    #[event_handler]
    fn construct_wrapper<C: CommandImpl>(
        ev: &mut ConstructWrapperEvent<C>, state: &mut Option<Box<dyn CommandImplWrapper>>,
    ) {
        let command = ev.0.take().expect("None encountered while making command impl?");
        *state = Some(Box::new(CommandImplTypeMarker(command, PhantomData::<fn(E)>)));
    }
}

/// An object-safe wrapper around [`CommandImpl`].
trait CommandImplWrapper: Send + Sync + 'static {
    fn can_access<'a>(
        &'a self, cmd: Command, ctx: &'a (dyn Any + Send + Sync),
    ) -> Result<BoxFuture<'a, Result<bool>>>;
    fn execute<'a>(
        &'a self, cmd: Command, ctx: &'a (dyn Any + Send + Sync),
    ) -> Result<BoxFuture<'a, Result<()>>>;
}
struct CommandImplTypeMarker<T, E>(T, PhantomData<fn(E)>);
impl <T: CommandImpl, E: Events> CommandImplWrapper for CommandImplTypeMarker<T, E> {
    fn can_access<'a>(
        &'a self, cmd: Command, ctx: &'a (dyn Any + Send + Sync),
    ) -> Result<BoxFuture<'a, Result<bool>>> {
        match ctx.downcast_ref::<CommandCtx<E>>() {
            Some(x) => Ok(CommandImpl::can_access(&self.0, cmd, x)),
            None => type_mismatch(),
        }
    }
    fn execute<'a>(
        &'a self, cmd: Command, ctx: &'a (dyn Any + Send + Sync),
    ) -> Result<BoxFuture<'a, Result<()>>> {
        match ctx.downcast_ref::<CommandCtx<E>>() {
            Some(x) => Ok(CommandImpl::execute(&self.0, cmd, x)),
            None => type_mismatch(),
        }
    }
}
