#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented
)]

//! Enforces ARCHITECTURE.md §2 — dependency direction.
//!
//! Adding an edge here is deliberately annoying: it forces a conversation in the PR.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

/// The complete set of allowed intra-workspace dependency edges.
const ALLOWED: &[(&str, &[&str])] = &[
    ("dit-model", &[]),
    ("dit-parse", &["dit-model"]),
    ("dit-query", &["dit-model"]),
    ("dit-store", &["dit-model", "dit-parse"]),
    ("dit-index", &["dit-model", "dit-parse", "dit-query"]),
    // The merge driver parses the three file versions it is handed, so it
    // needs the frontmatter reader alongside the domain types.
    ("dit-vcs", &["dit-model", "dit-parse"]),
    ("dit-ai", &["dit-model"]),
    (
        "dit-core",
        &[
            "dit-model",
            "dit-parse",
            "dit-query",
            "dit-store",
            "dit-index",
            "dit-vcs",
            "dit-ai",
        ],
    ),
    ("dit-cli", &["dit-core"]),
    ("dit-server", &["dit-core"]),
    ("dit-wasm", &["dit-model", "dit-parse", "dit-query"]),
];

fn workspace_graph() -> BTreeMap<String, BTreeSet<String>> {
    let out = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .expect("cargo metadata failed to run");
    let json = String::from_utf8(out.stdout).expect("cargo metadata emitted non-utf8");

    // Deliberately dependency-free parsing: this test must not itself pull in
    // a crate that could drift.
    let members: BTreeSet<String> = ALLOWED.iter().map(|(n, _)| (*n).to_owned()).collect();
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for chunk in json.split("\"name\":\"").skip(1) {
        let name = chunk.split('"').next().unwrap_or_default().to_owned();
        if !members.contains(&name) {
            continue;
        }
        let deps: BTreeSet<String> = members
            .iter()
            .filter(|m| **m != name && chunk.contains(&format!("\"name\":\"{m}\"")))
            .cloned()
            .collect();
        graph.entry(name).or_default().extend(deps);
    }
    graph
}

#[test]
fn dependencies_point_inward() {
    let allowed: BTreeMap<&str, BTreeSet<&str>> = ALLOWED
        .iter()
        .map(|(k, v)| (*k, v.iter().copied().collect()))
        .collect();

    let mut violations = Vec::new();
    for (crate_name, deps) in workspace_graph() {
        let Some(permitted) = allowed.get(crate_name.as_str()) else {
            violations.push(format!("crate `{crate_name}` is not listed in ALLOWED"));
            continue;
        };
        for dep in deps {
            if !permitted.contains(dep.as_str()) {
                violations.push(format!("`{crate_name}` -> `{dep}` is not an allowed edge"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "dependency direction violated (ARCHITECTURE.md §2):\n  {}\n\n\
         If the new edge is correct, add it to ALLOWED in this file and say why in the PR.",
        violations.join("\n  ")
    );
}

#[test]
fn every_member_is_declared() {
    let declared: BTreeSet<&str> = ALLOWED.iter().map(|(n, _)| *n).collect();
    let on_disk: BTreeSet<String> = std::fs::read_dir("crates")
        .expect("crates/ must exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    let missing: Vec<_> = on_disk
        .iter()
        .filter(|c| !declared.contains(c.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "crates on disk but absent from ALLOWED: {missing:?}"
    );
}
