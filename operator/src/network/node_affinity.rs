use k8s_openapi::api::core::v1::{Affinity, NodeAffinity, NodeSelector, PodSpec, PodTemplateSpec};

use super::NetworkSpec;

#[derive(Default)]
pub struct NodeAffinityConfig {
    node_selector_terms: Option<Vec<k8s_openapi::api::core::v1::NodeSelectorTerm>>,
}

impl NodeAffinityConfig {
    pub fn apply_to_pod_template(&self, pod_template: PodTemplateSpec) -> PodTemplateSpec {
        if let Some(node_selector_terms) = &self.node_selector_terms {
            PodTemplateSpec {
                spec: pod_template.spec.map(|spec| PodSpec {
                    affinity: Some(Affinity {
                        node_affinity: Some(NodeAffinity {
                            required_during_scheduling_ignored_during_execution: Some(
                                NodeSelector {
                                    node_selector_terms: node_selector_terms.clone(),
                                },
                            ),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..spec
                }),
                ..pod_template
            }
        } else {
            pod_template
        }
    }
}

impl From<&NetworkSpec> for NodeAffinityConfig {
    fn from(value: &NetworkSpec) -> Self {
        Self {
            node_selector_terms: value.node_selector_terms.clone(),
        }
    }
}
