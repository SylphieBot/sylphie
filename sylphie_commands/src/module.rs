use crate::commands::*;
use crate::ctx::*;
use crate::manager::*;
use futures::FutureExt;
use futures::future::BoxFuture;
use sylphie_core::core::{SylphieEvents, InitEvent};
use sylphie_core::derives::*;
use sylphie_core::interface::{TerminalCommandEvent, SetupLoggerEvent};
use sylphie_core::prelude::*;

/// The module containing the implementation of Sylphie commands.
#[derive(Module)]
#[module(integral)]
pub struct CommandsModule<R: Module> {
    #[module_info] info: ModuleInfo,

    #[subhandler] #[init_with { CommandImplConstructor::new() }]
    command_constructor: CommandImplConstructor<SylphieEvents<R>>,
    #[service] #[init_with { CommandManager::new() }]
    cmd_manager: CommandManager,
}

#[module_impl]
impl <R: Module> CommandsModule<R> {
    #[event_handler]
    async fn init_commands(target: &Handler<impl Events>, _: &InitEvent) {
        target.get_service::<CommandManager>().reload(target).await;
    }

    #[event_handler]
    async fn run_terminal_command(
        &self, target: &Handler<impl Events>, command: &TerminalCommandEvent,
    ) {
        let ctx = CommandCtx::new(target.get_service::<CoreRef<R>>(), TerminalContext {
            raw_message: command.0.clone(),
        });
        if let Err(e) = target.get_service::<CommandManager>().execute(&ctx).await {
            e.report_error();
        }
    }

    #[event_handler]
    fn setup_logger(ev: &mut SetupLoggerEvent) {
        ev.add_console_directive("sylphie_commands=debug");
    }
}

struct TerminalContext {
    raw_message: String,
}
impl CommandCtxImpl for TerminalContext {
    fn raw_message(&self) -> &str {
        &self.raw_message
    }

    fn respond<'a>(
        &'a self, _: &'a Handler<impl Events>, msg: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        async move {
            info!(target: "[term]", "{}", msg);
            Ok(())
        }.boxed()
    }
}