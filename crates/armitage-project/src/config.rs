use armitage_core::domain::Domain;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitHubProjectConfig {
    /// GitHub org that owns the project board.
    #[serde(default)]
    pub org: String,
    /// Project number from the board URL (e.g. 42 in /orgs/MyOrg/projects/42).
    #[serde(default)]
    pub number: u32,
    /// Name of the "Start date" field on the board (e.g. "Start date").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date_field: Option<String>,
    /// Name of the "Target date" field on the board (e.g. "Target date").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_date_field: Option<String>,
    /// Name of the Status field (optional; skip status sync if omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_field: Option<String>,
    #[serde(default)]
    pub status_values: StatusValues,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusValues {
    pub backlog: String,
    pub todo: String,
    pub sprint_todo: String,
    pub in_progress: String,
}

impl Default for StatusValues {
    fn default() -> Self {
        Self {
            backlog: "Backlog".to_string(),
            todo: "Todo".to_string(),
            sprint_todo: "Sprint Todo".to_string(),
            in_progress: "In Progress".to_string(),
        }
    }
}

pub struct ProjectDomain;

impl Domain for ProjectDomain {
    const NAME: &'static str = "project";
    const CONFIG_KEY: &'static str = "github_project";
    type Config = GitHubProjectConfig;
}
