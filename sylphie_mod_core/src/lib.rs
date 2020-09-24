use sylphie::commands::manager::CommandManager;
use sylphie::prelude::*;

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
        ctx.respond("Available commands:").await?;
        for command in &*ctx.handler().get_service::<CommandManager>().command_list() {
            ctx.respond(&format!(
                "* {}", command.disambiguated_name(),
            )).await?;
        }
        Ok(())
    }

    #[command]
    async fn cmd_shutdown(&self, target: &Handler<impl Events>) -> Result<()> {
        target.shutdown_bot();
        Ok(())
    }
}
