pub mod executor;
pub mod matcher;
pub mod registry;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub triggers: Vec<String>,
    #[serde(rename = "type")]
    pub action_type: ActionType,
    pub command: String,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub requires_permissions: Vec<String>,
    #[serde(default)]
    pub destructive: bool,
    #[serde(default)]
    pub success_feedback: String,
    #[serde(default)]
    pub failure_feedback: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Shell,
    Applescript,
    OpenUrl,
    OpenApp,
    Keystroke,
}
