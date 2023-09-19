use std::collections::BTreeMap;

/// Create lables that can be used as a unique selector for a given app name.
pub fn selector_labels(app: &str) -> Option<BTreeMap<String, String>> {
    Some(BTreeMap::from_iter(vec![(
        "app".to_owned(),
        app.to_owned(),
    )]))
}

/// Manage by label
pub const MANAGED_BY_LABEL_SELECTOR: &str = "managed-by=keramik";

/// Labels that indicate the resource is managed by the keramik operator.
pub fn managed_labels() -> Option<BTreeMap<String, String>> {
    Some(BTreeMap::from_iter(vec![(
        "managed-by".to_owned(),
        "keramik".to_owned(),
    )]))
}
