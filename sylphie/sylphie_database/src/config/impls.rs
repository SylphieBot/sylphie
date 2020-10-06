use crate::config::ConfigType;
use std::fmt;
use sylphie_core::errors::*;

impl ConfigType for String {
    fn ui_fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&*self)
    }
    fn ui_parse(text: &str) -> Result<Self> {
        Ok(text.to_string())
    }
}