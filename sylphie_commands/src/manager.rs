use arc_swap::ArcSwapOption;
use crate::commands::Command;
use crate::ctx::CommandCtx;
use static_events::prelude_async::*;
use std::sync::Arc;
use sylphie_core::errors::*;
use sylphie_core::utils::{CanDisambiguate, DisambiguatedSet, Disambiguated};

/// The event used to register commands.
#[derive(Debug, Default)]
pub struct RegisterCommandsEvent {
    commands: Vec<Command>,
}
self_event!(RegisterCommandsEvent);
impl RegisterCommandsEvent {
    /// Registers a new command.
    pub fn register_command(&mut self, command: Command) {
        self.commands.push(command);
    }
}

impl CanDisambiguate for Command {
    const CLASS_NAME: &'static str = "command";

    fn name(&self) -> &str {
        self.name()
    }
    fn full_name(&self) -> &str {
        self.full_name()
    }
    fn module_name(&self) -> &str {
        self.module_name()
    }
}

/// The result of a command lookup.
pub enum CommandLookupResult {
    /// No matching commands were found.
    NoneFound,
    /// A single unambiguous command was found.
    Found(Command),
    /// An ambiguous set of commands was found.
    Ambigious(Vec<Command>),
}

/// The service used to lookup commands.
#[derive(Clone, Debug)]
pub struct CommandManager(Arc<CommandManagerData>);
#[derive(Debug)]
struct CommandManagerData {
    null: DisambiguatedSet<Command>,
    data: ArcSwapOption<DisambiguatedSet<Command>>,
}
impl CommandManager {
    pub(crate) fn new() -> Self {
        CommandManager(Arc::new(CommandManagerData {
            null: DisambiguatedSet::new(Vec::new()),
            data: ArcSwapOption::new(None),
        }))
    }

    /// Reloads the command manager.
    pub async fn reload(&self, target: &Handler<impl Events>) {
        let new_set = DisambiguatedSet::new(target.dispatch_async(RegisterCommandsEvent {
            commands: Vec::new(),
        }).await.commands);
        self.0.data.store(Some(Arc::new(new_set)));
    }

    /// Returns a list of all commands currently registered.
    pub fn command_list(&self) -> Arc<[Arc<Disambiguated<Command>>]> {
        self.0.data.load().as_ref()
            .map_or_else(|| self.0.null.all_commands(), |x| x.all_commands())
    }

    /// Looks ups a command for a given context.
    pub async fn lookup_command(
        &self, ctx: &CommandCtx<impl Events>, command: &str,
    ) -> Result<CommandLookupResult> {
        let data = self.0.data.load();
        let data = data.as_ref().map_or(&self.0.null, |x| &*x);

        let mut valid_commands = Vec::new();
        for command in data.resolve(command)? {
            if command.value.can_access(ctx).await? {
                valid_commands.push(command.value.clone());
            }
        }

        Ok(if valid_commands.len() == 0 {
            CommandLookupResult::NoneFound
        } else if valid_commands.len() == 1 {
            CommandLookupResult::Found(valid_commands.pop().unwrap())
        } else {
            CommandLookupResult::Ambigious(valid_commands)
        })
    }

    /// Executes a command immediately.
    pub async fn execute(&self, ctx: &CommandCtx<impl Events>) -> Result<()> {
        if ctx.args_count() == 0 {
            ctx.respond("Command context contains no arguments?").await?;
        } else {
            let command = self.lookup_command(&ctx, ctx.arg(0).text).await?;
            match command {
                CommandLookupResult::NoneFound => ctx.respond("No such command found.").await?,
                CommandLookupResult::Found(cmd) => {
                    match Error::catch_panic_async(cmd.execute(ctx)).await {
                        Ok(()) => { }
                        Err(e) => {
                            // split to avoid saving a `&ErrorKind` which is !Send
                            let maybe_respond = match e.error_kind() {
                                ErrorKind::CommandError(e) => Some(e),
                                _ => { // TODO: Do something extensible
                                    e.report_error();
                                    None
                                },
                            };
                            if let Some(e) = maybe_respond {
                                ctx.respond(e).await?;
                            }
                        },
                    }
                }
                CommandLookupResult::Ambigious(cmds) => {
                    let mut str = String::new();
                    for cmd in cmds {
                        str.push_str(&format!("{}, ", cmd.full_name()));
                    }
                    ctx.respond(&format!("Command is ambiguous: {}", str)).await?;
                }
            }
        }
        Ok(())
    }
}
