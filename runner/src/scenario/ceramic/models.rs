use ceramic_http_client::GetRootSchema;
use rand::{
    distributions::{Alphanumeric, DistString},
    prelude::*,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub trait RandomModelInstance {
    fn random() -> Self;
}

#[derive(Deserialize, JsonSchema, Serialize)]
#[schemars(rename_all = "camelCase", deny_unknown_fields)]
pub struct SmallModel {
    creator: String,
    radius: i32,
    red: i32,
    green: i32,
    blue: i32,
}

impl GetRootSchema for SmallModel {}

impl RandomModelInstance for SmallModel {
    fn random() -> Self {
        let mut rng = thread_rng();
        Self {
            creator: "keramik".to_string(),
            radius: rng.gen_range(0..100),
            red: rng.gen_range(0..255),
            green: rng.gen_range(0..255),
            blue: rng.gen_range(0..255),
        }
    }
}

#[derive(Deserialize, JsonSchema, Serialize)]
#[schemars(rename_all = "camelCase", deny_unknown_fields)]
pub struct LargeModel {
    pub creator: String,
    pub name: String,
    pub description: String,
    pub tpe: i64,
}

impl GetRootSchema for LargeModel {}

impl LargeModel {
    pub fn new(name: String, description: String, tpe: i64) -> Self {
        Self {
            creator: "keramik".to_string(),
            name,
            description,
            tpe,
        }
    }

    pub fn random_1kb() -> Self {
        let mut rng = thread_rng();

        Self {
            creator: "keramik".to_string(),
            name: format!(
                "keramik-large-model-{}",
                Alphanumeric.sample_string(&mut rand::thread_rng(), 40)
            ),
            description: Alphanumeric.sample_string(&mut rand::thread_rng(), 200),
            tpe: rng.gen_range(0..100),
        }
    }
}

impl RandomModelInstance for LargeModel {
    fn random() -> Self {
        let mut rng = thread_rng();
        let name = Alphanumeric.sample_string(&mut rand::thread_rng(), 100);
        Self {
            creator: "keramik".to_string(),
            name: format!("keramik-large-model-{}", name),
            description: (0..1_000)
                .map(|_| rng.gen::<char>())
                .filter(|c| *c != '\u{0000}')
                .collect(),
            tpe: rng.gen_range(0..100),
        }
    }
}
