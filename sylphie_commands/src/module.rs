use async_trait::*;
use crate::commands::*;
use crate::ctx::*;
use crate::manager::*;
use std::time::Instant;
use sylphie_core::core::{SylphieEvents, InitEvent};
use sylphie_core::derives::*;
use sylphie_core::interface::{TerminalCommandEvent, SetupLoggerEvent};
use sylphie_core::prelude::*;
use sylphie_utils::scopes::*;
use sylphie_utils::strings::StringWrapper;

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
        let ctx = CommandCtx::new(target, TerminalContext {
            raw_message: command.0.clone(),
        });
        let start_time = Instant::now();
        if let Err(e) = target.get_service::<CommandManager>().execute(&ctx).await {
            e.report_error();
        } else {
            let total_time = (Instant::now() - start_time).as_millis();
            info!(target: "[term]", "Command completed in {} ms", total_time);
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
#[async_trait]
impl CommandCtxImpl for TerminalContext {
    fn scopes(&self) -> &[Scope] {
        const SCOPES: [Scope; 1] = [Scope {
            scope_type: StringWrapper::Static("terminal"),
            args: ScopeArgs::None,
        }];
        &SCOPES
    }

    fn raw_message(&self) -> &str {
        &self.raw_message
    }

    async fn respond<E: Events>(&self, _: &Handler<E>, msg: &str) -> Result<()> {
        info!(target: "[term]", "{}", msg);
        Ok(())
    }
}