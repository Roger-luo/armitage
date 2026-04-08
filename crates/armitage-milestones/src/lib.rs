pub mod error;
pub mod milestone;

use armitage_core::domain::Domain;

#[derive(Debug, Default, serde::Deserialize)]
pub struct MilestonesConfig {}

pub struct MilestonesDomain;

impl Domain for MilestonesDomain {
    const NAME: &'static str = "milestones";
    const CONFIG_KEY: &'static str = "milestones";
    type Config = MilestonesConfig;
    const NODE_FILES: &'static [&'static str] = &["milestones.toml"];
}
