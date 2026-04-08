pub mod apply;
pub mod cache;
pub mod categories;
pub mod config;
pub mod db;
pub mod error;
pub mod examples;
pub mod fetch;
pub mod label_import;
pub mod llm;
pub mod review;

use armitage_core::domain::Domain;

pub struct TriageDomain;

impl Domain for TriageDomain {
    const NAME: &'static str = "triage";
    const CONFIG_KEY: &'static str = "triage";
    type Config = config::TriageConfig;
}
