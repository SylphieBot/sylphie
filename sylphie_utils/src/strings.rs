use crate::cache::*;
use crate::scopes::*;
use lazy_static::*;
use serde::*;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

/// A wrapper enum for storing strings in static contexts efficiently.
#[derive(Clone)]
pub enum StringWrapper {
    Static(&'static str),
    Owned(Box<str>),
    Shared(Arc<str>),
    /// An interned string. This is an optimization to avoid excessive cache lookups.
    Interned(Arc<str>),
}
impl StringWrapper {
    /// Clones this string, potentially rewriting this wrapper into a shared one.
    pub fn clone_mut(&mut self) -> StringWrapper {
        match self {
            StringWrapper::Static(s) => StringWrapper::Static(s),
            StringWrapper::Owned(o) => {
                let o: Arc<str> = std::mem::take(o).into();
                *self = StringWrapper::Shared(o.clone());
                StringWrapper::Shared(o)
            }
            StringWrapper::Shared(s) => StringWrapper::Shared((*s).clone()),
            StringWrapper::Interned(s) => StringWrapper::Shared((*s).clone()),
        }
    }

    /// Returns the string contained in this type.
    pub fn as_str(&self) -> &str {
        match self {
            StringWrapper::Static(s) => s,
            StringWrapper::Owned(s) => &s,
            StringWrapper::Shared(s) => &s,
            StringWrapper::Interned(s) => &s,
        }
    }
}

impl From<&'static str> for StringWrapper {
    fn from(s: &'static str) -> Self {
        StringWrapper::Static(s)
    }
}
impl From<String> for StringWrapper {
    fn from(s: String) -> Self {
        StringWrapper::Owned(s.into())
    }
}
impl From<Arc<str>> for StringWrapper {
    fn from(s: Arc<str>) -> Self {
        StringWrapper::Shared(s)
    }
}
impl Deref for StringWrapper {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}
impl Default for StringWrapper {
    fn default() -> Self {
        StringWrapper::Static("")
    }
}

impl PartialEq for StringWrapper {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl Eq for StringWrapper { }
impl PartialOrd for StringWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.as_str().partial_cmp(other.as_str())
    }
}
impl Ord for StringWrapper {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for StringWrapper {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

impl fmt::Debug for StringWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}
impl fmt::Display for StringWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl Serialize for StringWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        self.as_str().serialize(serializer)
    }
}
impl <'de> Deserialize<'de> for StringWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(StringWrapper::Owned(String::deserialize(deserializer)?.into()))
    }
}

/// A helper trait that exists to help intern strings.
pub trait InternString {
    type InternedType;
    fn intern(&self) -> Self::InternedType;
}

lazy_static! {
    static ref INTERN_CACHE: LruCache<Arc<str>, Arc<str>> = LruCache::new(1024);
}
impl InternString for Arc<str> {
    type InternedType = Arc<str>;
    fn intern(&self) -> Self::InternedType {
        INTERN_CACHE.cached(self.clone(), || Ok(self.clone())).unwrap()
    }
}
impl <'a> InternString for &'a str {
    type InternedType = Arc<str>;
    fn intern(&self) -> Self::InternedType {
        let arc: Arc<str> = String::from(*self).into();
        arc.intern()
    }
}
impl InternString for String {
    type InternedType = Arc<str>;
    fn intern(&self) -> Self::InternedType {
        let arc: Arc<str> = String::from(self.as_str()).into();
        arc.intern()
    }
}
impl InternString for Box<str> {
    type InternedType = Arc<str>;
    fn intern(&self) -> Self::InternedType {
        let arc: Arc<str> = String::from(&**self).into();
        arc.intern()
    }
}
impl InternString for StringWrapper {
    type InternedType = StringWrapper;
    fn intern(&self) -> Self::InternedType {
        match self {
            StringWrapper::Static(s) => StringWrapper::Static(s),
            StringWrapper::Owned(s) => StringWrapper::Interned(s.intern()),
            StringWrapper::Shared(s) => StringWrapper::Interned(s.intern()),
            StringWrapper::Interned(s) => StringWrapper::Interned(s.clone()),
        }
    }
}

impl InternString for ScopeArgs {
    type InternedType = ScopeArgs;
    fn intern(&self) -> Self::InternedType {
        match self {
            ScopeArgs::String(s) => ScopeArgs::String(s.intern()),
            x => x.clone(),
        }
    }
}
impl InternString for Scope {
    type InternedType = Scope;
    fn intern(&self) -> Self::InternedType {
        Scope {
            scope_type: self.scope_type.intern(),
            args: self.args.intern(),
        }
    }
}