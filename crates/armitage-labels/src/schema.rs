use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LabelSchema {
    #[serde(default)]
    pub prefixes: Vec<LabelPrefix>,
    /// Label naming style guidance included in LLM prompts.
    #[serde(default)]
    pub style: Option<LabelStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelPrefix {
    pub prefix: String,
    pub category: String,
    #[serde(default)]
    pub examples: Vec<String>,
}

/// Configurable label naming convention for LLM suggestions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelStyle {
    /// Free-text description of the naming convention.
    pub convention: String,
    /// Example labels showing the naming convention.
    #[serde(default)]
    pub examples: Vec<LabelStyleExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelStyleExample {
    pub name: String,
    pub description: String,
}
