use arc_swap::ArcSwapOption;
use crate::commands::Command;
use crate::ctx::CommandCtx;
use static_events::prelude_async::*;
use std::sync::Arc;
use sylphie_core::errors::*;
use sylphie_utils::disambiguate::{DisambiguatedSet, Disambiguated, LookupResult};

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

/// The result of a command lookup.
pub type CommandLookupResult = LookupResult<Command>;

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
            null: DisambiguatedSet::new("command", Vec::new()),
            data: ArcSwapOption::new(None),
        }))
    }

    /// Reloads the command manager.
    pub async fn reload(&self, target: &Handler<impl Events>) {
        let commands = target.dispatch_async(RegisterCommandsEvent {
            commands: Vec::new(),
        }).await.commands;
        let mut marked_commands = Vec::new();
        for command in commands {
            let name = command.entry_name().clone();
            marked_commands.push((name, command));
        }

        let new_set = DisambiguatedSet::new("command", marked_commands);
        self.0.data.store(Some(Arc::new(new_set)));
    }

    /// Returns a list of all commands currently registered.
    pub fn command_list(&self) -> Arc<[Disambiguated<Command>]> {
        self.0.data.load().as_ref()
            .map_or_else(|| self.0.null.list_arc(), |x| x.list_arc())
    }

    /// Looks up a raw command, without regard for permissions, etc.
    pub fn lookup_command_raw(
        &self, command: &str,
    ) -> Result<LookupResult<Disambiguated<Command>>> {
        let data = self.0.data.load();
        let data = data.as_ref().map_or(&self.0.null, |x| &*x);
        data.resolve(command)
    }

    /// Looks ups a command for a given context.
    pub async fn lookup_command(
        &self, ctx: &CommandCtx<impl Events>, command: &str,
    ) -> Result<CommandLookupResult> {
        let data = self.0.data.load();
        let data = data.as_ref().map_or(&self.0.null, |x| &*x);

        let mut valid_commands = Vec::new();
        for command in data.resolve_iter(command)? {
            if command.value.can_access(ctx).await? {
                valid_commands.push(command.value.clone());
            }
        }
        Ok(CommandLookupResult::new(valid_commands))
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
