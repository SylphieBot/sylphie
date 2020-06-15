use arc_swap::ArcSwapOption;
use crate::commands::commands::Command;
use crate::errors::*;
use fxhash::{FxHashMap, FxHashSet};
use static_events::*;
use std::sync::Arc;
use crate::commands::ctx::CommandCtx;

/// The event used to register commands.
#[derive(Debug, Default)]
pub struct RegisterCommandsEvent {
    commands: Vec<Command>,
}
self_event!(Command);
impl RegisterCommandsEvent {
    /// Registers a new command.
    pub fn register_command(&mut self, command: Command) {
        self.commands.push(command);
    }
}

// TODO: Consider inlining strings here?
#[derive(Clone, Debug)]
struct CommandSet {
    list: Arc<[Command]>,
    // a map of {base command name -> {possible prefix -> [possible commands]}}
    // an unprefixed command looks up an empty prefix
    by_name: FxHashMap<String, FxHashMap<String, Vec<Command>>>,
}
impl CommandSet {
    fn from_event(event: RegisterCommandsEvent) -> Self {
        let list = event.commands;

        let mut used_full_names = FxHashSet::default();
        let mut commands_for_name = FxHashMap::default();
        for command in &list {
            if used_full_names.contains(command.full_name()) {
                warn!(
                    "Found duplicated command `{}`. One of the copies will not be accessible.",
                    command.full_name(),
                );
            } else {
                used_full_names.insert(command.full_name());
                commands_for_name.entry(command.name()).or_insert(Vec::new()).push(command);
            }
        }
        let by_name = commands_for_name.into_iter().map(|(name, variants)| {
            let mut map = FxHashMap::default();
            for variant in variants {
                let mod_name = variant.module_name();
                map.entry(mod_name.to_string()).or_insert(Vec::new()).push(variant.clone());
                map.entry(String::new()).or_insert(Vec::new()).push(variant.clone());
                for (i, _) in mod_name.char_indices().filter(|(_, c)| *c == '.') {
                    let prefix = mod_name[i+1..].to_string();
                    map.entry(prefix).or_insert(Vec::new()).push(variant.clone());
                }
            }
            (name.to_string(), map)
        }).collect();

        CommandSet { list: list.into(), by_name }
    }
}

struct ReloadCommandManagerEvent;
simple_event!(ReloadCommandManagerEvent);

/// The result of a command lookup.
pub enum CommandLookupResult {
    /// No matching commands were found.
    NoneFound,
    /// A single unambigious command was found.
    Found(Command),
    /// An ambigious set of commands was found.
    Ambigious(Vec<Command>),
}

/// The service used to lookup commands.
#[derive(Clone, Debug)]
pub struct CommandManager {
    null: CommandSet,
    data: ArcSwapOption<CommandSet>,
}
impl CommandManager {
    /// Returns a list of all commands currently registered.
    pub fn command_list(&self) -> Arc<[Command]> {
        self.data.load().as_ref().map_or_else(|| self.null.list.clone(), |x| x.list.clone())
    }

    /// Looks ups a command for a given context.
    pub async fn lookup_command(
        &self, ctx: &CommandCtx<impl Events>, command: &str,
    ) -> Result<CommandLookupResult> {
        let split: Vec<_> = command.split(':').collect();
        let (group, name) = match split.as_slice() {
            &[name] => ("", name),
            &[group, name] => (group, name),
            _ => cmd_error!("No more than one `:` can appear in a command name."),
        };

        let data = self.data.load();
        let data = data.as_ref().map_or(&self.null, |x| &*x);
        Ok(match data.by_name.get(name) {
            Some(x) => match x.get(group) {
                Some(x) => {
                    let mut valid_commands = Vec::new();
                    for command in x {
                        if command.can_access(ctx).await? {
                            valid_commands.push(command.clone());
                        }
                    }
                    if valid_commands.len() == 0 {
                        CommandLookupResult::NoneFound
                    } else if valid_commands.len() == 1 {
                        CommandLookupResult::Found(valid_commands.pop().unwrap())
                    } else {
                        CommandLookupResult::Ambigious(valid_commands)
                    }
                },
                None => CommandLookupResult::NoneFound,
            },
            None => CommandLookupResult::NoneFound,
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
                    match cmd.execute(ctx).await {
                        Ok(()) => { }
                        Err(e) => match e.error_kind() {
                            ErrorKind::CommandError(e) => ctx.respond(e).await?,
                            _ => e.report_error(), // TODO: Do something extensible
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
