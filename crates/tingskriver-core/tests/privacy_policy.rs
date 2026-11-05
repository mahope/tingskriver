//! #191: every external host in the code must be named in `PRIVACY_POLICY.md`.
//!
//! The policy is a table of what leaves the machine and to whom. A new
//! provider, download source or update feed in the code without a row in
//! that table makes this test fail, so the check runs on every change and
//! therefore before every release. Add the host to the table (feature, what
//! leaves, to whom, how to turn it off), not to the exemptions below.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Source trees that can make network requests.
const SCANNED: &[&str] = &[
    "crates/tingskriver-core/src",
    "crates/tingskriver-app/src",
    "crates/tingskriver-host/src",
    "crates/tingskriver-overlay/src",
    "frontend/src-tauri/src",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().unwrap().to_string_lossy();
            if name != "tests.rs" && !name.ends_with("_tests.rs") {
                out.push(path);
            }
        }
    }
}

/// Not remote: this machine, private networks, and reserved example names.
fn is_exempt(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host.ends_with(".invalid")
        || host.ends_with(".example")
        || host == "example.com"
        || host.ends_with(".example.com")
        || host.ends_with(".eksempel.com")
}

/// Hosts in string literals of non-test code. Only `#[cfg(test)]` modules
/// (which end with a `}` in column 0, as rustfmt writes them) are skipped,
/// so code after a test module is still scanned; comments are skipped too.
fn hosts_in(src: &str) -> Vec<String> {
    let url = Regex::new(r"https?://([A-Za-z0-9][A-Za-z0-9.-]*)").unwrap();
    let mut hosts = Vec::new();
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed == "#[cfg(test)]" {
            let mut item = lines.next().unwrap_or("").trim_start();
            while item.starts_with("#[") {
                item = lines.next().unwrap_or("").trim_start();
            }
            if item.starts_with("mod ") && item.ends_with('{') {
                for inner in lines.by_ref() {
                    if inner == "}" {
                        break;
                    }
                }
            }
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        for caps in url.captures_iter(line) {
            hosts.push(caps[1].trim_end_matches('.').to_string());
        }
    }
    hosts
}

fn undocumented(policy: &str, hosts: &[String]) -> Vec<String> {
    let policy = policy.to_ascii_lowercase();
    let mut missing: Vec<String> = hosts
        .iter()
        .filter(|h| !is_exempt(h) && !policy.contains(&h.to_ascii_lowercase()))
        .cloned()
        .collect();
    missing.sort();
    missing.dedup();
    missing
}

#[test]
fn every_external_host_in_the_code_is_in_the_policy() {
    let root = workspace_root();
    let policy = fs::read_to_string(root.join("PRIVACY_POLICY.md")).expect("PRIVACY_POLICY.md");
    let mut files = Vec::new();
    for dir in SCANNED {
        rust_files(&root.join(dir), &mut files);
    }
    assert!(files.len() > 50, "found only {} source files", files.len());
    let mut missing = Vec::new();
    for file in files {
        let hosts = hosts_in(&fs::read_to_string(&file).unwrap());
        for host in undocumented(&policy, &hosts) {
            missing.push(format!("{}: {host}", file.strip_prefix(&root).unwrap().display()));
        }
    }
    assert!(
        missing.is_empty(),
        "hosts missing from PRIVACY_POLICY.md (add a table row):\n{}",
        missing.join("\n")
    );
}

#[test]
fn the_policy_names_no_analytics_and_has_a_real_date() {
    let policy = fs::read_to_string(workspace_root().join("PRIVACY_POLICY.md")).unwrap();
    assert!(!policy.contains("[Current Date]"));
    assert!(!policy.to_ascii_lowercase().contains("posthog"));
    assert!(Regex::new(r"Last updated: \d{1,2} \w+ 20\d\d").unwrap().is_match(&policy));
}

#[test]
fn the_check_can_fail() {
    let src = r#"const A: &str = "https://new-provider.invalid2.net/v1";
// "https://commented.out.net"
const B: &str = "http://localhost:1234";"#;
    let hosts = hosts_in(src);
    assert_eq!(hosts, ["new-provider.invalid2.net", "localhost"]);
    assert_eq!(
        undocumented("| `api.openai.com` |", &hosts),
        ["new-provider.invalid2.net"]
    );
    assert!(undocumented("`new-provider.invalid2.net`", &hosts).is_empty());
}

#[test]
fn code_after_a_test_module_is_still_scanned() {
    let src = "#[cfg(test)]\nmod tests {\n    const T: &str = \"https://only-in-tests.invalid2.net\";\n}\n\nconst P: &str = \"https://after-tests.invalid2.net\";\n";
    assert_eq!(hosts_in(src), ["after-tests.invalid2.net"]);
}
