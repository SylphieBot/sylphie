//! Various utility types that are helpful in constructing Sylphie modules.

use arc_swap::{ArcSwapOption, Guard as ArcSwapGuard};
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering, fence};

pub struct GlobalInstance<T: Sync + Send + 'static> {
    is_active: AtomicBool,
    contents: ArcSwapOption<T>,
}
impl <T: Sync + Send + 'static> GlobalInstance<T> {
    pub fn new() -> Self {
        GlobalInstance {
            is_active: AtomicBool::new(false),
            contents: ArcSwapOption::new(None),
        }
    }

    pub fn set_instance(&'static self, value: T) -> InstanceScopeGuard<T> {
        if !self.is_active.compare_and_swap(false, true, Ordering::SeqCst) {
            fence(Ordering::SeqCst);
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

    pub fn load(&'static self) -> InstanceGuard<T> {
        InstanceGuard {
            inner_guard: self.contents.load(),
        }
    }
}

pub struct InstanceGuard<T: Sync + Send + 'static> {
    inner_guard: ArcSwapGuard<'static, Option<Arc<T>>>,
}
impl <T: Sync + Send + 'static> InstanceGuard<T> {
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

pub struct InstanceScopeGuard<T: Sync + Send + 'static> {
    instance: &'static GlobalInstance<T>,
}
impl <T: Sync + Send + 'static> Drop for InstanceScopeGuard<T> {
    fn drop(&mut self) {
        self.instance.unset_instance();
        fence(Ordering::SeqCst);
        self.instance.is_active.store(false, Ordering::SeqCst);
    }
}