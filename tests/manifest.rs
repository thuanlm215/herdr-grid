use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    id: String,
    name: String,
    version: String,
    min_herdr_version: String,
    platforms: Vec<String>,
    build: Vec<Build>,
    panes: Vec<Pane>,
    actions: Vec<Action>,
}
#[derive(Deserialize)]
struct Build {
    command: Vec<String>,
}
#[derive(Deserialize)]
struct Pane {
    id: String,
    placement: String,
    command: Vec<String>,
}
#[derive(Deserialize)]
struct Action {
    title: String,
    command: Vec<String>,
}

#[test]
fn manifest_declares_build_popup_and_launcher() {
    let manifest: Manifest = toml::from_str(include_str!("../herdr-plugin.toml")).unwrap();
    assert_eq!(manifest.id, "herdr-grid");
    assert_eq!(manifest.name, "herdr-grid");
    assert_eq!(manifest.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest.min_herdr_version, "0.8.2");
    assert_eq!(manifest.platforms, ["linux", "macos"]);
    assert_eq!(
        manifest.build[0].command,
        ["cargo", "build", "--release", "--locked"]
    );
    assert_eq!(manifest.panes[0].id, "grid");
    assert_eq!(manifest.panes[0].placement, "popup");
    assert_eq!(manifest.panes[0].command, ["bash", "scripts/run-pane.sh"]);
    assert!(!manifest.actions[0].title.is_empty());
    assert_eq!(
        manifest.actions[0].command,
        ["./target/release/herdr-grid", "--open-popup"]
    );
}
