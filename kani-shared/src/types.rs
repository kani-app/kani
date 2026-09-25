//! Serializable domain models exchanged by the web, application, host-runtime, and extension
//! layers. WIT-owned boundary types remain authoritative in [`crate::wit_types`].

use crate::bindings::kani::extension::types as wit_types;

#[cfg(feature = "host")]
use serde::de::{self, SeqAccess, Visitor};
#[cfg(feature = "host")]
use serde::{Deserialize, Deserializer, Serialize};
#[cfg(feature = "host")]
use std::fmt;

/// Manga publication status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "host", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "host", serde(rename_all = "lowercase"))]
pub enum MangaStatus {
    Ongoing,
    Completed,
    Hiatus,
    Cancelled,
    Unknown,
}

impl std::fmt::Display for MangaStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MangaStatus::Ongoing => write!(f, "Ongoing"),
            MangaStatus::Completed => write!(f, "Completed"),
            MangaStatus::Hiatus => write!(f, "Hiatus"),
            MangaStatus::Cancelled => write!(f, "Cancelled"),
            MangaStatus::Unknown => write!(f, "Unknown"),
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for MangaStatus {
    fn default() -> Self {
        MangaStatus::Unknown
    }
}

impl From<i64> for MangaStatus {
    fn from(value: i64) -> Self {
        match value {
            0 => MangaStatus::Ongoing,
            1 => MangaStatus::Completed,
            2 => MangaStatus::Hiatus,
            3 => MangaStatus::Cancelled,
            _ => MangaStatus::Unknown,
        }
    }
}

impl From<MangaStatus> for i64 {
    fn from(status: MangaStatus) -> Self {
        match status {
            MangaStatus::Ongoing => 0,
            MangaStatus::Completed => 1,
            MangaStatus::Hiatus => 2,
            MangaStatus::Cancelled => 3,
            MangaStatus::Unknown => 4,
        }
    }
}

/// Filter state used by extensions to read active filter values.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "host", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "host", serde(tag = "kind", content = "data"))]
pub enum FilterState {
    Selection { name: String, value: String },
    Checkbox(bool),
    TextInput(String),
    Multiselect(Vec<String>),
}

impl From<FilterState> for wit_types::FilterState {
    fn from(s: FilterState) -> Self {
        match s {
            FilterState::Selection { name, value } => {
                wit_types::FilterState::Selection(wit_types::OptionState { name, value })
            }
            FilterState::Checkbox(b) => wit_types::FilterState::Checkbox(b),
            FilterState::TextInput(s) => wit_types::FilterState::TextInput(s),
            FilterState::Multiselect(values) => wit_types::FilterState::Multiselect(values),
        }
    }
}

impl From<wit_types::FilterState> for FilterState {
    fn from(s: wit_types::FilterState) -> Self {
        match s {
            wit_types::FilterState::Selection(opt) => FilterState::Selection {
                name: opt.name,
                value: opt.value,
            },
            wit_types::FilterState::Checkbox(b) => FilterState::Checkbox(b),
            wit_types::FilterState::TextInput(s) => FilterState::TextInput(s),
            wit_types::FilterState::Multiselect(values) => FilterState::Multiselect(values),
        }
    }
}

/// Active filter value passed to extension search/popular methods.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "host", derive(Serialize, Deserialize))]
pub struct ActiveFilter {
    pub filter_name: String,
    pub state: FilterState,
}

/// Convert a WIT-generated `ActiveFilter` list (guest side) to the shared `ActiveFilter` type.
pub fn to_shared_filters(filters: Vec<wit_types::ActiveFilter>) -> Vec<ActiveFilter> {
    filters
        .into_iter()
        .map(|f| ActiveFilter {
            filter_name: f.filter_name,
            state: f.state.into(),
        })
        .collect()
}

pub const DEFAULT_BROWSER_AUTO_SCROLL: bool = false;

/// Whether `id` has the `[a-z][a-z0-9-]*` form every extension id must take. Ids name
/// artifact files, source rows and cache namespaces, so they may not contain separators.
pub fn is_valid_extension_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

const AUTO_SCROLL_ON: &str = "/*kani:auto-scroll=true*/\n";
const AUTO_SCROLL_OFF: &str = "/*kani:auto-scroll=false*/\n";

/// Prefixes a capture script with its auto-scroll choice, which is how the setting
/// crosses the WIT `capture-page-payload` call.
pub fn mark_auto_scroll(init_script: &str, auto_scroll: bool) -> String {
    let marker = if auto_scroll {
        AUTO_SCROLL_ON
    } else {
        AUTO_SCROLL_OFF
    };
    format!("{marker}{init_script}")
}

/// Splits a capture script into its auto-scroll choice and the script itself; an
/// unmarked script takes [`DEFAULT_BROWSER_AUTO_SCROLL`].
pub fn take_auto_scroll(init_script: &str) -> (bool, &str) {
    if let Some(script) = init_script.strip_prefix(AUTO_SCROLL_ON) {
        (true, script)
    } else if let Some(script) = init_script.strip_prefix(AUTO_SCROLL_OFF) {
        (false, script)
    } else {
        (DEFAULT_BROWSER_AUTO_SCROLL, init_script)
    }
}

/// Builds a [`FilterList`](crate::wit_types::FilterList) from semicolon-separated
/// filter declarations. A declaration may provide separate identifier and label
/// expressions or use one expression for both; selection options accept either
/// `(label, value)` pairs or values used as both label and value.
#[macro_export]
macro_rules! filter_list {
    (@munch [ $($output:tt)* ]) => {
        $crate::wit_types::FilterList { filters: vec![ $($output)* ] }
    };

    (@opts $id:expr, [ $($accum:tt)* ] ) => { vec![ $($accum)* ] };

    (@opts $id:expr, [ $($accum:tt)* ] ( $n:expr, $v:expr ) $(, $($rest:tt)*)? ) => {
        filter_list!(@opts $id, [
            $($accum)*
            $crate::wit_types::FilterOption {
                filter_name: $id.to_string(),
                name: $n.to_string(),
                value: $v.to_string(),
            },
        ] $($($rest)*)? )
    };

    (@opts $id:expr, [ $($accum:tt)* ] $n:expr $(, $($rest:tt)*)? ) => {
        filter_list!(@opts $id, [
            $($accum)*
            $crate::wit_types::FilterOption {
                filter_name: $id.to_string(),
                name: $n.to_string(),
                value: $n.to_string(),
            },
        ] $($($rest)*)? )
    };

    (@build [ $($output:tt)* ] $id:expr, $display:expr, $kind:ident, $opts:expr, $def:expr, $sem:expr $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [
            $($output)*
            $crate::wit_types::FilterDef {
                id: $id.to_string(),
                name: $display.to_string(),
                tag: $crate::wit_types::FilterTypeTag::$kind,
                options: $opts,
                default_value: $def,
                semantic: $sem,
            },
        ] $($($rest)*)?)
    };

    (@munch [ $($output:tt)* ] $id:expr, $display:expr, $kind:ident, [ $($opts:tt)* ], default: ( $def_n:expr, $def_v:expr ) $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@build [ $($output)* ] $id, $display, $kind,
            filter_list!(@opts $id, [] $($opts)*),
            Some($crate::wit_types::FilterState::Selection($crate::wit_types::OptionState {
                name: $def_n.to_string(),
                value: $def_v.to_string(),
            })),
            { #[allow(unused_mut)] let mut _s: Option<$crate::wit_types::FilterSemantic> = None; $( _s = Some($sem); )? _s }
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $id:expr, $display:expr, $kind:ident, [ $($opts:tt)* ], default: $def:expr $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $id, $display, $kind, [ $($opts)* ], default: ($def, $def) $(, semantic: $sem)? $(; $($rest)*)? )
    };
    (@munch [ $($output:tt)* ] $id:expr, $display:expr, $kind:ident, [ $($opts:tt)* ] $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@build [ $($output)* ] $id, $display, $kind,
            filter_list!(@opts $id, [] $($opts)*),
            None,
            { #[allow(unused_mut)] let mut _s: Option<$crate::wit_types::FilterSemantic> = None; $( _s = Some($sem); )? _s }
            $(; $($rest)*)?
        )
    };

    (@munch [ $($output:tt)* ] $display:expr, $kind:ident, [ $($opts:tt)* ], default: ( $def_n:expr, $def_v:expr ) $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $display, $display, $kind, [ $($opts)* ], default: ($def_n, $def_v) $(, semantic: $sem)? $(; $($rest)*)? )
    };
    (@munch [ $($output:tt)* ] $display:expr, $kind:ident, [ $($opts:tt)* ], default: $def:expr $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $display, $display, $kind, [ $($opts)* ], default: ($def, $def) $(, semantic: $sem)? $(; $($rest)*)? )
    };
    (@munch [ $($output:tt)* ] $display:expr, $kind:ident, [ $($opts:tt)* ] $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $display, $display, $kind, [ $($opts)* ] $(, semantic: $sem)? $(; $($rest)*)? )
    };

    (@munch [ $($output:tt)* ] $id:expr, $display:expr, Checkbox, default: $def:expr $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@build [ $($output)* ] $id, $display, Checkbox,
            vec![],
            Some($crate::wit_types::FilterState::Checkbox($def)),
            { #[allow(unused_mut)] let mut _s: Option<$crate::wit_types::FilterSemantic> = None; $( _s = Some($sem); )? _s }
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $id:expr, $display:expr, Checkbox $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@build [ $($output)* ] $id, $display, Checkbox,
            vec![],
            None,
            { #[allow(unused_mut)] let mut _s: Option<$crate::wit_types::FilterSemantic> = None; $( _s = Some($sem); )? _s }
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $display:expr, Checkbox, default: $def:expr $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $display, $display, Checkbox, default: $def $(, semantic: $sem)? $(; $($rest)*)? )
    };
    (@munch [ $($output:tt)* ] $display:expr, Checkbox $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $display, $display, Checkbox $(, semantic: $sem)? $(; $($rest)*)? )
    };

    (@munch [ $($output:tt)* ] $id:expr, $display:expr, TextInput, default: $def:expr $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@build [ $($output)* ] $id, $display, TextInput,
            vec![],
            Some($crate::wit_types::FilterState::TextInput($def.to_string())),
            { #[allow(unused_mut)] let mut _s: Option<$crate::wit_types::FilterSemantic> = None; $( _s = Some($sem); )? _s }
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $id:expr, $display:expr, TextInput $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@build [ $($output)* ] $id, $display, TextInput,
            vec![],
            None,
            { #[allow(unused_mut)] let mut _s: Option<$crate::wit_types::FilterSemantic> = None; $( _s = Some($sem); )? _s }
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $display:expr, TextInput, default: $def:expr $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $display, $display, TextInput, default: $def $(, semantic: $sem)? $(; $($rest)*)? )
    };
    (@munch [ $($output:tt)* ] $display:expr, TextInput $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $display, $display, TextInput $(, semantic: $sem)? $(; $($rest)*)? )
    };

    (@munch [ $($output:tt)* ] $id:expr, $display:expr, Multiselect, [ $($opts:tt)* ] $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@build [ $($output)* ] $id, $display, Multiselect,
            filter_list!(@opts $id, [] $($opts)*),
            None,
            { #[allow(unused_mut)] let mut _s: Option<$crate::wit_types::FilterSemantic> = None; $( _s = Some($sem); )? _s }
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $display:expr, Multiselect, [ $($opts:tt)* ] $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $display, $display, Multiselect, [ $($opts)* ] $(, semantic: $sem)? $(; $($rest)*)? )
    };

    (@munch [ $($output:tt)* ] $id:expr, $display:expr, Multiselect, splice: $opts:expr $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@build [ $($output)* ] $id, $display, Multiselect,
            $opts.iter().map(|(n, v)| $crate::wit_types::FilterOption {
                filter_name: ($id).to_string(),
                name: n.to_string(),
                value: v.to_string(),
            }).collect(),
            None,
            { #[allow(unused_mut)] let mut _s: Option<$crate::wit_types::FilterSemantic> = None; $( _s = Some($sem); )? _s }
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $display:expr, Multiselect, splice: $opts:expr $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $display, $display, Multiselect, splice: $opts $(, semantic: $sem)? $(; $($rest)*)? )
    };
    (@munch [ $($output:tt)* ] $id:expr, $display:expr, Select, splice: $opts:expr $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@build [ $($output)* ] $id, $display, Select,
            $opts.iter().map(|(n, v)| $crate::wit_types::FilterOption {
                filter_name: ($id).to_string(),
                name: n.to_string(),
                value: v.to_string(),
            }).collect(),
            None,
            { #[allow(unused_mut)] let mut _s: Option<$crate::wit_types::FilterSemantic> = None; $( _s = Some($sem); )? _s }
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $display:expr, Select, splice: $opts:expr $(, semantic: $sem:expr)? $(; $($rest:tt)*)? ) => {
        filter_list!(@munch [ $($output)* ] $display, $display, Select, splice: $opts $(, semantic: $sem)? $(; $($rest)*)? )
    };

    (@munch $($invalid:tt)*) => {
        compile_error!(concat!("Invalid filter syntax. Stuck on: ", stringify!($($invalid)*)))
    };
    () => { $crate::wit_types::FilterList { filters: vec![] } };
    ($first:expr, $($rest:tt)*) => { filter_list!(@munch [] $first, $($rest)*) };
}

#[macro_export]
/// Builds chapter sort options. Each `id, label` declaration expands to
/// descending and ascending options; `raw: expression` inserts one option.
macro_rules! chapter_sort_list {
    (@munch [$($output:tt)*]) => {
        vec![$($output)*]
    };

    (@munch [$($output:tt)*] $id:literal, $name:literal $(; $($rest:tt)*)?) => {
        chapter_sort_list!(@munch [
            $($output)*
            $crate::wit_types::SortOption {
                id: concat!($id, "_desc").to_string(),
                name: concat!($name, " (descending)").to_string(),
            },
            $crate::wit_types::SortOption {
                id: concat!($id, "_asc").to_string(),
                name: concat!($name, " (ascending)").to_string(),
            },
        ] $($($rest)*)?)
    };

    (@munch [$($output:tt)*] raw: $expr:expr $(; $($rest:tt)*)?) => {
        chapter_sort_list!(@munch [
            $($output)*
            $expr,
        ] $($($rest)*)?)
    };

    (@munch $($invalid:tt)*) => {
        compile_error!(concat!("Invalid chapter_sort_list syntax. Stuck on: ", stringify!($($invalid)*)))
    };
    () => { vec![] };
    ($($rest:tt)+) => { chapter_sort_list!(@munch [] $($rest)+) };
}

#[macro_export]
/// Builds preference specifications from semicolon-separated declarations.
/// The label may also serve as the key; text preferences may be secret, and
/// multi-value preferences always use `[]` as their serialized default while
/// storing an optional placeholder in the options list.
macro_rules! preference_list {
    (@munch [ $($output:tt)* ]) => { vec![ $($output)* ] };

    (@opts [ $($accum:tt)* ]) => { vec![ $($accum)* ] };
    (@opts [ $($accum:tt)* ] ( $l:expr, $v:expr ) $(, $($rest:tt)*)? ) => {
        preference_list!(@opts [
            $($accum)*
            ($l.to_string(), $v.to_string()),
        ] $($($rest)*)? )
    };

    (@build [ $($output:tt)* ] $key:expr, $label:expr, $kind:ident, $opts:expr, $def:expr, $desc:expr, $secret:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@munch [
            $($output)*
            $crate::wit_types::PreferenceSpec {
                key: $key.to_string(),
                label: $label.to_string(),
                kind: $crate::wit_types::PrefKind::$kind,
                options: $opts,
                default: $def.to_string(),
                description: $desc,
                secret: $secret,
            },
        ] $($($rest)*)?)
    };

    (@munch [ $($output:tt)* ] $key:expr, $label:expr, Toggle, default: $def:expr, description: $desc:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, Toggle,
            vec![], if $def { "true" } else { "false" }, Some($desc.to_string()), false
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $key:expr, $label:expr, Toggle, default: $def:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, Toggle,
            vec![], if $def { "true" } else { "false" }, None, false
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $label:expr, Toggle, default: $def:expr $(, description: $desc:expr)? $(; $($rest:tt)*)? ) => {
        preference_list!(@munch [ $($output)* ] $label, $label, Toggle, default: $def $(, description: $desc)? $(; $($rest)*)? )
    };

    (@munch [ $($output:tt)* ] $key:expr, $label:expr, Select, [ $($opts:tt)* ], default: $def:expr, description: $desc:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, Select,
            preference_list!(@opts [] $($opts)*), $def, Some($desc.to_string()), false
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $key:expr, $label:expr, Select, [ $($opts:tt)* ], default: $def:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, Select,
            preference_list!(@opts [] $($opts)*), $def, None, false
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $label:expr, Select, [ $($opts:tt)* ], default: $def:expr $(, description: $desc:expr)? $(; $($rest:tt)*)? ) => {
        preference_list!(@munch [ $($output)* ] $label, $label, Select, [ $($opts)* ], default: $def $(, description: $desc)? $(; $($rest)*)? )
    };

    (@munch [ $($output:tt)* ] $key:expr, $label:expr, Text, default: $def:expr, description: $desc:expr, secret: true $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, Text, vec![], $def, Some($desc.to_string()), true $(; $($rest)*)?)
    };
    (@munch [ $($output:tt)* ] $key:expr, $label:expr, Text, default: $def:expr, description: $desc:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, Text, vec![], $def, Some($desc.to_string()), false $(; $($rest)*)?)
    };
    (@munch [ $($output:tt)* ] $key:expr, $label:expr, Text, default: $def:expr, secret: true $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, Text, vec![], $def, None, true $(; $($rest)*)?)
    };
    (@munch [ $($output:tt)* ] $key:expr, $label:expr, Text, default: $def:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, Text, vec![], $def, None, false $(; $($rest)*)?)
    };
    (@munch [ $($output:tt)* ] $label:expr, Text, default: $def:expr $(, $($mods:tt)*)? $(; $($rest:tt)*)? ) => {
        preference_list!(@munch [ $($output)* ] $label, $label, Text, default: $def $(, $($mods)*)? $(; $($rest)*)? )
    };

    (@munch [ $($output:tt)* ] $key:expr, $label:expr, MultiValueList, placeholder: $ph:expr, description: $desc:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, MultiValueList,
            vec![("placeholder".to_string(), $ph.to_string())], "[]", Some($desc.to_string()), false
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $key:expr, $label:expr, MultiValueList, placeholder: $ph:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, MultiValueList,
            vec![("placeholder".to_string(), $ph.to_string())], "[]", None, false
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $key:expr, $label:expr, MultiValueList, description: $desc:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, MultiValueList,
            vec![], "[]", Some($desc.to_string()), false
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $key:expr, $label:expr, MultiValueList $(; $($rest:tt)*)? ) => {
        preference_list!(@build [ $($output)* ] $key, $label, MultiValueList,
            vec![], "[]", None, false
            $(; $($rest)*)?
        )
    };
    (@munch [ $($output:tt)* ] $label:expr, MultiValueList, placeholder: $ph:expr, description: $desc:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@munch [ $($output)* ] $label, $label, MultiValueList, placeholder: $ph, description: $desc $(; $($rest)*)? )
    };
    (@munch [ $($output:tt)* ] $label:expr, MultiValueList, placeholder: $ph:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@munch [ $($output)* ] $label, $label, MultiValueList, placeholder: $ph $(; $($rest)*)? )
    };
    (@munch [ $($output:tt)* ] $label:expr, MultiValueList, description: $desc:expr $(; $($rest:tt)*)? ) => {
        preference_list!(@munch [ $($output)* ] $label, $label, MultiValueList, description: $desc $(; $($rest)*)? )
    };
    (@munch [ $($output:tt)* ] $label:expr, MultiValueList $(; $($rest:tt)*)? ) => {
        preference_list!(@munch [ $($output)* ] $label, $label, MultiValueList $(; $($rest)*)? )
    };

    (@munch $($invalid:tt)*) => {
        compile_error!(concat!("Invalid preference syntax. Stuck on: ", stringify!($($invalid)*)))
    };
    () => { vec![] };
    ($first:expr, $($rest:tt)*) => { preference_list!(@munch [] $first, $($rest)*) };
}

/// Deserializes `Vec<NamedItem>` from either an array of objects or plain strings.
#[cfg(feature = "host")]
fn deserialize_named_item_vec<'de, D>(deserializer: D) -> Result<Vec<NamedItem>, D::Error>
where
    D: Deserializer<'de>,
{
    struct NamedItemVecVisitor;

    impl<'de> Visitor<'de> for NamedItemVecVisitor {
        type Value = Vec<NamedItem>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an array of NamedItem objects or strings")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut items = Vec::new();
            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                let item = match value {
                    serde_json::Value::String(s) => NamedItem { id: 0, name: s },
                    obj => serde_json::from_value(obj).map_err(de::Error::custom)?,
                };
                items.push(item);
            }
            Ok(items)
        }
    }

    deserializer.deserialize_seq(NamedItemVecVisitor)
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ssr", derive(sqlx::FromRow))]
pub struct NamedItem {
    pub id: i64,
    pub name: String,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MangaListItem {
    pub id: String,
    pub title: String,
    pub cover_url: Option<String>,
    #[serde(default)]
    pub new_chapter_count: i64,
    #[serde(default)]
    pub resume: Option<ContinueReadingChapter>,
    /// Which indexed field matched a library search, when it was not the title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_field: Option<String>,
    /// The matching fragment of that field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_text: Option<String>,
    /// `pending` or `unlinked` while an imported id has no proven replacement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_link_status: Option<String>,
    /// The source no longer lists this manga.
    #[serde(default)]
    pub is_orphaned: bool,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MangaList {
    pub manga: Vec<MangaListItem>,
    pub has_next_page: bool,
    #[serde(default)]
    pub total_pages: Option<u32>,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MangaInfo {
    pub id: String,
    pub title: String,
    pub cover_url: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub description_html: Option<String>,
    #[serde(deserialize_with = "deserialize_named_item_vec")]
    pub authors: Vec<NamedItem>,
    #[serde(deserialize_with = "deserialize_named_item_vec")]
    pub artists: Vec<NamedItem>,
    pub status: MangaStatus,
    #[serde(deserialize_with = "deserialize_named_item_vec")]
    pub tags: Vec<NamedItem>,
}

/// Durable download lifecycle state of a chapter, stored as the integer
/// `download_status` column. Serialises as that integer (0/1/2) to preserve the
/// existing JSON API and frontend contract.
#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(into = "i64", try_from = "i64")]
#[repr(i64)]
pub enum DownloadStatus {
    #[default]
    Pending = 0,
    InProgress = 1,
    Complete = 2,
}

#[cfg(feature = "host")]
impl From<DownloadStatus> for i64 {
    fn from(s: DownloadStatus) -> i64 {
        s as i64
    }
}

#[cfg(feature = "host")]
impl TryFrom<i64> for DownloadStatus {
    type Error = String;
    fn try_from(v: i64) -> Result<Self, String> {
        match v {
            0 => Ok(Self::Pending),
            1 => Ok(Self::InProgress),
            2 => Ok(Self::Complete),
            other => Err(format!("invalid download_status: {other}")),
        }
    }
}

#[cfg(feature = "ssr")]
impl sqlx::Type<sqlx::Sqlite> for DownloadStatus {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <i64 as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
    fn compatible(ty: &sqlx::sqlite::SqliteTypeInfo) -> bool {
        <i64 as sqlx::Type<sqlx::Sqlite>>::compatible(ty)
    }
}

#[cfg(feature = "ssr")]
impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for DownloadStatus {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<sqlx::sqlite::SqliteArgumentValue<'q>>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <i64 as sqlx::Encode<sqlx::Sqlite>>::encode_by_ref(&(*self as i64), buf)
    }
}

#[cfg(feature = "ssr")]
impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for DownloadStatus {
    fn decode(value: sqlx::sqlite::SqliteValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let v = <i64 as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
        DownloadStatus::try_from(v).map_err(Into::into)
    }
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Chapter {
    pub id: String,
    pub title: Option<String>,
    pub number: f64,
    pub volume: Option<i64>,
    pub language: String,
    pub scanlator: Option<String>,
    pub date_uploaded: Option<i64>,
    #[serde(default)]
    pub download_status: DownloadStatus,
    #[serde(default)]
    pub is_orphaned: bool,
    pub page_count: Option<i64>,
    #[serde(default)]
    pub is_read: bool,
    #[serde(default)]
    pub last_page_read: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_error: Option<serde_json::Value>,
    /// Upgrade candidates found for this chapter, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade_available: Option<serde_json::Value>,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ChapterList {
    pub chapters: Vec<Chapter>,
    pub has_next_page: bool,
    #[serde(default)]
    pub total_pages: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
}

/// A sort option declared by an extension (e.g. for ordering a chapter list).
#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SortOption {
    pub id: String,
    pub name: String,
}

#[cfg(feature = "host")]
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChapterSortOrder {
    #[default]
    ChapterDesc,
    ChapterAsc,
    UploadedDesc,
    UploadedAsc,
    VolumeDesc,
    VolumeAsc,
    LanguageAsc,
    LanguageDesc,
    ScanlatorAsc,
    ScanlatorDesc,
}

#[cfg(feature = "host")]
impl ChapterSortOrder {
    pub fn to_sql_order(&self) -> &'static str {
        match self {
            Self::ChapterDesc => "c.chapter_number DESC, c.id DESC",
            Self::ChapterAsc => "c.chapter_number ASC, c.id ASC",
            Self::UploadedDesc => "c.uploaded_at DESC, c.chapter_number DESC",
            Self::UploadedAsc => "c.uploaded_at ASC, c.chapter_number ASC",
            Self::VolumeDesc => "c.volume DESC NULLS LAST, c.chapter_number DESC",
            Self::VolumeAsc => "c.volume ASC NULLS FIRST, c.chapter_number ASC",
            Self::LanguageAsc => "c.language ASC NULLS LAST, c.chapter_number DESC",
            Self::LanguageDesc => "c.language DESC NULLS LAST, c.chapter_number DESC",
            Self::ScanlatorAsc => "c.scanlator ASC NULLS LAST, c.chapter_number DESC",
            Self::ScanlatorDesc => "c.scanlator DESC NULLS LAST, c.chapter_number DESC",
        }
    }

    pub fn to_select_value(&self) -> &'static str {
        match self {
            Self::ChapterDesc => "chapter_desc",
            Self::ChapterAsc => "chapter_asc",
            Self::UploadedDesc => "uploaded_desc",
            Self::UploadedAsc => "uploaded_asc",
            Self::VolumeDesc => "volume_desc",
            Self::VolumeAsc => "volume_asc",
            Self::LanguageAsc => "language_asc",
            Self::LanguageDesc => "language_desc",
            Self::ScanlatorAsc => "scanlator_asc",
            Self::ScanlatorDesc => "scanlator_desc",
        }
    }

    pub fn from_select_value(s: &str) -> Self {
        match s {
            "chapter_asc" => Self::ChapterAsc,
            "uploaded_desc" => Self::UploadedDesc,
            "uploaded_asc" => Self::UploadedAsc,
            "volume_desc" => Self::VolumeDesc,
            "volume_asc" => Self::VolumeAsc,
            "language_asc" => Self::LanguageAsc,
            "language_desc" => Self::LanguageDesc,
            "scanlator_asc" => Self::ScanlatorAsc,
            "scanlator_desc" => Self::ScanlatorDesc,
            _ => Self::ChapterDesc,
        }
    }
}

#[cfg(feature = "host")]
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MangaSortOrder {
    #[default]
    NameDesc,
    NameAsc,
    UpdatedDesc,
    UpdatedAsc,
    AddedDesc,
    AddedAsc,
    ScoreDesc,
    ScoreAsc,
    LastReadDesc,
}

#[cfg(feature = "host")]
impl MangaSortOrder {
    pub fn to_sql_order(&self) -> &'static str {
        match self {
            Self::NameDesc => "m.name DESC, m.id DESC",
            Self::NameAsc => "m.name ASC, m.id ASC",
            Self::UpdatedDesc => "m.updated_at DESC, m.name ASC",
            Self::UpdatedAsc => "m.updated_at ASC, m.name ASC",
            Self::AddedDesc => "m.created_at DESC, m.name ASC",
            Self::AddedAsc => "m.created_at ASC, m.name ASC",
            Self::ScoreDesc => "umt.score DESC NULLS LAST, m.name ASC",
            Self::ScoreAsc => "umt.score ASC NULLS LAST, m.name ASC",
            Self::LastReadDesc => "max_last_read DESC NULLS LAST, m.name ASC",
        }
    }

    pub fn to_select_value(&self) -> &'static str {
        match self {
            Self::NameDesc => "name_desc",
            Self::NameAsc => "name_asc",
            Self::UpdatedDesc => "updated_desc",
            Self::UpdatedAsc => "updated_asc",
            Self::AddedDesc => "added_desc",
            Self::AddedAsc => "added_asc",
            Self::ScoreDesc => "score_desc",
            Self::ScoreAsc => "score_asc",
            Self::LastReadDesc => "last_read_desc",
        }
    }

    pub fn from_select_value(s: &str) -> Self {
        match s {
            "name_desc" => Self::NameDesc,
            "name_asc" => Self::NameAsc,
            "updated_desc" => Self::UpdatedDesc,
            "updated_asc" => Self::UpdatedAsc,
            "added_desc" => Self::AddedDesc,
            "added_asc" => Self::AddedAsc,
            "score_desc" => Self::ScoreDesc,
            "score_asc" => Self::ScoreAsc,
            "last_read_desc" => Self::LastReadDesc,
            _ => Self::NameAsc,
        }
    }

    /// Returns true if the sort requires the `max_last_read` subquery column in the SELECT.
    pub fn needs_last_read_column(&self) -> bool {
        matches!(self, Self::LastReadDesc)
    }

    /// Returns true if the sort requires joining `user_manga_tracking`.
    pub fn needs_tracking_join(&self) -> bool {
        matches!(self, Self::ScoreDesc | Self::ScoreAsc | Self::LastReadDesc)
    }
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DownloadProgressEvent {
    ChapterStarted {
        chapter_id: i64,
        chapter_name: String,
        manga_id: i64,
        manga_title: String,
        total_pages: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job_id: Option<String>,
    },
    PageCompleted {
        chapter_id: i64,
        chapter_name: String,
        page_index: i32,
    },
    ChapterCompleted {
        chapter_id: i64,
        chapter_name: String,
        manga_id: i64,
        manga_title: String,
        successful_pages: usize,
    },
    ChapterFailed {
        chapter_id: i64,
        chapter_name: String,
        error: String,
    },
    ChapterCancelled {
        chapter_id: i64,
        chapter_name: String,
    },
}

#[cfg(feature = "host")]
#[derive(Clone, Serialize, Deserialize)]
pub struct LibraryPage {
    pub items: Vec<MangaListItem>,
    pub has_next_page: bool,
    pub total_pages: Option<u32>,
}

#[cfg(feature = "host")]
fn default_true() -> bool {
    true
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ssr", derive(sqlx::FromRow))]
pub struct Source {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub base_url: String,
    pub enabled: bool,
    pub favourited: bool,
    pub unrestricted_http: bool,
    #[serde(default = "default_true")]
    #[cfg_attr(feature = "ssr", sqlx(default))]
    pub browser_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ssr", sqlx(default))]
    pub download_concurrency: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ssr", sqlx(default))]
    pub circuit_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ssr", sqlx(default))]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ssr", sqlx(default))]
    pub description: Option<String>,
    /// JSON-encoded `Vec<String>` of language codes; kept as the raw column text
    /// rather than decoded server-side since nothing on the host needs the typed
    /// form yet — the frontend parses it directly for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ssr", sqlx(default))]
    pub languages: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ssr", sqlx(default))]
    pub schema_version: i64,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GlobalSearchResult {
    pub source_id: i64,
    pub source_name: String,
    pub has_next_page: bool,
    pub manga: Vec<MangaListItem>,
    /// Why this source contributed nothing, when it failed rather than matched
    /// nothing. Without it the two are the same empty list, and a source that
    /// never answered is reported as one that found no matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SearchScope {
    FavouritedOnly,
    AllEnabled,
    Sources(Vec<i64>),
}

#[cfg(feature = "host")]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "ssr", derive(sqlx::FromRow))]
pub struct ChapterFilterRow {
    pub id: i64,
    pub scanlator: Option<String>,
    pub language: String,
    pub name: Option<String>,
    pub chapter_number: f64,
    /// Seconds since Unix epoch; `None` if the source didn't provide an upload date.
    pub uploaded_at: Option<i64>,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum DownloadRuleKind {
    LanguageInclude(String),
    LanguageExclude(String),
    TitleContains(String),
    TitleExcludes(String),
    ChapterNumberMin(f64),
    ChapterNumberMax(f64),
    ExcludeFractional,
    MaxAgeDays(i32),
    PublishedAfter(i64),
}

#[cfg(feature = "host")]
impl DownloadRuleKind {
    pub fn axis(&self) -> u8 {
        match self {
            Self::LanguageInclude(_) | Self::LanguageExclude(_) => 0,
            Self::TitleContains(_) | Self::TitleExcludes(_) => 1,
            Self::ChapterNumberMin(_) | Self::ChapterNumberMax(_) => 2,
            Self::ExcludeFractional => 3,
            Self::MaxAgeDays(_) | Self::PublishedAfter(_) => 4,
        }
    }

    pub fn is_include(&self) -> bool {
        matches!(self, Self::LanguageInclude(_) | Self::TitleContains(_))
    }

    pub fn passes(&self, chapter: &ChapterFilterRow) -> bool {
        match self {
            Self::LanguageInclude(v) => chapter.language.eq_ignore_ascii_case(v),
            Self::LanguageExclude(v) => !chapter.language.eq_ignore_ascii_case(v),
            Self::TitleContains(v) => chapter
                .name
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&v.to_lowercase()),
            Self::TitleExcludes(v) => !chapter
                .name
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&v.to_lowercase()),
            Self::ChapterNumberMin(min) => chapter.chapter_number >= *min,
            Self::ChapterNumberMax(max) => chapter.chapter_number <= *max,
            Self::ExcludeFractional => {
                (chapter.chapter_number - chapter.chapter_number.floor()).abs() < f64::EPSILON
            }
            Self::MaxAgeDays(days) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let cutoff = now - (*days as i64) * 86_400;
                chapter.uploaded_at.is_none_or(|t| t >= cutoff)
            }
            Self::PublishedAfter(epoch) => chapter.uploaded_at.is_none_or(|t| t >= *epoch),
        }
    }
}

#[cfg(feature = "host")]
impl std::fmt::Display for DownloadRuleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadRuleKind::LanguageInclude(v) => write!(f, "Language includes {v}"),
            DownloadRuleKind::LanguageExclude(v) => write!(f, "Language excludes {v}"),
            DownloadRuleKind::TitleContains(v) => write!(f, "Title contains {v}"),
            DownloadRuleKind::TitleExcludes(v) => write!(f, "Title excludes {v}"),
            DownloadRuleKind::ChapterNumberMin(n) => write!(f, "Chapter number ≥ {n}"),
            DownloadRuleKind::ChapterNumberMax(n) => write!(f, "Chapter number ≤ {n}"),
            DownloadRuleKind::ExcludeFractional => write!(f, "Exclude fractional chapters"),
            DownloadRuleKind::MaxAgeDays(n) => write!(f, "Published within last {n} days"),
            DownloadRuleKind::PublishedAfter(ts) => write!(f, "Published after epoch {ts}"),
        }
    }
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DownloadRule {
    pub id: i64,
    pub manga_id: i64,
    pub kind: DownloadRuleKind,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ssr", derive(sqlx::FromRow))]
pub struct ScanlatorPreference {
    pub id: i64,
    /// `None` for a library-wide default. A per-manga row always wins over it.
    pub manga_id: Option<i64>,
    pub scanlator: String,
    pub priority: i64,
    /// In `priority` mode: if `true` this scanlator is completely blocked.
    pub blocked: bool,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ssr", derive(sqlx::FromRow))]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserStats {
    pub calls_total: u64,
    pub restarts: u64,
    pub graceful_shutdowns: u64,
    pub forced_terminations: u64,
    pub solver_attempts: u64,
    pub solver_successes: u64,
    pub solver_failures: u64,
    pub max_memory_mb: u32,
    pub idle_timeout_s: u32,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AppSettings {
    pub flaresolverr_url: String,
    pub library_path: String,
    pub wasm_storage_path: String,
    pub concurrent_page_downloads: i64,
    pub max_retries: i64,
    pub initial_retry_delay_ms: i64,
    pub max_wasm_instances: i64,
    pub auto_scan: bool,
    pub scan_interval_minutes: i64,
    pub scan_exclude_completed: bool,
    pub auto_download_category_ids: Vec<i64>,
    pub default_tracking_enabled: bool,
    pub http_request_logging: bool,
    #[serde(alias = "browser_debug_logging")]
    pub v8_debug_logging: bool,
    pub registration_enabled: bool,
    pub cover_max_dimension: Option<i64>,
    pub email_enabled: bool,
    pub email_provider: String,
    pub email_provider_config: String,
    pub email_from_address: String,
    pub app_url: String,
    pub password_reset_enabled: bool,
    pub email_verification_required: bool,
    pub first_run_complete: bool,
    pub scan_concurrency: i64,
    pub per_source_download_concurrency: i64,
    pub trash_retention_days: i64,
    pub audit_retention_days: i64,
    pub audit_security_retention_days: i64,
    pub disk_warn_threshold: f64,
    pub thumbnail_formats: String,
    pub max_login_attempts: i64,
    pub max_ip_attempts: i64,
    pub login_lockout_seconds: i64,
    pub session_timeout_secs: i64,
    pub tracker_auto_sync_enabled: bool,
    pub tracker_sync_interval_hours: i64,
    pub max_concurrent_jobs: i64,
    pub db_maintenance_interval_hours: i64,
    pub db_vacuum_interval_hours: i64,
    pub audit_prune_interval_hours: i64,
    pub trash_purge_interval_hours: i64,
    pub integrity_quick_scrub_interval_hours: i64,
    pub integrity_deep_scrub_interval_hours: i64,
    pub scrub_on_startup: bool,
    #[serde(default = "default_revalidate_days")]
    pub integrity_revalidate_after_days: i64,
    pub upgrade_detection_enabled: bool,
    pub upgrade_min_res_gain: f64,
    pub upgrade_confirm_fetches: i64,
    pub upgrade_axis_resolution: String,
    pub upgrade_axis_colour: String,
    pub upgrade_axis_encoder: String,
    pub upgrade_axis_bitrate: String,
    pub upgrade_show_downgrades: bool,
    pub upgrade_auto_replace_reasons: String,
    #[serde(alias = "browser_max_memory_mb")]
    pub v8_max_memory_mb: i64,
    #[serde(alias = "browser_idle_timeout_s")]
    pub v8_idle_timeout_s: i64,
    pub update_check_enabled: bool,
    /// OPDS-PSE `?page=` numbering: `false` is one-based and `true` is zero-based.
    pub opds_page_index_zero_based: bool,
    #[serde(default = "default_barren_page_tolerance")]
    pub scan_barren_page_tolerance: i64,
    #[serde(default = "default_global_search_timeout_secs")]
    pub global_search_timeout_secs: i64,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RecentUpdate {
    pub recent_updates: Vec<RecentUpdateItem>,
    pub has_next_page: bool,
    pub total_pages: Option<u32>,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RecentUpdateItem {
    pub manga_id: i64,
    pub manga_name: String,
    pub cover_url: Option<String>,
    #[serde(skip)]
    pub local_cover_path: Option<String>,
    #[serde(skip)]
    pub cover_hash: Option<String>,
    pub base_url: String,
    pub chapter_id: i64,
    pub chapter_number: f64,
    pub chapter_name: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub discovered_at: std::option::Option<time::OffsetDateTime>,
    pub is_downloaded: bool,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ChapterContents {
    pub pages: Vec<Page>,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Page {
    pub index: i64,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MigrationResult {
    pub chapters_matched: usize,
    pub chapters_orphaned: usize,
    pub chapters_new: usize,
    pub chapters_kept: usize,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MigrationPreview {
    pub target_title: String,
    pub target_cover_url: Option<String>,
    pub chapters_matched: usize,
    pub chapters_orphaned: usize,
    pub chapters_new: usize,
    pub downloaded_chapters_at_risk: usize,
}

#[cfg(feature = "host")]
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct AuthenticatedUser {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub roles: Vec<String>,
    pub email_verified_at: Option<String>,
}

#[cfg(feature = "host")]
impl AuthenticatedUser {
    pub fn has_role(&self, slug: &str) -> bool {
        self.roles.iter().any(|r| r == slug)
    }
    pub fn is_admin(&self) -> bool {
        self.has_role("admin")
    }
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DownloadSettings {
    pub concurrent_page_downloads: i64,
    pub max_retries: i64,
    pub initial_retry_delay_ms: i64,
    pub auto_download_category_ids: Vec<i64>,
    pub scan_concurrency: i64,
    pub per_source_download_concurrency: i64,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ScanSettings {
    pub auto_scan: bool,
    pub scan_interval_minutes: i64,
    pub scan_exclude_completed: bool,
    pub upgrade_detection_enabled: bool,
    pub upgrade_min_res_gain: f64,
    pub upgrade_confirm_fetches: i64,
    // Defaulted so a client that predates the per-axis policy can still PATCH
    // the scan settings it does know about, rather than being rejected for
    // omitting fields it has never heard of.
    #[serde(default = "default_axis_both")]
    pub upgrade_axis_resolution: String,
    #[serde(default = "default_axis_both")]
    pub upgrade_axis_colour: String,
    #[serde(default = "default_axis_both")]
    pub upgrade_axis_encoder: String,
    #[serde(default = "default_axis_gain")]
    pub upgrade_axis_bitrate: String,
    #[serde(default)]
    pub upgrade_show_downgrades: bool,
    #[serde(default = "default_auto_replace_reasons")]
    pub upgrade_auto_replace_reasons: String,
    #[serde(default = "default_barren_page_tolerance")]
    pub scan_barren_page_tolerance: i64,
}

#[cfg(feature = "host")]
fn default_revalidate_days() -> i64 {
    30
}

#[cfg(feature = "host")]
fn default_barren_page_tolerance() -> i64 {
    3
}

#[cfg(feature = "host")]
fn default_global_search_timeout_secs() -> i64 {
    6
}

#[cfg(feature = "host")]
fn default_axis_both() -> String {
    "both".into()
}

#[cfg(feature = "host")]
fn default_axis_gain() -> String {
    "gain".into()
}

#[cfg(feature = "host")]
fn default_auto_replace_reasons() -> String {
    "preferred_scanlator,resolution,colour".into()
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AdvancedSettings {
    pub flaresolverr_url: String,
    pub library_path: String,
    pub wasm_storage_path: String,
    pub max_wasm_instances: i64,
    pub http_request_logging: bool,
    #[serde(alias = "browser_debug_logging")]
    pub v8_debug_logging: bool,
    pub registration_enabled: bool,
    pub cover_max_dimension: Option<i64>,
    #[serde(alias = "browser_max_memory_mb")]
    pub v8_max_memory_mb: i64,
    #[serde(alias = "browser_idle_timeout_s")]
    pub v8_idle_timeout_s: i64,
    pub update_check_enabled: bool,
    /// OPDS-PSE `?page=` numbering: `false` is one-based and `true` is zero-based.
    pub opds_page_index_zero_based: bool,
    #[serde(default = "default_global_search_timeout_secs")]
    pub global_search_timeout_secs: i64,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TrackingSettings {
    pub default_tracking_enabled: bool,
    pub tracker_auto_sync_enabled: bool,
    pub tracker_sync_interval_hours: i64,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct EmailSettings {
    pub email_enabled: bool,
    pub email_provider: String,
    /// JSON blob with provider-specific credentials. Password/key fields use "••••••" as a
    /// placeholder meaning "do not update the stored value".
    pub email_provider_config: String,
    pub email_from_address: String,
    pub app_url: String,
    pub password_reset_enabled: bool,
    pub email_verification_required: bool,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MaintenanceSettings {
    pub trash_retention_days: i64,
    pub audit_retention_days: i64,
    pub audit_security_retention_days: i64,
    pub disk_warn_threshold: f64,
    pub thumbnail_formats: String,
    pub integrity_quick_scrub_interval_hours: i64,
    pub integrity_deep_scrub_interval_hours: i64,
    pub scrub_on_startup: bool,
    #[serde(default = "default_revalidate_days")]
    pub integrity_revalidate_after_days: i64,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SecuritySettings {
    pub max_login_attempts: i64,
    pub max_ip_attempts: i64,
    pub login_lockout_seconds: i64,
    pub session_timeout_secs: i64,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PerformanceSettings {
    pub max_concurrent_jobs: i64,
    pub db_maintenance_interval_hours: i64,
    pub db_vacuum_interval_hours: i64,
    pub audit_prune_interval_hours: i64,
    pub trash_purge_interval_hours: i64,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SettingsUpdate {
    Download(DownloadSettings),
    Scan(ScanSettings),
    Advanced(AdvancedSettings),
    Tracking(TrackingSettings),
    Email(EmailSettings),
    Maintenance(MaintenanceSettings),
    Security(SecuritySettings),
    Performance(PerformanceSettings),
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CredentialEncryptionStatus {
    pub encryption_enabled: bool,
    /// Number of credential values currently stored in plaintext.
    pub plaintext_count: i64,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[repr(i64)]
pub enum MangaTrackingStatus {
    Reading = 0,
    OnHold = 1,
    Dropped = 2,
    PlanToRead = 3,
    Completed = 4,
    Rereading = 5,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MangaTracking {
    pub status: Option<MangaTrackingStatus>,
    pub score: Option<f64>,
    pub chapters_read: i64,
    pub total_chapters: i64,
    pub tracking_enabled: bool,
    pub notify_new_chapters: bool,
    pub reading_direction: String,
    pub reader_prefs: Option<String>,
}

#[cfg(feature = "host")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ContinueReadingChapter {
    pub chapter_id: i64,
    pub chapter_number: f64,
    pub last_page: i64,
    pub page_count: i64,
}

#[cfg(all(test, feature = "host"))]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn extension_id_form() {
        for valid in ["a", "comix", "mangapill-gen", "x9"] {
            assert!(is_valid_extension_id(valid), "{valid} should be valid");
        }
        for invalid in ["", "A", "1a", "-a", "a_b", "a:b", "a b", "é"] {
            assert!(
                !is_valid_extension_id(invalid),
                "{invalid} should be invalid"
            );
        }
    }

    #[test]
    fn auto_scroll_marker_round_trips_and_defaults_off() {
        for auto_scroll in [true, false] {
            let marked = mark_auto_scroll("run()", auto_scroll);
            assert_eq!(take_auto_scroll(&marked), (auto_scroll, "run()"));
        }
        assert_eq!(take_auto_scroll("run()"), (false, "run()"));
    }

    #[test]
    fn download_status_serialises_as_integer() {
        assert_eq!(
            serde_json::to_string(&DownloadStatus::Complete).unwrap(),
            "2"
        );
        let s: DownloadStatus = serde_json::from_str("1").unwrap();
        assert_eq!(s, DownloadStatus::InProgress);
        assert_eq!(DownloadStatus::default(), DownloadStatus::Pending);
        assert!(serde_json::from_str::<DownloadStatus>("3").is_err());
        assert_eq!(i64::from(DownloadStatus::Complete), 2);
        assert_eq!(
            DownloadStatus::try_from(0).unwrap(),
            DownloadStatus::Pending
        );
    }

    fn json_rt<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
        v: &T,
    ) {
        let s = serde_json::to_string(v).unwrap();
        let back: T = serde_json::from_str(&s).unwrap();
        assert_eq!(*v, back);
    }

    #[test]
    fn manga_status_json_variants_are_lowercase() {
        for (status, expected) in [
            (MangaStatus::Ongoing, "\"ongoing\""),
            (MangaStatus::Completed, "\"completed\""),
            (MangaStatus::Hiatus, "\"hiatus\""),
            (MangaStatus::Cancelled, "\"cancelled\""),
            (MangaStatus::Unknown, "\"unknown\""),
        ] {
            assert_eq!(
                serde_json::to_string(&status).unwrap(),
                expected,
                "serialise {status:?}"
            );
            let back: MangaStatus = serde_json::from_str(expected).unwrap();
            assert_eq!(status, back, "deserialise {expected}");
        }
    }

    #[test]
    fn manga_status_default_is_unknown() {
        assert_eq!(MangaStatus::default(), MangaStatus::Unknown);
    }

    #[test]
    fn manga_status_from_i64_all_variants() {
        assert_eq!(MangaStatus::from(0i64), MangaStatus::Ongoing);
        assert_eq!(MangaStatus::from(1i64), MangaStatus::Completed);
        assert_eq!(MangaStatus::from(2i64), MangaStatus::Hiatus);
        assert_eq!(MangaStatus::from(3i64), MangaStatus::Cancelled);
        assert_eq!(MangaStatus::from(99i64), MangaStatus::Unknown);
    }

    #[test]
    fn manga_status_into_i64_all_variants() {
        assert_eq!(i64::from(MangaStatus::Ongoing), 0i64);
        assert_eq!(i64::from(MangaStatus::Completed), 1i64);
        assert_eq!(i64::from(MangaStatus::Hiatus), 2i64);
        assert_eq!(i64::from(MangaStatus::Cancelled), 3i64);
        assert_eq!(i64::from(MangaStatus::Unknown), 4i64);
    }

    #[test]
    fn filter_state_json_round_trip_all_variants() {
        for v in &[
            FilterState::Selection {
                name: "Genre".into(),
                value: "action".into(),
            },
            FilterState::Checkbox(true),
            FilterState::Checkbox(false),
            FilterState::TextInput("naruto".into()),
            FilterState::Multiselect(vec!["en".into(), "ja".into()]),
        ] {
            json_rt(v);
        }
    }

    #[test]
    fn active_filter_json_round_trip() {
        json_rt(&ActiveFilter {
            filter_name: "genre".into(),
            state: FilterState::Checkbox(false),
        });
    }

    #[test]
    fn manga_list_item_json_round_trip() {
        json_rt(&MangaListItem {
            id: "abc123".into(),
            title: "One Piece".into(),
            cover_url: Some("https://example.com/cover.jpg".into()),
            new_chapter_count: 5,
            resume: None,
            match_field: Some("author".into()),
            match_text: Some("Oda".into()),
            import_link_status: Some("pending".into()),
            is_orphaned: true,
        });
    }

    #[test]
    fn manga_list_item_default_new_chapter_count_is_zero() {
        let json = r#"{"id":"x","title":"t","cover_url":null}"#;
        let item: MangaListItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.new_chapter_count, 0);
    }

    #[test]
    fn manga_list_json_round_trip() {
        json_rt(&MangaList {
            manga: vec![MangaListItem {
                id: "1".into(),
                title: "A".into(),
                cover_url: None,
                new_chapter_count: 0,
                resume: None,
                match_field: None,
                match_text: None,
                import_link_status: None,
                is_orphaned: false,
            }],
            has_next_page: true,
            total_pages: Some(5),
        });
    }

    #[test]
    fn manga_info_json_round_trip_with_named_item_objects() {
        json_rt(&MangaInfo {
            id: "m1".into(),
            title: "Berserk".into(),
            cover_url: None,
            description: Some("Dark fantasy".into()),
            description_html: None,
            authors: vec![NamedItem {
                id: 1,
                name: "Kentaro Miura".into(),
            }],
            artists: vec![NamedItem {
                id: 2,
                name: "Studio Gaga".into(),
            }],
            status: MangaStatus::Ongoing,
            tags: vec![NamedItem {
                id: 3,
                name: "Action".into(),
            }],
        });
    }

    #[test]
    fn manga_info_deserializes_authors_as_plain_strings() {
        let json = r#"{
            "id":"m1","title":"Test","cover_url":null,
            "description":null,"description_html":null,
            "authors":["Oda"],
            "artists":[],
            "status":"ongoing",
            "tags":[]
        }"#;
        let info: MangaInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.authors.len(), 1);
        assert_eq!(info.authors[0].name, "Oda");
        assert_eq!(info.authors[0].id, 0);
    }

    #[test]
    fn chapter_json_round_trip() {
        json_rt(&Chapter {
            id: "ch1".into(),
            title: Some("Chapter 1: Dawn".into()),
            number: 1.0,
            volume: Some(1),
            language: "en".into(),
            scanlator: Some("TeamX".into()),
            date_uploaded: Some(1_700_000_000),
            download_status: DownloadStatus::Pending,
            is_orphaned: false,
            page_count: Some(20),
            is_read: true,
            last_page_read: Some(10),
            download_error: None,
            upgrade_available: None,
        });
    }

    #[test]
    fn chapter_missing_optional_fields_default_to_zero_or_false() {
        let json = r#"{"id":"c","number":1.0,"language":"en"}"#;
        let ch: Chapter = serde_json::from_str(json).unwrap();
        assert_eq!(ch.download_status, DownloadStatus::Pending);
        assert!(!ch.is_orphaned);
        assert!(!ch.is_read);
        assert!(ch.last_page_read.is_none());
    }

    #[test]
    fn chapter_list_json_round_trip() {
        json_rt(&ChapterList {
            chapters: vec![],
            has_next_page: false,
            total_pages: None,
            total: None,
        });
    }

    #[test]
    fn chapter_sort_order_default_is_chapter_desc() {
        assert_eq!(ChapterSortOrder::default(), ChapterSortOrder::ChapterDesc);
    }

    #[test]
    fn chapter_sort_order_select_value_round_trip_all_variants() {
        for order in &[
            ChapterSortOrder::ChapterDesc,
            ChapterSortOrder::ChapterAsc,
            ChapterSortOrder::UploadedDesc,
            ChapterSortOrder::UploadedAsc,
            ChapterSortOrder::VolumeDesc,
            ChapterSortOrder::VolumeAsc,
            ChapterSortOrder::LanguageAsc,
            ChapterSortOrder::LanguageDesc,
            ChapterSortOrder::ScanlatorAsc,
            ChapterSortOrder::ScanlatorDesc,
        ] {
            let val = order.to_select_value();
            let back = ChapterSortOrder::from_select_value(val);
            assert_eq!(*order, back, "failed round-trip for {order:?}");
        }
    }

    #[test]
    fn manga_sort_order_default_is_name_desc() {
        assert_eq!(MangaSortOrder::default(), MangaSortOrder::NameDesc);
    }

    #[test]
    fn manga_sort_order_select_value_round_trip_all_variants() {
        for order in &[
            MangaSortOrder::NameDesc,
            MangaSortOrder::NameAsc,
            MangaSortOrder::UpdatedDesc,
            MangaSortOrder::UpdatedAsc,
            MangaSortOrder::AddedDesc,
            MangaSortOrder::AddedAsc,
            MangaSortOrder::ScoreDesc,
            MangaSortOrder::ScoreAsc,
            MangaSortOrder::LastReadDesc,
        ] {
            let val = order.to_select_value();
            let back = MangaSortOrder::from_select_value(val);
            assert_eq!(*order, back, "failed round-trip for {order:?}");
        }
    }

    #[test]
    fn manga_sort_order_needs_columns_flags() {
        assert!(MangaSortOrder::LastReadDesc.needs_last_read_column());
        assert!(!MangaSortOrder::NameAsc.needs_last_read_column());
        assert!(MangaSortOrder::ScoreDesc.needs_tracking_join());
        assert!(MangaSortOrder::ScoreAsc.needs_tracking_join());
        assert!(MangaSortOrder::LastReadDesc.needs_tracking_join());
        assert!(!MangaSortOrder::NameAsc.needs_tracking_join());
    }

    #[test]
    fn download_progress_event_json_round_trip_all_variants() {
        for ev in &[
            DownloadProgressEvent::ChapterStarted {
                chapter_id: 1,
                chapter_name: "Ch1".into(),
                manga_id: 10,
                manga_title: "Manga".into(),
                total_pages: 20,
                job_id: None,
            },
            DownloadProgressEvent::PageCompleted {
                chapter_id: 1,
                chapter_name: "Ch1".into(),
                page_index: 5,
            },
            DownloadProgressEvent::ChapterCompleted {
                chapter_id: 1,
                chapter_name: "Ch1".into(),
                manga_id: 10,
                manga_title: "Manga".into(),
                successful_pages: 20,
            },
            DownloadProgressEvent::ChapterFailed {
                chapter_id: 1,
                chapter_name: "Ch1".into(),
                error: "timeout".into(),
            },
            DownloadProgressEvent::ChapterCancelled {
                chapter_id: 1,
                chapter_name: "Ch1".into(),
            },
        ] {
            json_rt(ev);
        }
    }

    #[test]
    fn download_rule_kind_json_round_trip_all_variants() {
        for rule in &[
            DownloadRuleKind::LanguageInclude("en".into()),
            DownloadRuleKind::LanguageExclude("ja".into()),
            DownloadRuleKind::TitleContains("vol".into()),
            DownloadRuleKind::TitleExcludes("omake".into()),
            DownloadRuleKind::ChapterNumberMin(1.5),
            DownloadRuleKind::ChapterNumberMax(100.0),
            DownloadRuleKind::ExcludeFractional,
            DownloadRuleKind::MaxAgeDays(30),
            DownloadRuleKind::PublishedAfter(1_700_000_000),
        ] {
            json_rt(rule);
        }
    }

    #[test]
    fn download_rule_kind_axis_values() {
        assert_eq!(DownloadRuleKind::LanguageInclude("en".into()).axis(), 0);
        assert_eq!(DownloadRuleKind::LanguageExclude("ja".into()).axis(), 0);
        assert_eq!(DownloadRuleKind::TitleContains("x".into()).axis(), 1);
        assert_eq!(DownloadRuleKind::TitleExcludes("x".into()).axis(), 1);
        assert_eq!(DownloadRuleKind::ChapterNumberMin(1.0).axis(), 2);
        assert_eq!(DownloadRuleKind::ChapterNumberMax(1.0).axis(), 2);
        assert_eq!(DownloadRuleKind::ExcludeFractional.axis(), 3);
        assert_eq!(DownloadRuleKind::MaxAgeDays(7).axis(), 4);
        assert_eq!(DownloadRuleKind::PublishedAfter(0).axis(), 4);
    }

    #[test]
    fn download_rule_kind_is_include() {
        assert!(DownloadRuleKind::LanguageInclude("en".into()).is_include());
        assert!(!DownloadRuleKind::LanguageExclude("en".into()).is_include());
        assert!(DownloadRuleKind::TitleContains("x".into()).is_include());
        assert!(!DownloadRuleKind::TitleExcludes("x".into()).is_include());
        assert!(!DownloadRuleKind::ExcludeFractional.is_include());
    }

    fn ch_row(
        language: &str,
        title: Option<&str>,
        number: f64,
        uploaded_at: Option<i64>,
    ) -> ChapterFilterRow {
        ChapterFilterRow {
            id: 1,
            scanlator: None,
            language: language.into(),
            name: title.map(Into::into),
            chapter_number: number,
            uploaded_at,
        }
    }

    #[test]
    fn download_rule_kind_passes_language_case_insensitive() {
        let rule = DownloadRuleKind::LanguageInclude("en".into());
        assert!(rule.passes(&ch_row("EN", None, 1.0, None)));
        assert!(rule.passes(&ch_row("en", None, 1.0, None)));
        assert!(!rule.passes(&ch_row("ja", None, 1.0, None)));
    }

    #[test]
    fn download_rule_kind_passes_language_exclude() {
        let rule = DownloadRuleKind::LanguageExclude("ja".into());
        assert!(rule.passes(&ch_row("en", None, 1.0, None)));
        assert!(!rule.passes(&ch_row("JA", None, 1.0, None)));
    }

    #[test]
    fn download_rule_kind_passes_title_contains() {
        let rule = DownloadRuleKind::TitleContains("Vol".into());
        assert!(rule.passes(&ch_row("en", Some("Volume 1"), 1.0, None)));
        assert!(!rule.passes(&ch_row("en", Some("Chapter 1"), 1.0, None)));
        assert!(!rule.passes(&ch_row("en", None, 1.0, None)));
    }

    #[test]
    fn download_rule_kind_passes_chapter_number_range() {
        let min = DownloadRuleKind::ChapterNumberMin(5.0);
        let max = DownloadRuleKind::ChapterNumberMax(10.0);
        assert!(min.passes(&ch_row("en", None, 5.0, None)));
        assert!(!min.passes(&ch_row("en", None, 4.99, None)));
        assert!(max.passes(&ch_row("en", None, 10.0, None)));
        assert!(!max.passes(&ch_row("en", None, 10.01, None)));
    }

    #[test]
    fn download_rule_kind_passes_exclude_fractional() {
        let rule = DownloadRuleKind::ExcludeFractional;
        assert!(rule.passes(&ch_row("en", None, 5.0, None)));
        assert!(!rule.passes(&ch_row("en", None, 5.5, None)));
    }

    #[test]
    fn download_rule_kind_passes_published_after() {
        let rule = DownloadRuleKind::PublishedAfter(1_000_000);
        assert!(rule.passes(&ch_row("en", None, 1.0, None)));
        assert!(rule.passes(&ch_row("en", None, 1.0, Some(1_000_001))));
        assert!(!rule.passes(&ch_row("en", None, 1.0, Some(999_999))));
    }

    #[test]
    fn source_json_round_trip() {
        json_rt(&Source {
            id: 1,
            name: "Example Source".into(),
            version: "1.0.0".into(),
            base_url: "https://example.com".into(),
            enabled: true,
            favourited: false,
            unrestricted_http: false,
            browser_enabled: true,
            download_concurrency: None,
            circuit_state: None,
            icon: Some("aWNvbg==".into()),
            description: Some("A manga source".into()),
            languages: Some(r#"["en","ja"]"#.into()),
            schema_version: 1,
        });
    }

    #[test]
    fn search_scope_json_round_trip_all_variants() {
        json_rt(&SearchScope::FavouritedOnly);
        json_rt(&SearchScope::AllEnabled);
        json_rt(&SearchScope::Sources(vec![1, 2, 3]));
    }

    #[test]
    fn manga_tracking_status_json_round_trip_all_variants() {
        for s in &[
            MangaTrackingStatus::Reading,
            MangaTrackingStatus::OnHold,
            MangaTrackingStatus::Dropped,
            MangaTrackingStatus::PlanToRead,
            MangaTrackingStatus::Completed,
            MangaTrackingStatus::Rereading,
        ] {
            json_rt(s);
        }
    }

    #[test]
    fn manga_tracking_status_repr_values() {
        assert_eq!(MangaTrackingStatus::Reading as i64, 0);
        assert_eq!(MangaTrackingStatus::OnHold as i64, 1);
        assert_eq!(MangaTrackingStatus::Dropped as i64, 2);
        assert_eq!(MangaTrackingStatus::PlanToRead as i64, 3);
        assert_eq!(MangaTrackingStatus::Completed as i64, 4);
        assert_eq!(MangaTrackingStatus::Rereading as i64, 5);
    }

    #[test]
    fn settings_update_json_round_trip_all_variants() {
        json_rt(&SettingsUpdate::Download(DownloadSettings {
            concurrent_page_downloads: 4,
            max_retries: 3,
            initial_retry_delay_ms: 1000,
            auto_download_category_ids: vec![1, 2],
            scan_concurrency: 3,
            per_source_download_concurrency: 2,
        }));
        json_rt(&SettingsUpdate::Scan(ScanSettings {
            auto_scan: true,
            scan_interval_minutes: 60,
            scan_exclude_completed: false,
            upgrade_detection_enabled: true,
            upgrade_min_res_gain: 1.2,
            upgrade_confirm_fetches: 3,
            upgrade_axis_resolution: "both".into(),
            upgrade_axis_colour: "both".into(),
            upgrade_axis_encoder: "both".into(),
            upgrade_axis_bitrate: "gain".into(),
            upgrade_show_downgrades: false,
            upgrade_auto_replace_reasons: "preferred_scanlator,resolution,colour".into(),
            scan_barren_page_tolerance: 3,
        }));
        json_rt(&SettingsUpdate::Advanced(AdvancedSettings {
            flaresolverr_url: String::new(),
            library_path: "/data/library".into(),
            wasm_storage_path: "/data/wasm".into(),
            max_wasm_instances: 4,
            http_request_logging: false,
            v8_debug_logging: false,
            registration_enabled: true,
            cover_max_dimension: Some(512),
            v8_max_memory_mb: 512,
            v8_idle_timeout_s: 300,
            update_check_enabled: true,
            opds_page_index_zero_based: false,
            global_search_timeout_secs: 6,
        }));
        json_rt(&SettingsUpdate::Tracking(TrackingSettings {
            default_tracking_enabled: true,
            tracker_auto_sync_enabled: false,
            tracker_sync_interval_hours: 24,
        }));
        json_rt(&SettingsUpdate::Email(EmailSettings {
            email_enabled: false,
            email_provider: "smtp".into(),
            email_provider_config: "{}".into(),
            email_from_address: "noreply@example.com".into(),
            app_url: "https://example.com".into(),
            password_reset_enabled: false,
            email_verification_required: false,
        }));
    }

    #[test]
    fn authenticated_user_role_checks() {
        let user = AuthenticatedUser {
            id: 1,
            username: "alice".into(),
            email: "alice@example.com".into(),
            roles: vec!["admin".into(), "user".into()],
            email_verified_at: None,
        };
        assert!(user.is_admin());
        assert!(user.has_role("admin"));
        assert!(user.has_role("user"));
        assert!(!user.has_role("moderator"));
    }

    #[test]
    fn authenticated_user_non_admin_does_not_have_admin_role() {
        let user = AuthenticatedUser {
            id: 2,
            username: "bob".into(),
            email: "bob@example.com".into(),
            roles: vec!["user".into()],
            email_verified_at: None,
        };
        assert!(!user.is_admin());
    }
}
