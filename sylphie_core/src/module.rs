use crate::core::CoreRef;
use enumset::*;
use static_events::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub use sylphie_derive::*;

/// Information relating to the git repo a module is contained in, if any.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct GitInfo {
    pub name: &'static str,
    pub revision: &'static str,
    pub modified_files: u32,
}

/// Metadata relating to this module.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ModuleMetadata {
    pub module_path: &'static str,
    pub crate_version: &'static str,
    pub git_info: Option<GitInfo>,
    pub flags: EnumSet<ModuleFlag>,
}

/// Metadata relating to an crate containing modules.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct CrateMetadata {
    pub crate_path: &'static str,
    pub crate_version: &'static str,
    pub git_info: Option<GitInfo>,
}

impl From<ModuleMetadata> for CrateMetadata {
    fn from(meta: ModuleMetadata) -> Self {
        CrateMetadata {
            crate_path: meta.module_path.split(':').next().unwrap(),
            crate_version: meta.crate_version,
            git_info: meta.git_info,
        }
    }
}

/// Used to uniquely identify a loaded module.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ModuleId(u32, u32);

#[derive(EnumSetType, Debug)]
pub enum ModuleFlag {
    /// Integral modules cannot be disabled.
    Integral,
    /// A module that is integral, and whose children are all integral.
    IntegralRecursive,
    /// A module that is only used internally, and should not be shown in any UI.
    Anonymous,
}

#[derive(Clone, Debug)]
struct ModuleInfoInternal {
    id: ModuleId,
    name: String,
    metadata: ModuleMetadata,
}

#[derive(Default, Clone, Debug)]
pub struct ModuleInfo(Option<ModuleInfoInternal>);

impl ModuleInfo {
    pub fn id(&self) -> ModuleId {
        self.0.as_ref().expect("Module not yet initialized!").id
    }
    pub fn name(&self) -> &str {
        &self.0.as_ref().expect("Module not yet initialized!").name
    }
    pub fn metadata(&self) -> ModuleMetadata {
        self.0.as_ref().expect("Module not yet initialized!").metadata
    }
    fn set(&mut self, data: ModuleInfoInternal) {
        if self.0.is_some() {
            panic!("Module is already initialized!");
        }
        self.0 = Some(data);
    }
}

pub struct ModuleTreeWalker<'a> {
    manager: &'a mut ModuleManager,
}
impl <'a> ModuleTreeWalker<'a> {
    fn init_module(
        &mut self, name: &str, metadata: ModuleMetadata, info: &mut ModuleInfo,
    ) {
        let name = if name.is_empty() { "__root__".to_string() } else { name.to_string() };

        assert!(self.manager.module_info.len() <= u32::max_value() as usize);
        let id = ModuleId(self.manager.module_id_root, self.manager.module_info.len() as u32);
        assert!(!self.manager.name_to_id.contains_key(&name));
        info.set(ModuleInfoInternal {
            id, name: name.clone(), metadata,
        });
        self.manager.module_info.push(info.clone());
        self.manager.name_to_id.insert(name, id);
    }
    pub fn register_module<R: Module, M: Module>(
        &mut self, core: CoreRef<R>, parent: &str, name: &str,
    ) -> M {
        assert_ne!(name, "__root__", "__root__ is a reserved module name.");
        assert!(!name.contains('.'), "Periods are not allowed in module names.");
        let submodule_name =
            if parent.is_empty() { name.to_string() } else { format!("{}.{}", parent, name) };
        let mut module = M::init_module(core, &submodule_name, self);
        let metadata = module.metadata();
        self.init_module(&submodule_name, metadata, module.info_mut());
        module
    }
}

pub trait Module: Events + Sized + Send + Sync + 'static {
    fn metadata(&self) -> ModuleMetadata;

    fn info(&self) -> &ModuleInfo;
    fn info_mut(&mut self) -> &mut ModuleInfo;

    fn init_module<R: Module>(
        core: CoreRef<R>, parent: &str, walker: &mut ModuleTreeWalker,
    ) -> Self;
}

#[derive(Debug)]
pub struct ModuleManager {
    module_id_root: u32,
    module_info: Vec<ModuleInfo>,
    name_to_id: HashMap<String, ModuleId>,
    source_crates: Arc<[CrateMetadata]>,
}
impl ModuleManager {
    fn compute_source_crates(&mut self) {
        let mut set = HashSet::new();
        for module in &self.module_info {
            set.insert(CrateMetadata::from(module.metadata()));
        }
        let mut list: Vec<_> = set.into_iter().collect();
        list.sort();
        self.source_crates = list.into();
    }
    pub(crate) fn init<R: Module>(core: CoreRef<R>) -> (ModuleManager, R) {
        static MODULE_ID_ROOT: AtomicU32 = AtomicU32::new(0);
        let mut manager = ModuleManager {
            module_id_root: MODULE_ID_ROOT.fetch_add(1, Ordering::Relaxed),
            module_info: Default::default(),
            name_to_id: Default::default(),
            source_crates: Vec::new().into(),
        };
        let mut walker = ModuleTreeWalker {
            manager: &mut manager,
        };
        let mut root = R::init_module(core, "", &mut walker);
        let metadata = root.metadata();
        walker.init_module("", metadata, root.info_mut());
        manager.compute_source_crates();
        (manager, root)
    }
    pub(crate) fn modules_list(&self) -> Arc<[CrateMetadata]> {
        self.source_crates.clone()
    }

    /// Returns the metadata for a given module.
    ///
    /// This method will panic if called on a `ModuleId` from a different `ModuleManager`.
    pub fn get_module(&self, id: ModuleId) -> &ModuleInfo {
        assert_eq!(id.0, self.module_id_root, "ModuleId is from a different ModuleManager.");
        &self.module_info[id.1 as usize]
    }

    /// Returns the metadata for a given module by name, if one exists.
    pub fn find_module(&self, name: &str) -> Option<&ModuleInfo> {
        self.name_to_id.get(name).map(|x| self.get_module(*x))
    }
}
