use sylphie::commands::manager::CommandManager;
use sylphie::prelude::*;
use sylphie::utils::disambiguate::LookupResult;

/// A module that can be added to a Sylphie bot to add core bot commands.
#[derive(Module)]
pub struct ModCore {
    #[module_info] info: ModuleInfo,
}

#[module_impl]
impl ModCore {
    #[command]
    async fn cmd_help(
        &self, ctx: &CommandCtx<impl Events>, help_cmd: Option<String>,
    ) -> Result<()> {
        let manager = ctx.handler().get_service::<CommandManager>();
        if let Some(command) = help_cmd {
            match manager.lookup_command_raw(&command)? {
                LookupResult::Found(cmd) => {
                    ctx.respond("Command full names:").await?;
                    for name in &*cmd.full_names {
                        ctx.respond(&format!("* {}", name)).await?;
                    }
                    ctx.respond("Command allowed names:").await?;
                    for name in &*cmd.allowed_names {
                        ctx.respond(&format!("* {}", name)).await?;
                    }
                }
                LookupResult::Ambigious(cmds) => {
                    ctx.respond("Command is ambiguous. Possible commands:").await?;
                    for command in cmds {
                        ctx.respond(&format!("* {}", command.shortest_name.full_name)).await?;
                    }
                }
                LookupResult::NoneFound => cmd_error!("No such command '{}' exists!"),
            }
        } else {
            ctx.respond("Available commands:").await?;
            for command in &*manager.command_list() {
                ctx.respond(&format!(
                    "* {}", command.shortest_name.full_name,
                )).await?;
            }
        }
        Ok(())
    }

    #[command]
    async fn cmd_shutdown(&self, target: &Handler<impl Events>) -> Result<()> {
        target.shutdown_bot();
        Ok(())
    }
}
