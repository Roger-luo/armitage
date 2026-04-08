use tempfile::TempDir;

#[test]
fn full_local_workflow() {
    let tmp = TempDir::new().unwrap();
    let org = tmp.path().join("testorg");

    // Init
    armitage::cli::init::init_at(&org, "testorg", &["testorg".to_string()], None).unwrap();
    assert!(org.join("armitage.toml").exists());
    assert!(org.join(".armitage").exists());

    // Create nodes
    armitage::cli::node::create_node(
        &org,
        "gemini",
        Some("Gemini"),
        Some("AI platform"),
        None,
        None,
        "active",
    )
    .unwrap();
    armitage::cli::node::create_node(&org, "gemini/auth", None, None, None, None, "active")
        .unwrap();
    armitage::cli::node::create_node(&org, "m4", Some("M4"), None, None, None, "active").unwrap();

    // Walk tree — use armitage_core
    let nodes = armitage_core::tree::walk_nodes(&org).unwrap();
    assert_eq!(nodes.len(), 3);

    // List children — use armitage_core
    let children = armitage_core::tree::list_children(&org, "gemini").unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].path, "gemini/auth");

    // Add milestone
    armitage::cli::milestone::add_milestone(
        &org,
        "gemini",
        "Alpha",
        "2026-03-15",
        "Core ready",
        "checkpoint",
        None,
        None,
    )
    .unwrap();

    let ms = armitage::cli::milestone::read_milestones(&org, "gemini").unwrap();
    assert_eq!(ms.milestones.len(), 1);

    // Read node — use armitage_core
    let entry = armitage_core::tree::read_node(&org, "gemini").unwrap();
    assert_eq!(entry.node.name, "Gemini");

    // Move node
    armitage::cli::node::move_node(&org, "m4", "gemini/m4").unwrap();

    // Verify move
    let nodes_after = armitage_core::tree::walk_nodes(&org).unwrap();
    assert_eq!(nodes_after.len(), 3);
    assert!(nodes_after.iter().any(|e| e.path == "gemini/m4"));
    assert!(!nodes_after.iter().any(|e| e.path == "m4"));

    // Remove node
    std::fs::remove_dir_all(org.join("gemini/m4")).unwrap();

    let nodes_final = armitage_core::tree::walk_nodes(&org).unwrap();
    assert_eq!(nodes_final.len(), 2);
}
