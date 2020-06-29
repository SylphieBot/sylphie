use crate::core::{ShutdownStartedEvent, SylphieHandlerExt};
use crate::interface::{TerminalCommandEvent, Interface};
use crate::module::Module;
use static_events::prelude_async::*;
use std::marker::PhantomData;

#[derive(Events)]
pub struct SylphieEventsImpl<R: Module>(pub PhantomData<R>);

#[events_impl]
impl <R: Module> SylphieEventsImpl<R> {
    #[event_handler(EvBeforeEvent)]
    fn builtin_commands(
        &self, target: &Handler<impl Events>, command: &TerminalCommandEvent,
    ) -> EventResult {
        match command.0.to_ascii_lowercase().as_str().trim() {
            ".help" => {
                info!(target: "[term]", "Built-in commands:");
                info!(target: "[term]", ".help - Shows this help message.");
                info!(target: "[term]", ".info - Prints information about the bot.");
                info!(target: "[term]", ".shutdown - Shuts down the bot.");
                info!(target: "[term]", ".abort!! - Forcefully shuts down the bot.");
            }
            ".info" => {
                for info_line in crate::interface::get_info_string().trim().split('\n') {
                    info!(target: "[term]", "{}", info_line);
                }
            }
            ".shutdown" => target.shutdown_bot(),
            ".abort!!" => {
                eprintln!("(abort)");
                ::std::process::abort()
            }
            x if x.starts_with(".abort") => {
                info!(
                    target: "[term]",
                    "Please use '.abort!!' if you really mean to forcefully stop the bot.",
                );
            }
            x if x.starts_with('.') => {
                error!(
                    target: "[term]",
                    "Unknown built-in command. Use '.help' for more information.",
                );
            }
            _ => return EvOk
        }
        EvCancel
    }

    #[event_handler]
    fn shutdown_handler(&self, target: &Handler<impl Events>, _: &ShutdownStartedEvent) {
        target.get_service::<Interface>().shutdown();
    }
}