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
        "acme",
        Some("Acme"),
        Some("Acme platform"),
        None,
        None,
        "active",
    )
    .unwrap();
    armitage::cli::node::create_node(&org, "acme/auth", None, None, None, None, "active").unwrap();
    armitage::cli::node::create_node(&org, "widget", Some("Widget"), None, None, None, "active")
        .unwrap();

    // Walk tree — use armitage_core
    let nodes = armitage_core::tree::walk_nodes(&org).unwrap();
    assert_eq!(nodes.len(), 3);

    // List children — use armitage_core
    let children = armitage_core::tree::list_children(&org, "acme").unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].path, "acme/auth");

    // Read node — use armitage_core
    let entry = armitage_core::tree::read_node(&org, "acme").unwrap();
    assert_eq!(entry.node.name, "Acme");

    // Move node
    armitage::cli::node::move_node(&org, "widget", "acme/widget").unwrap();

    // Verify move
    let nodes_after = armitage_core::tree::walk_nodes(&org).unwrap();
    assert_eq!(nodes_after.len(), 3);
    assert!(nodes_after.iter().any(|e| e.path == "acme/widget"));
    assert!(!nodes_after.iter().any(|e| e.path == "widget"));

    // Remove node
    std::fs::remove_dir_all(org.join("acme/widget")).unwrap();

    let nodes_final = armitage_core::tree::walk_nodes(&org).unwrap();
    assert_eq!(nodes_final.len(), 2);
}
