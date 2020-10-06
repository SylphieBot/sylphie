//! Contains types to help disambiguate names that may exist across multiple scopes.

use crate::strings::InternString;
use fxhash::{FxHashMap, FxHashSet};
use std::hash::Hash;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use sylphie_core::errors::*;

/// Returns the data underlying this entry name.
#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Clone, Hash)]
pub struct EntryNameData {
    pub prefix: Arc<str>,
    pub name: Arc<str>,
    pub full_name: Arc<str>,
    pub lc_name: Arc<str>,
    pub is_truncated: bool,
    _priv: (),
}

/// Stores the name for a given entry in an [`DisambiguatedSet`].
#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Clone, Hash)]
pub struct EntryName(Arc<EntryNameData>);
impl EntryName {
    /// Creates a new entry name.
    pub fn new(
        prefix: impl InternString<InternedType = Arc<str>>,
        name: impl InternString<InternedType = Arc<str>>
    ) -> Self {
        Self::new_0(prefix.intern(), name.intern())
    }
    fn new_0(prefix: Arc<str>, name: Arc<str>) -> Self {
        let full_name = if prefix.is_empty() {
            name.intern()
        } else {
            format!("{}:{}", prefix, name).intern()
        };
        let lc_name = full_name.to_ascii_lowercase().intern();
        EntryName(Arc::new(EntryNameData {
            prefix, name, full_name, lc_name, is_truncated: false, _priv: ()
        }))
    }

    /// Returns this name with a different prefix.
    pub fn with_prefix(&self, prefix: impl InternString<InternedType = Arc<str>>) -> Self {
        EntryName::new(prefix, self.name.clone())
    }

    /// Marks the is_truncated flag on this entry.
    pub fn mark_truncated(&self) -> Self {
        let mut entry = (*self.0).clone();
        entry.is_truncated = true;
        EntryName(Arc::new(entry))
    }

    fn variants(&self) -> Vec<EntryName> {
        let mut vec = Vec::new();
        vec.push(self.with_prefix("").mark_truncated());
        for (i, _) in self.prefix.char_indices().filter(|(_, c)| *c == '.') {
            vec.push(self.with_prefix(&self.prefix[..i]).mark_truncated());
        }
        vec.push(self.clone());
        vec
    }
}
impl fmt::Display for EntryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.full_name)
    }
}
impl Deref for EntryName {
    type Target = EntryNameData;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The data contained within an [`Disambiguated`] value.
#[derive(Debug)]
pub struct DisambiguatedData<T> {
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

    /// The list of all full names for this item.
    pub full_names: Arc<[EntryName]>,
}

#[derive(Debug)]
pub struct Disambiguated<T>(Arc<DisambiguatedData<T>>);
impl <T> Deref for Disambiguated<T> {
    type Target = DisambiguatedData<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl <T> Clone for Disambiguated<T> {
    fn clone(&self) -> Self {
        Disambiguated(self.0.clone())
    }
}

#[derive(Debug)]
pub struct DisambiguatedSet<T> {
    class_name: String,
    list: Arc<[Disambiguated<T>]>,
    // a map of {base command name -> {possible prefix -> [possible commands]}}
    // an unprefixed command looks up an empty prefix
    by_name: FxHashMap<Arc<str>, Box<[Disambiguated<T>]>>,
}
impl <T> DisambiguatedSet<T> {
    pub fn new(class_name: &str, values: Vec<(EntryName, T)>) -> Self {
        Self::new_aliased(
            class_name,
            values.into_iter().enumerate().map(|(i, (n, v))| (n, v, i)).collect()
        )
    }

    pub fn new_aliased<A: Eq + Hash + Copy>(
        class_name: &str,
        values: Vec<(EntryName, T, A)>,
    ) -> Self {
        // Sorts the raw values vector into a series of maps that are easier to process.
        //
        // This step checks for duplicate entries and handles aliased IDs.
        let mut duplicate_check = FxHashSet::default();
        let mut ids_for_name = FxHashMap::default();
        let mut values_for_id = FxHashMap::default();
        let mut names_for_id = FxHashMap::default();
        for (name, value, alias_id) in values {
            if duplicate_check.contains(&*name.lc_name) {
                warn!(
                    "Found duplicated {} `{}`. Only one of the copies will be accessible.",
                    class_name, name.full_name,
                );
            } else {
                if &*name.prefix == "__root__" {
                    warn!(
                        "It is not recommended to define a {} in the root module: `{}`",
                        class_name, name.full_name,
                    );
                }
                duplicate_check.insert(name.lc_name.clone());

                for variant_name in name.variants() {
                    ids_for_name
                        .entry(variant_name.lc_name.clone())
                        .or_insert_with(FxHashSet::default)
                        .insert(alias_id);
                    names_for_id.entry(alias_id).or_insert_with(Vec::new).push(variant_name);
                }
                values_for_id.insert(alias_id, value);
            }
        }
        std::mem::drop(duplicate_check);

        // Create the list of `Disambiguated` objects that store metadata about the entries, and
        // create the main lookup map.
        let mut disambiguated_list = Vec::new();
        let mut disambiguated_map = FxHashMap::default();
        for (id, value) in values_for_id {
            let mut names = names_for_id.remove(&id).unwrap();
            names.sort_by_cached_key(|x| x.full_name.clone());

            let mut shortest_name = names[0].clone();
            let mut allowed_names = Vec::new();
            let mut all_names = Vec::new();
            let mut full_names = Vec::new();

            for name in &names {
                if ids_for_name.get(&*name.lc_name).unwrap().len() == 1 {
                    if name.full_name.len() < shortest_name.full_name.len() {
                        shortest_name = name.clone();
                    }
                    allowed_names.push(name.clone());
                }
                all_names.push(name.clone());
                if !name.is_truncated {
                    full_names.push(name.clone());
                }
            }

            let disambiguated = Disambiguated(Arc::new(DisambiguatedData {
                value,
                shortest_name,
                allowed_names: allowed_names.into(),
                all_names: all_names.into(),
                full_names: full_names.into(),
            }));
            disambiguated_list.push(disambiguated.clone());
            for name in names {
                disambiguated_map
                    .entry(name.lc_name.clone())
                    .or_insert_with(Vec::new)
                    .push(disambiguated.clone());
            }
        }
        std::mem::drop(names_for_id);

        // Sort the disambiguated list so the ordering doesn't change between runs
        disambiguated_list.sort_by_cached_key(|x| x.shortest_name.full_name.clone());
        for (_, values) in &mut disambiguated_map {
            values.sort_by_cached_key(|x| x.shortest_name.full_name.clone());
        }

        // Create the actual full set
        DisambiguatedSet {
            class_name: class_name.to_string(),
            list: disambiguated_list.into(),
            by_name: disambiguated_map.into_iter().map(|(k, v)| (k, v.into())).collect(),
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
        let mut lc_name = raw_name.to_ascii_lowercase();
        if lc_name.chars().filter(|x| *x == ':').count() > 1 {
            cmd_error!("No more than one `:` can appear in a {} name.", self.class_name);
        }
        if lc_name.starts_with(':') {
            lc_name = lc_name[1..].to_string();
        }

        let list = self.by_name
            .get(&*lc_name)
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
