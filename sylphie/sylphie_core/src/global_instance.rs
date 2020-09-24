use arc_swap::{ArcSwapOption, Guard as ArcSwapGuard};
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
