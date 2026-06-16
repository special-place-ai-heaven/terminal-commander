// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Ensures MCP fixture documentation cannot silently drift from the live
//! rmcp catalogue.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use terminal_commander_mcp::tools::tool_catalogue;

#[derive(Debug, Deserialize)]
struct FixtureMap {
    daemon_unavailable_shapes: BTreeMap<String, String>,
    live_tools: Vec<MapEntry>,
    obsolete_fixtures: Vec<MapEntry>,
    counts: MapCounts,
}

#[derive(Debug, Deserialize)]
struct MapEntry {
    name: String,
    fixture: Option<String>,
    status: String,
}

#[derive(Debug, Deserialize)]
struct MapCounts {
    live_tools: usize,
    covered_live: usize,
    placeholder_for_live_tool: usize,
    missing_fixture: usize,
    obsolete_fixture_present: usize,
}

#[derive(Debug, Deserialize)]
struct SystemDiscoverFixture {
    response_example_daemon_unavailable: SystemDiscoverExample,
}

#[derive(Debug, Deserialize)]
struct SystemDiscoverExample {
    tools: Vec<SystemDiscoverTool>,
}

#[derive(Debug, Deserialize)]
struct SystemDiscoverTool {
    name: String,
}

#[test]
fn fixture_map_matches_live_tool_catalogue() {
    let workspace = workspace_root();
    let fixture_root = workspace.join("tests/fixtures/contracts");
    let map = read_json::<FixtureMap>(&fixture_root.join("mcp-tool-fixture-map.v1.json"));
    let discover =
        read_json::<SystemDiscoverFixture>(&fixture_root.join("mcp-tools/system_discover.v1.json"));

    let catalogue_names: BTreeSet<&'static str> =
        tool_catalogue().iter().map(|tool| tool.name).collect();
    assert_eq!(catalogue_names.len(), tool_catalogue().len());

    let map_names: BTreeSet<&str> = map
        .live_tools
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(
        map_names,
        catalogue_names.iter().copied().collect::<BTreeSet<_>>(),
        "mcp-tool-fixture-map live_tools must match tool_catalogue() exactly"
    );

    let discover_names: BTreeSet<&str> = discover
        .response_example_daemon_unavailable
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(
        discover_names,
        catalogue_names.iter().copied().collect::<BTreeSet<_>>(),
        "system_discover fixture must mirror tool_catalogue() exactly"
    );

    let shape_names: BTreeSet<&str> = map
        .daemon_unavailable_shapes
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        shape_names, map_names,
        "daemon_unavailable_shapes must cover every live mapped tool exactly"
    );

    let mut covered_live = 0;
    let mut placeholders = 0;
    let mut missing = 0;

    for entry in &map.live_tools {
        match entry.status.as_str() {
            "covered_live" => {
                covered_live += 1;
                let fixture = entry
                    .fixture
                    .as_ref()
                    .expect("covered_live entry must name a fixture file");
                assert_fixture_status(&fixture_root, fixture, "live");
                let shape = map
                    .daemon_unavailable_shapes
                    .get(&entry.name)
                    .expect("live tool must declare daemon_unavailable shape");
                assert_daemon_unavailable_shape(&fixture_root, fixture, &entry.name, shape);
            }
            "placeholder_for_live_tool" => {
                placeholders += 1;
                let fixture = entry
                    .fixture
                    .as_ref()
                    .expect("placeholder entry must name a fixture file");
                assert_fixture_status(&fixture_root, fixture, "reserved-not-implemented");
            }
            "missing_fixture" => {
                missing += 1;
                assert!(
                    entry.fixture.is_none(),
                    "missing fixture entry {} must not point at a file",
                    entry.name
                );
            }
            other => panic!(
                "unexpected live tool fixture-map status {other:?} for {}",
                entry.name
            ),
        }
    }

    assert_obsolete_fixture_map(&fixture_root, &map.obsolete_fixtures, &catalogue_names);
    assert_mcp_tool_fixture_inventory(&fixture_root, &map);

    assert_eq!(map.counts.live_tools, catalogue_names.len());
    assert_eq!(map.counts.covered_live, covered_live);
    assert_eq!(map.counts.placeholder_for_live_tool, placeholders);
    assert_eq!(map.counts.missing_fixture, missing);
    assert_eq!(
        map.counts.obsolete_fixture_present,
        map.obsolete_fixtures.len()
    );
}

fn assert_obsolete_fixture_map(
    fixture_root: &Path,
    obsolete_fixtures: &[MapEntry],
    catalogue_names: &BTreeSet<&'static str>,
) {
    let obsolete_names: BTreeSet<&str> = obsolete_fixtures
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    let live_names: BTreeSet<&str> = catalogue_names.iter().copied().collect();
    assert!(
        obsolete_names.is_disjoint(&live_names),
        "obsolete fixture names must not also be live catalogue names"
    );
    for entry in obsolete_fixtures {
        assert_eq!(entry.status, "obsolete_fixture_present");
        let fixture = entry
            .fixture
            .as_ref()
            .expect("obsolete fixture entry must name a fixture file");
        assert!(
            fixture_root.join(fixture).exists(),
            "obsolete fixture map entry points at missing file: {fixture}"
        );
    }
}

fn assert_mcp_tool_fixture_inventory(fixture_root: &Path, map: &FixtureMap) {
    let mut mapped: BTreeSet<String> = BTreeSet::new();
    for entry in map.live_tools.iter().chain(map.obsolete_fixtures.iter()) {
        if let Some(fixture) = &entry.fixture {
            assert!(
                fixture_root.join(fixture).exists(),
                "mapped fixture path does not exist: {fixture}"
            );
            assert!(
                mapped.insert(fixture.clone()),
                "fixture path is mapped more than once: {fixture}"
            );
        }
    }

    let mcp_tools_dir = fixture_root.join("mcp-tools");
    let actual: BTreeSet<String> = fs::read_dir(&mcp_tools_dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", mcp_tools_dir.display()))
        .map(|entry| {
            let entry = entry.unwrap_or_else(|err| panic!("read mcp-tools entry: {err}"));
            let file_name = entry
                .file_name()
                .into_string()
                .unwrap_or_else(|_| panic!("non-utf8 fixture path: {}", entry.path().display()));
            format!("mcp-tools/{file_name}")
        })
        .filter(|path| path.ends_with(".v1.json"))
        .collect();
    assert_eq!(
        actual, mapped,
        "mcp-tools fixture files must be exactly live-mapped or obsolete-mapped"
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("mcp crate has workspace parent")
        .parent()
        .expect("crates dir has workspace parent")
        .to_path_buf()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> T {
    let raw =
        fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

fn assert_fixture_status(fixture_root: &Path, fixture: &str, expected_status: &str) {
    let path = fixture_root.join(fixture);
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read fixture {}: {err}", path.display()));
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("parse fixture {}: {err}", path.display()));
    let actual = parsed
        .pointer("/_meta/status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    assert_eq!(
        actual, expected_status,
        "fixture status mismatch for {fixture}"
    );
}

fn assert_daemon_unavailable_shape(
    fixture_root: &Path,
    fixture: &str,
    tool_name: &str,
    expected_shape: &str,
) {
    let path = fixture_root.join(fixture);
    let parsed: serde_json::Value = read_json(&path);

    match expected_shape {
        "not_applicable" => {
            assert_eq!(
                tool_name, "system_discover",
                "only system_discover may mark daemon_unavailable as not_applicable"
            );
            assert!(
                parsed.get("unavailable_error_example").is_none(),
                "not_applicable tool {tool_name} must not carry daemon_unavailable error fixture"
            );
        }
        "legacy_state_unavailable" => {
            let details = parsed
                .pointer("/unavailable_error_example/data/details")
                .unwrap_or_else(|| {
                    panic!("legacy daemon_unavailable fixture missing details: {fixture}")
                });
            assert_eq!(
                details.get("state").and_then(serde_json::Value::as_str),
                Some("unavailable"),
                "legacy daemon_unavailable fixture must declare details.state for {fixture}"
            );
            assert!(
                details.get("status").is_none(),
                "legacy daemon_unavailable fixture {fixture} must not be mislabeled canonical"
            );
        }
        "ensure_daemon_status_v1" => {
            let details = parsed
                .pointer("/unavailable_error_example/data/details")
                .unwrap_or_else(|| {
                    panic!("canonical daemon_unavailable fixture missing details: {fixture}")
                });
            assert_eq!(
                details.get("status").and_then(serde_json::Value::as_str),
                Some("unavailable"),
                "canonical daemon_unavailable fixture must use details.status for {fixture}"
            );
            assert_eq!(
                details.get("reason").and_then(serde_json::Value::as_str),
                Some("binary_not_found"),
                "canonical daemon_unavailable fixture must name the example reason for {fixture}"
            );
            assert!(
                details.get("state").is_none(),
                "canonical daemon_unavailable fixture {fixture} must not retain legacy details.state"
            );
            let diagnostics = details
                .get("diagnostics")
                .and_then(serde_json::Value::as_object)
                .unwrap_or_else(|| {
                    panic!(
                        "canonical daemon_unavailable fixture missing diagnostics object: {fixture}"
                    )
                });
            assert_eq!(
                details
                    .pointer("/diagnostics/endpoint/kind")
                    .and_then(serde_json::Value::as_str),
                Some("unix_socket"),
                "canonical daemon_unavailable fixture must include endpoint.kind for {fixture}"
            );
            for key in [
                "log_path",
                "last_error",
                "startup_attempted",
                "startup_elapsed_ms",
            ] {
                assert!(
                    diagnostics.contains_key(key),
                    "canonical daemon_unavailable fixture {fixture} missing diagnostics.{key}"
                );
            }
        }
        "target_local_independent" => {
            // P5: target_list answers from the adapter-side targets.toml
            // registry, so (like system_discover) it carries no
            // daemon_unavailable error fixture. Unlike `not_applicable` this
            // shape is reserved for remote-federation listing tools, not
            // system_discover.
            assert_eq!(
                tool_name, "target_list",
                "only target_list may use the target_local_independent shape"
            );
            assert!(
                parsed.get("unavailable_error_example").is_none(),
                "target_local_independent tool {tool_name} must not carry a daemon_unavailable error fixture"
            );
        }
        other => panic!("unexpected daemon_unavailable shape {other:?} for {tool_name}"),
    }
}
