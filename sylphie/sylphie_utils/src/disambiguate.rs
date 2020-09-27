//! Contains types to help disambiguate names that may exist across multiple scopes.

use crate::strings::InternString;
use fxhash::{FxHashMap, FxHashSet};
use std::fmt;
use std::hash::Hash;
use std::ops::Deref;
use std::sync::Arc;
use sylphie_core::errors::*;

/// A trait for items that can be disambiguated between modules.
pub trait CanDisambiguate {
    /// The display name for the type of object this is.
    const CLASS_NAME: &'static str;

    /// Returns the name of the disambiguated item.
    fn name(&self) -> &str;

    /// Returns the full name of the disambiguated item.
    fn full_name(&self) -> &str;

    /// Returns the name of the module this disambiguated item is in.
    fn module_name(&self) -> &str;
}

/// A trait for items that can be disambiguated between modules, but still map to the same
/// underlying value.
pub trait CanDisambiguateAliased : CanDisambiguate {
    type AliasId: Eq + Hash + Copy;
    fn alias_id(&self) -> Self::AliasId;
}

/// Stores the name for a given entry in an [`DisambiguatedSet`].
#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Clone)]
pub struct EntryName {
    pub prefix: Arc<str>,
    pub name: Arc<str>,
}
impl EntryName {
    /// Displays this entry
    pub fn display(&self) -> impl fmt::Display + '_ {
        FormatEntryName(self)
    }

    fn full_len(&self) -> usize {
        if self.prefix.is_empty() {
            self.name.len()
        } else {
            self.prefix.len() + 1 + self.name.len()
        }
    }
}

struct FormatEntryName<'a>(&'a EntryName);
impl <'a> fmt::Display for FormatEntryName<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.prefix.is_empty() {
            f.write_str(&self.0.name)
        } else {
            write!(f, "{}:{}", self.0.prefix, self.0.name)
        }
    }
}

/// The data contained within an [`Disambiguated`] value.
#[derive(Debug)]
pub struct DisambiguatedData<T: CanDisambiguate> {
    /// The actual disambiguated value.
    pub value: T,

    /// The shortest unambiguous name for this item, not accounting for permissions and such.
    pub shortest_name: EntryName,

    /// The list of unambiguous names for this item, in order from longest to shortest.
    ///
    /// If multiple command names are allowed, the order is not guaranteed.
    pub allowed_names: Arc<[EntryName]>,

    /// The list of all names for this item, in order from longest to shortest.
    ///
    /// If multiple command names are allowed, the order is not guaranteed.
    pub all_names: Arc<[EntryName]>,
}

#[derive(Debug)]
pub struct Disambiguated<T: CanDisambiguate>(Arc<DisambiguatedData<T>>);
impl <T: CanDisambiguate> Deref for Disambiguated<T> {
    type Target = DisambiguatedData<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl <T: CanDisambiguate> Clone for Disambiguated<T> {
    fn clone(&self) -> Self {
        Disambiguated(self.0.clone())
    }
}

#[derive(Debug)]
pub struct DisambiguatedSet<T: CanDisambiguate> {
    list: Arc<[Disambiguated<T>]>,
    // a map of {base command name -> {possible prefix -> [possible commands]}}
    // an unprefixed command looks up an empty prefix
    by_name: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Box<[Disambiguated<T>]>>>,
}
impl <T: CanDisambiguate> DisambiguatedSet<T> {
    pub fn new(values: Vec<T>) -> Self {
        let mut duplicate_check = FxHashSet::default();
        let mut values_for_name = FxHashMap::default();
        let mut root_warning_given = false;
        for value in values {
            let lc_name = value.full_name().to_ascii_lowercase();
            if duplicate_check.contains(&lc_name) {
                warn!(
                    "Found duplicated {} `{}`. Only one of the copies will be accessible.",
                    T::CLASS_NAME, value.full_name(),
                );
            } else {
                if !root_warning_given && value.module_name() == "__root__" {
                    warn!(
                        "It is not recommended to define a {} in the root module.",
                        T::CLASS_NAME,
                    );
                    root_warning_given = true;
                }

                duplicate_check.insert(lc_name);
                values_for_name.entry(value.name().to_ascii_lowercase())
                    .or_insert(Vec::new()).push(value);
            }
        }
        std::mem::drop(duplicate_check);

        let mut disambiguated_list = Vec::new();
        let by_name = values_for_name.into_iter().map(|(name, variants)| {
            let name = name.intern();

            let mut prefix_count = FxHashMap::default();
            let mut variants_temp = Vec::new();
            for variant in variants {
                let mod_name = variant.module_name().to_ascii_lowercase();
                let full_name = variant.full_name().to_ascii_lowercase().intern();

                let mut prefixes = Vec::new();
                prefixes.push(full_name);
                for (i, _) in mod_name.char_indices().filter(|(_, c)| *c == '.') {
                    prefixes.push(mod_name[i+1..].to_string().intern());
                }
                prefixes.push("".intern());

                for prefix in &prefixes {
                    *prefix_count.entry(prefix.clone()).or_insert(0) += 1;
                }

                variants_temp.push((prefixes, variant));
            }

            let mut map = FxHashMap::default();
            for (prefixes, variant) in variants_temp {
                let mut shortest_prefix = prefixes[0].clone();
                for prefix in &prefixes {
                    if *prefix_count.get(prefix).unwrap() == 1 {
                        shortest_prefix = prefix.clone();
                    }
                }

                let mut allowed_names = Vec::new();
                let mut all_names = Vec::new();
                for prefix in &prefixes {
                    let entry = EntryName {
                        prefix: prefix.clone(),
                        name: name.clone(),
                    };
                    all_names.push(entry.clone());
                    if *prefix_count.get(prefix).unwrap() == 1 {
                        allowed_names.push(entry);
                    }
                }

                let entry = Disambiguated(Arc::new(DisambiguatedData {
                    value: variant,
                    shortest_name: EntryName {
                        prefix: shortest_prefix,
                        name: name.clone(),
                    },
                    allowed_names: allowed_names.into(),
                    all_names: all_names.into(),
                }));
                for prefix in prefixes {
                    map.entry(prefix).or_insert(Vec::new()).push(entry.clone());
                }
                disambiguated_list.push(entry);
            }
            (name.intern(), map.into_iter().map(|(k, v)| (k, v.into())).collect())
        }).collect();

        // sort the disambiguated list so the ordering doesn't change between runs
        disambiguated_list.sort_by_cached_key(|x| x.value.full_name().to_string());

        DisambiguatedSet { list: disambiguated_list.into(), by_name }
    }

    pub fn list(&self) -> &[Disambiguated<T>] {
        &self.list
    }

    pub fn list_arc(&self) -> Arc<[Disambiguated<T>]> {
        self.list.clone()
    }

    pub fn resolve_iter<'a>(
        &'a self, raw_name: &str,
    ) -> Result<impl Iterator<Item = Disambiguated<T>> + 'a> {
        let lc_name = raw_name.to_ascii_lowercase();
        let split: Vec<_> = lc_name.split(':').collect();
        let (group, name) = match split.as_slice() {
            &[name] => ("", name),
            &[group, name] => (group, name),
            _ => cmd_error!("No more than one `:` can appear in a {} name.", T::CLASS_NAME),
        };

        let list = self.by_name
            .get(name)
            .and_then(|x| x.get(group))
            .map(|x| &**x)
            .unwrap_or(&[]);
        Ok(list.iter().map(|x| x.clone()))
    }

    pub fn resolve(&self, raw_name: &str) -> Result<LookupResult<Disambiguated<T>>> {
        let mut vec = Vec::new();
        for item in self.resolve_iter(raw_name)? {
            vec.push(item.clone());
        }
        Ok(LookupResult::new(vec))
    }

    pub fn resolve_cloned(&self, raw_name: &str) -> Result<LookupResult<T>> where T: Clone {
        Ok(self.resolve(raw_name)?.map(|x| x.value.clone()))
    }
}

/// The result of a lookup.
#[derive(Debug)]
pub enum LookupResult<T> {
    /// No matching entries were found.
    NoneFound,
    /// A single unambiguous entry was found.
    Found(T),
    /// An ambiguous set of entries were found.
    Ambigious(Vec<T>),
}
impl <T> LookupResult<T> {
    pub fn new(mut list: Vec<T>) -> Self {
        if list.len() == 0 {
            LookupResult::NoneFound
        } else if list.len() == 1 {
            LookupResult::Found(list.pop().unwrap())
        } else {
            LookupResult::Ambigious(list)
        }
    }

    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> LookupResult<U> {
        match self {
            LookupResult::NoneFound => LookupResult::NoneFound,
            LookupResult::Found(v) => LookupResult::Found(f(v)),
            LookupResult::Ambigious(v) => LookupResult::Ambigious(v.into_iter().map(f).collect()),
        }
    }
}

#[derive(Debug)]
struct ConfigNameKey<T: CanDisambiguateAliased> {
    id: T::AliasId,
    name: Arc<str>,
    module_name: Arc<str>,
    full_name: Arc<str>,
}
impl <T: CanDisambiguateAliased> CanDisambiguate for ConfigNameKey<T> {
    const CLASS_NAME: &'static str = T::CLASS_NAME;
    fn name(&self) -> &str {
        &self.name
    }
    fn full_name(&self) -> &str {
        &self.full_name
    }
    fn module_name(&self) -> &str {
        &self.module_name
    }
}

#[derive(Debug)]
pub struct AliasedDisambiguatedSet<T: CanDisambiguateAliased> {
    underlying: DisambiguatedSet<ConfigNameKey<T>>,
    lookup: Arc<FxHashMap<T::AliasId, Disambiguated<T>>>,
    list: Arc<[Disambiguated<T>]>,
}
impl <T: CanDisambiguateAliased> AliasedDisambiguatedSet<T> {
    pub fn new(values: Vec<T>) -> Self {
        let mut id_vec = Vec::new();
        let mut value_map = FxHashMap::default();
        for value in values {
            id_vec.push(ConfigNameKey::<T> {
                id: value.alias_id(),
                name: value.name().intern(),
                module_name: value.module_name().intern(),
                full_name: value.full_name().intern(),
            });
            value_map.insert(value.alias_id(), value);
        }
        let underlying = DisambiguatedSet::new(id_vec);

        let mut alias_values = FxHashMap::default();
        for id in underlying.list() {
            alias_values.entry(id.value.id)
                .or_insert_with(Vec::new)
                .push(id.clone())
        }

        let mut lookup = FxHashMap::default();
        let mut list = Vec::new();
        for (id, mut aliased) in alias_values {
            aliased.sort_by_cached_key(|x| x.shortest_name.clone());

            let mut shortest_name = aliased[0].shortest_name.clone();
            let mut all_names = Vec::new();
            let mut allowed_names = Vec::new();

            for alias in aliased {
                for name in &*alias.all_names {
                    all_names.push(name.clone());
                }
                for name in &*alias.allowed_names {
                    allowed_names.push(name.clone());
                }
                if alias.shortest_name.full_len() < shortest_name.full_len() {
                    shortest_name = alias.shortest_name.clone()
                }
            }

            let value = Disambiguated(Arc::new(DisambiguatedData {
                value: value_map.remove(&id).unwrap(),
                shortest_name,
                allowed_names: allowed_names.into(),
                all_names: all_names.into(),
            }));
            list.push(value.clone());
            lookup.insert(id, value);
        }

        // sort the disambiguated list so the ordering doesn't change between runs
        list.sort_by_cached_key(|x| x.all_names[0].display().to_string());

        AliasedDisambiguatedSet {
            underlying,
            lookup: Arc::new(lookup),
            list: list.into(),
        }
    }

    pub fn list(&self) -> &[Disambiguated<T>] {
        &self.list
    }

    pub fn list_arc(&self) -> Arc<[Disambiguated<T>]> {
        self.list.clone()
    }

    pub fn resolve_iter<'a>(
        &'a self, raw_name: &str,
    ) -> Result<impl Iterator<Item = Disambiguated<T>> + 'a> {
        let mut already_found = FxHashSet::default();
        let lookup = self.lookup.clone();
        Ok(self.underlying.resolve_iter(raw_name)?
            .filter(move |x| {
                if already_found.contains(&x.value.id) {
                    false
                } else {
                    already_found.insert(x.value.id);
                    true
                }
            })
            .map(move |x| {
                lookup.get(&x.value.id).unwrap().clone()
            }))
    }

    pub fn resolve(&self, raw_name: &str) -> Result<LookupResult<Disambiguated<T>>> {
        let mut vec = Vec::new();
        for item in self.resolve_iter(raw_name)? {
            vec.push(item.clone());
        }
        Ok(LookupResult::new(vec))
    }

    pub fn resolve_cloned(&self, raw_name: &str) -> Result<LookupResult<T>> where T: Clone {
        Ok(self.resolve(raw_name)?.map(|x| x.value.clone()))
    }
}