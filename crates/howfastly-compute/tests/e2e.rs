use std::env;
use std::process::Command;

#[test]
fn viceroy() {
    // commonCargoSources carries the rs alone, so the nix check hands in the
    // script from the store and a dev shell run takes the one in the tree
    let script = env::var("HOWFASTLY_E2E").unwrap_or_else(|_| "tests/e2e.nu".into());
    let status = Command::new("nu")
        .arg(script)
        .status()
        .expect("nu not found, run inside the dev shell");
    assert!(status.success());
}
