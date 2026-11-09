//! The domain layer must stay free of platform crates (docs/adr/0003-clean-architecture.md).

const MANIFEST: &str = include_str!("../Cargo.toml");

const PLATFORM_CRATES: [&str; 9] = [
    "slint",
    "windows",
    "ureq",
    "tray-icon",
    "global-hotkey",
    "nvml-wrapper",
    "sysinfo",
    "raw-window-handle",
    "hud-app",
];

fn dependency_names(manifest: &str) -> Vec<&str> {
    manifest
        .lines()
        .skip_while(|l| !l.trim().starts_with("[dependencies]"))
        .skip(1)
        .take_while(|l| !l.trim().starts_with('['))
        .filter_map(|l| l.split('=').next())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect()
}

#[test]
fn core_depends_on_no_platform_crate() {
    let names = dependency_names(MANIFEST);
    assert!(!names.is_empty());
    for crate_name in PLATFORM_CRATES {
        assert!(
            !names.contains(&crate_name),
            "hud-core must not depend on `{crate_name}`"
        );
    }
}

#[test]
fn dependency_parser_sees_the_real_manifest_entries() {
    let names = dependency_names(MANIFEST);
    assert!(names.contains(&"serde"));
    assert!(names.contains(&"thiserror"));
}

#[test]
fn dependency_parser_catches_a_platform_crate() {
    let names = dependency_names("[dependencies]\nslint = \"1\"\n[dev-dependencies]\nx = 1\n");
    assert_eq!(names, vec!["slint"]);
}

fn production_code(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap_or(source)
}

#[test]
fn core_production_code_never_touches_the_real_system() {
    let forbidden = [
        "std::fs",
        "std::env",
        "std::net",
        "std::process",
        "SystemTime",
    ];
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for entry in std::fs::read_dir(src).expect("read hud-core/src") {
        let path = entry.expect("dir entry").path();
        let text = std::fs::read_to_string(&path).expect("read source");
        for token in forbidden {
            assert!(
                !production_code(&text).contains(token),
                "{} uses `{token}`; real system access belongs behind a port in hud-app",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked > 10);
}
