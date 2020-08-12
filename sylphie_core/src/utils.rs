//! Various utility types that are helpful in constructing Sylphie modules.

use arc_swap::{ArcSwapOption, Guard as ArcSwapGuard};
use serde::*;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering, fence};

/// A helper class for a global variable that is set during the lifetime of the bot.
///
/// This is mainly for internal use, but is provided here in case it is useful.
pub struct GlobalInstance<T: Sync + Send + 'static> {
    is_active: AtomicBool,
    contents: ArcSwapOption<T>,
}
impl <T: Sync + Send + 'static> GlobalInstance<T> {
    /// Creates a global instance container.
    pub fn new() -> Self {
        GlobalInstance {
            is_active: AtomicBool::new(false),
            contents: ArcSwapOption::new(None),
        }
    }

    /// Sets the current instance and returns a guard that unsets it when dropped.
    pub fn set_instance(&'static self, value: T) -> InstanceScopeGuard<T> {
        if !self.is_active.compare_and_swap(false, true, AtomicOrdering::SeqCst) {
            fence(AtomicOrdering::SeqCst);
            self.contents.store(Some(Arc::new(value)));
            InstanceScopeGuard {
                instance: self,
            }
        } else {
            panic!("Another instance of Sylphie is already running.");
        }
    }
    fn unset_instance(&'static self) {
        self.contents.store(None);
    }

    /// Returns a guard that contains the current instance.
    ///
    /// Note that if none exists, it will panic when you deref the instance, not when this method
    /// is called.
    pub fn load(&'static self) -> InstanceGuard<T> {
        InstanceGuard {
            inner_guard: self.contents.load(),
        }
    }
}

/// A guard for [`GlobalInstance::set_instance`].
pub struct InstanceGuard<T: Sync + Send + 'static> {
    inner_guard: ArcSwapGuard<'static, Option<Arc<T>>>,
}
impl <T: Sync + Send + 'static> InstanceGuard<T> {
    /// Returns whether an instance is actually loaded.
    pub fn is_loaded(&self) -> bool {
        self.inner_guard.is_some()
    }
}
impl <T: Sync + Send + 'static> Deref for InstanceGuard<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &**self.inner_guard.as_ref().expect("No instance of Sylphie is running.")
    }
}

/// A guard for [`GlobalInstance::set_instance`].
pub struct InstanceScopeGuard<T: Sync + Send + 'static> {
    instance: &'static GlobalInstance<T>,
}
impl <T: Sync + Send + 'static> Drop for InstanceScopeGuard<T> {
    fn drop(&mut self) {
        self.instance.unset_instance();
        fence(AtomicOrdering::SeqCst);
        self.instance.is_active.store(false, AtomicOrdering::SeqCst);
    }
}

/// A wrapper enum for storing strings in static contexts efficiently.
#[derive(Clone)]
pub enum StringWrapper {
    Static(&'static str),
    Owned(Box<str>),
    Shared(Arc<str>),
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
        }
    }

    /// Returns the string contained in this type.
    pub fn as_str(&self) -> &str {
        match self {
            StringWrapper::Static(s) => s,
            StringWrapper::Owned(s) => &s,
            StringWrapper::Shared(s) => &s
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