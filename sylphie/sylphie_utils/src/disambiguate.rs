use crate::strings::InternString;
use fxhash::{FxHashMap, FxHashSet};
use std::fmt;
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

#[derive(Debug)]
pub struct Disambiguated<T: CanDisambiguate> {
    /// The actual disambiguated value.
    pub value: T,

    /// The shortest unambiguous prefix for this item, not accounting for permissions and such.
    pub disambiguated_prefix: Arc<str>,

    /// The list of prefixes allowed for this item, in order from longest to shortest.
    pub allowed_prefixes: Arc<[Arc<str>]>,
}
impl <T: CanDisambiguate> Disambiguated<T> {
    /// Returns the shortest unambigious name for this command.
    pub fn disambiguated_name(&self) -> impl fmt::Display + '_ {
        FormatDisambiguated(self)
    }
}

struct FormatDisambiguated<'a, T: CanDisambiguate>(&'a Disambiguated<T>);
impl <'a, T: CanDisambiguate> fmt::Display for FormatDisambiguated<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.disambiguated_prefix.is_empty() {
            f.write_str(self.0.value.name())
        } else {
            write!(f, "{}:{}", self.0.disambiguated_prefix, self.0.value.name())
        }
    }
}

#[derive(Debug)]
pub struct DisambiguatedSet<T: CanDisambiguate> {
    list: Arc<[Arc<Disambiguated<T>>]>,
    // a map of {base command name -> {possible prefix -> [possible commands]}}
    // an unprefixed command looks up an empty prefix
    by_name: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Box<[Arc<Disambiguated<T>>]>>>,
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
                let mut longest_prefix = prefixes[0].clone();
                for prefix in &prefixes {
                    if *prefix_count.get(prefix).unwrap() == 1 {
                        longest_prefix = prefix.clone();
                    }
                }

                let entry = Arc::new(Disambiguated {
                    value: variant,
                    disambiguated_prefix: longest_prefix,
                    allowed_prefixes: prefixes.clone().into(),
                });
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

    pub fn list(&self) -> Arc<[Arc<Disambiguated<T>>]> {
        self.list.clone()
    }

    pub fn resolve_iter<'a>(
        &'a self, raw_name: &str,
    ) -> Result<impl Iterator<Item = &Arc<Disambiguated<T>>> + 'a> {
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
        Ok(list.iter())
    }
}

/// The result of a lookup.
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
}