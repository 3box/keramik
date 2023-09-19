use crate::simulation::SimulationSpec;

/// Configuration for job images.
#[derive(Clone, Debug)]
pub struct JobImageConfig {
    /// Image for all jobs created by the simulation.
    pub image: String,
    /// Pull policy for image.
    pub image_pull_policy: String,
}

impl Default for JobImageConfig {
    fn default() -> Self {
        Self {
            image: "public.ecr.aws/r5b3e0r5/3box/keramik-runner:latest".to_owned(),
            image_pull_policy: "Always".to_owned(),
        }
    }
}

impl From<&SimulationSpec> for JobImageConfig {
    fn from(value: &SimulationSpec) -> Self {
        let default = Self::default();
        Self {
            image: value.image.to_owned().unwrap_or(default.image),
            image_pull_policy: value
                .image_pull_policy
                .to_owned()
                .unwrap_or(default.image_pull_policy),
        }
    }
}
