use std::collections::{BTreeMap, HashMap};

/// Named `passPayload` init scripts declared by a source, resolved by name at
/// browser-capture time. Owning them here (rather than reaching into the raw
/// config map at each call site) lets both the interpreted tier and Rhai hooks
/// share one lookup, and keeps raw JS out of hook scripts.
#[derive(Debug, Default, Clone)]
pub struct BrowserScriptRegistry {
    scripts: HashMap<String, String>,
}

impl BrowserScriptRegistry {
    pub fn from_map(scripts: &BTreeMap<String, String>) -> Self {
        Self {
            scripts: scripts.clone().into_iter().collect(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.scripts.get(name).map(String::as_str)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.scripts.keys().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn registry() -> BrowserScriptRegistry {
        let mut m = BTreeMap::new();
        m.insert("catalog".to_string(), "passPayload('{}')".to_string());
        BrowserScriptRegistry::from_map(&m)
    }

    #[test]
    fn get_hit_returns_script() {
        assert_eq!(registry().get("catalog"), Some("passPayload('{}')"));
    }

    #[test]
    fn get_miss_returns_none() {
        assert_eq!(registry().get("missing"), None);
    }

    #[test]
    fn empty_registry_is_empty() {
        assert!(BrowserScriptRegistry::from_map(&BTreeMap::new()).is_empty());
        assert!(!registry().is_empty());
    }
}
