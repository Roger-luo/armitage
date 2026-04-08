pub mod def;
pub mod error;
pub mod rename;
pub mod schema;

use armitage_core::domain::Domain;

pub struct LabelsDomain;

impl Domain for LabelsDomain {
    const NAME: &'static str = "labels";
    const CONFIG_KEY: &'static str = "label_schema";
    type Config = schema::LabelSchema;
    const NODE_FILES: &'static [&'static str] = &["labels.toml"];
}
