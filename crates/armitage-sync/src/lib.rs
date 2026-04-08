pub mod config;
pub mod conflict;
pub mod error;
pub mod hash;
pub mod merge;
pub mod pull;
pub mod push;
pub mod state;

use armitage_core::domain::Domain;

pub struct SyncDomain;

impl Domain for SyncDomain {
    const NAME: &'static str = "sync";
    const CONFIG_KEY: &'static str = "sync";
    type Config = config::SyncConfig;
}
