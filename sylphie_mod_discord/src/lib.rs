use minnie::prelude::*;
use sylphie::core::InitEvent;
use sylphie::database::config::*;
use sylphie::prelude::*;

/// A module that can be added to a Sylphie bot to add Discord support.
#[derive(Module)]
pub struct ModDiscord {
    #[module_info] info: ModuleInfo,
}

#[module_impl]
impl ModDiscord {
    #[config]
    pub const CFG_DISCORD_TOKEN: ConfigKey<String> = config_option!(
        Any, "discord token f1733370-515d-43f8-87b4-8b2833cfdd9d", || "!".to_string(),
    );

    #[event_handler]
    fn on_init(&self, _: &InitEvent) -> Result<()> {
        Ok(())
    }
}
