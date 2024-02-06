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
/// Labels that indicate the resource is managed by the keramik operator extend with custom labels.
pub fn managed_labels_extend(
    extend_labels: Option<BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    if let Some(extend_labels) = extend_labels {
        let mut labels = managed_labels().unwrap();
        labels.extend(extend_labels);
        Some(labels)
    } else {
        managed_labels()
    }
}
