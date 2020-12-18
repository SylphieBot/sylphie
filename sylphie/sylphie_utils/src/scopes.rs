//! Types used to specify particular contexts such as users, members or servers.

use crate::strings::StringWrapper;
use serde::*;

/// The data contained in a scope.
#[derive(Serialize, Deserialize, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub enum ScopeArgs {
    /// A scope that does not contain any arguments.
    None,

    /// An arbitrary string.
    String(StringWrapper),

    /// A scope containing a single 64-bit integer.
    Long(u64),
    /// A scope containing two 64-bit integers.
    Long2(u64, u64),
    /// A scope containing three 64-bit integers.
    Long3(u64, u64, u64),

    /// A scope containing a single 32-bit integer.
    Int(u32),
    /// A scope containing two 32-bit integers.
    Int2(u32, u32),
    /// A scope containing three 32-bit integers.
    Int3(u32, u32, u32),
}

/// A tagged scope used as an identifier.
#[derive(Serialize, Deserialize, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub struct Scope {
    pub scope_type: StringWrapper,
    pub args: ScopeArgs,
}
impl Scope {
    /// Creates a new scope.
    pub fn new(scope_type: impl Into<StringWrapper>, args: ScopeArgs) -> Self {
        Scope {
            scope_type: scope_type.into(),
            args,
        }
    }
}