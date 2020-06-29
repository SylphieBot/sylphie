use crate::commands::ctx::*;
use crate::commands::manager::CommandManager;
use crate::core::{ShutdownStartedEvent, SylphieHandlerExt, InitEvent, CoreRef};
use crate::errors::*;
use crate::interface::{TerminalCommandEvent, Interface};
use crate::module::Module;
use futures::*;
use futures::future::BoxFuture;
use static_events::prelude_async::*;
use std::marker::PhantomData;

#[derive(Events)]
pub struct SylphieEventsImpl<R: Module>(pub PhantomData<R>);

#[events_impl]
impl <R: Module> SylphieEventsImpl<R> {
    #[event_handler]
    async fn init_commands(target: &Handler<impl Events>, _: &InitEvent) {
        target.get_service::<CommandManager>().reload(target).await;
    }

    #[event_handler(EvBeforeEvent)]
    fn builtin_commands(
        &self, target: &Handler<impl Events>, command: &TerminalCommandEvent,
    ) -> EventResult {
        match command.0.as_str().trim() {
            ".help" => {
                info!("Built-in commands:");
                info!(".help - Shows this help message.");
                info!(".info - Prints information about the bot.");
                info!(".shutdown - Shuts down the bot.");
                info!(".abort!! - Forcefully shuts down the bot.");
            }
            ".info" => {
                // TODO: Implement.
            }
            ".shutdown" => target.shutdown_bot(),
            ".abort!!" => {
                eprintln!("(abort)");
                ::std::process::abort()
            }
            x if x.starts_with(".abort") => {
                info!("Please use '.abort!!' if you really mean to forcefully stop the bot.");
            }
            x if x.starts_with('.') => {
                error!("Unknown built-in command. Use '.help' for more information.");
            }
            _ => return EvOk
        }
        EvCancel
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
    fn shutdown_handler(&self, target: &Handler<impl Events>, _: &ShutdownStartedEvent) {
        target.get_service::<Interface>().shutdown();
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
            info!(target: "sylphie_core", "{}", msg);
            Ok(())
        }.boxed()
    }
}