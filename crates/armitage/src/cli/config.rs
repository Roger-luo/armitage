use crate::error::{Error, Result};
use armitage_core::org::Org;
use armitage_core::tree::find_org_root;
use armitage_triage::TriageDomain;
use armitage_triage::config::TriageConfig;

pub fn run_set(key: String, value: String) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let org = Org::open(&org_root)?;
    let mut info = org.info().clone();
    let mut triage: TriageConfig = org.domain_config::<TriageDomain>()?;

    let value = if value.is_empty() { None } else { Some(value) };

    match key.as_str() {
        "org.default_repo" => info.default_repo.clone_from(&value),
        "triage.backend" => triage.backend.clone_from(&value),
        "triage.model" => triage.model.clone_from(&value),
        "triage.effort" => triage.effort.clone_from(&value),
        other => {
            return Err(Error::Other(format!("unknown config key: '{other}'")));
        }
    }

    // Rebuild the config table, preserving other sections
    let mut raw = org.raw_config().clone();
    raw.insert(
        "org".to_string(),
        toml::Value::try_from(&info).map_err(|e| Error::Other(e.to_string()))?,
    );
    raw.insert(
        "triage".to_string(),
        toml::Value::try_from(&triage).map_err(|e| Error::Other(e.to_string()))?,
    );
    let toml_content = toml::to_string(&raw)?;
    std::fs::write(org_root.join("armitage.toml"), toml_content)?;

    match &value {
        Some(v) => println!("Set {key} = {v}"),
        None => println!("Cleared {key}"),
    }
    Ok(())
}

pub fn run_set_secret(name: String) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let value = dialoguer::Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt(format!("Enter value for {name}"))
        .interact()
        .map_err(|e| Error::Other(e.to_string()))?;
    armitage_core::secrets::write_secret(&org_root, &name, &value)?;
    println!("Secret '{name}' saved to .armitage/secrets.toml");
    Ok(())
}

pub fn run_show() -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let content = std::fs::read_to_string(org_root.join("armitage.toml"))?;
    print!("{content}");
    Ok(())
}
