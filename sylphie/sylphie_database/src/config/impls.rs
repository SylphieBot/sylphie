use crate::config::ConfigType;
use std::fmt;
use sylphie_core::errors::*;

impl <T: ConfigType> ConfigType for Option<T> {
    fn ui_fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Some(v) => T::ui_fmt(v, formatter),
            None => formatter.write_str("(default)"),
        }
    }
    fn ui_parse(text: &str) -> Result<Self> {
        Ok(Some(T::ui_parse(text)?))
    }
}
impl ConfigType for String {
    fn ui_fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&*self)
    }
    fn ui_parse(text: &str) -> Result<Self> {
        Ok(text.to_string())
    }
}