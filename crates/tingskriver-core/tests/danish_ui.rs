//! Source-level guardrails, intentionally independent of the Windows UI build.
use std::fs;
use std::path::Path;

use regex::Regex;
use tingskriver_core::strings;

// Exact path + literal exemptions: technical strings and ownership-limited baseline.
const ALLOW: &[(&str, &str)] = &[
    ("ui.rs", "database task stopped: {error}"), // diagnostic fed into DbFailure
    ("ui.rs", "page-{title}"),                   // GPUI element identity
    (
        "startup.rs",
        "startup: first frame {} ms after process start ({} ms before main, budget {} ms)",
    ), // log report
    ("single_instance.rs", "{}.gpui-{}"),        // IPC endpoint
    ("single_instance.rs", "user"),              // fallback endpoint component
    ("single_instance.rs", r"\\.\pipe\{}"),      // Windows pipe path
    ("single_instance.rs", "{}.sock"),           // Unix socket path
    // Existing copy outside issue #65's file ownership. Migrate with those screens.
    ("probe.rs", "◌{c} U+{:04X} (kombinerende tegn)"),
    ("probe.rs", "{c} U+{:04X}"),
    ("crash.rs", "Detaljerne står i logfilen:\\n{}"),
    (
        "crash.rs",
        r#"{what}\n\n{log}\n\nVælg \"Prøv igen\" for at starte Tingskriver igen."#,
    ),
    ("update.rs", "Henter version {version} …"),
    (
        "update.rs",
        "Version {version} er klar og installeres, næste gang Tingskriver starter.",
    ),
];

fn forbidden(text: &str) -> bool {
    text.to_lowercase().contains("meetily")
}

/// Lexical heuristic: inspect literals inside known UI calls and format!.
/// Does not resolve variables, aliases, custom wrappers or macro expansions.
fn violations(source: &str, file: &str) -> Vec<String> {
    let lexer = Regex::new(r##"(?s)//[^\n]*|/\*.*?\*/|r\#*".*?"\#*|"(?:\\.|[^"\\])*"|[A-Za-z_][A-Za-z_0-9]*|::|[^\s]"##).unwrap();
    let tokens: Vec<_> = lexer
        .find_iter(source)
        .filter(|m| !m.as_str().starts_with("//") && !m.as_str().starts_with("/*"))
        .collect();
    let mut stack: Vec<&str> = Vec::new();
    let mut found = Vec::new();
    let mut skip_until = 0;
    let escapes = Regex::new(r"\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)").unwrap();
    for (i, token) in tokens.iter().enumerate() {
        if i < skip_until {
            continue;
        }
        let cfg_test = ["#", "[", "cfg", "(", "test", ")", "]"];
        if tokens[i..]
            .iter()
            .take(cfg_test.len())
            .map(|t| t.as_str())
            .eq(cfg_test)
        {
            // Skip just this test item, not production code following it.
            let mut depth = 0;
            for (j, next) in tokens.iter().enumerate().skip(i + cfg_test.len()) {
                match next.as_str() {
                    "{" => depth += 1,
                    "}" if depth == 1 => {
                        skip_until = j + 1;
                        break;
                    }
                    "}" => depth -= 1,
                    ";" if depth == 0 => {
                        skip_until = j + 1;
                        break;
                    }
                    _ => {}
                }
            }
            continue;
        }
        let text = token.as_str();
        if text == "(" {
            let previous = i.checked_sub(1).map(|j| tokens[j].as_str()).unwrap_or("");
            let name = if previous == "!" {
                i.checked_sub(2).map(|j| tokens[j].as_str()).unwrap_or("")
            } else {
                previous
            };
            stack.push(name);
        } else if text == ")" {
            stack.pop();
        } else if text.starts_with('"') || text.starts_with("r#") || text.starts_with("r\"") {
            let literal = text
                .trim_start_matches('r')
                .trim_matches('#')
                .trim_matches('"');
            let visible = stack.last().is_some_and(|name| {
                matches!(
                    *name,
                    "child"
                        | "label"
                        | "title"
                        | "placeholder"
                        | "tooltip"
                        | "format"
                        | "heading"
                        | "paragraph"
                        | "muted"
                        | "section_title"
                        | "empty_state"
                        | "setting_group"
                        | "loading"
                        | "page"
                )
            }) || stack.last().is_some_and(|name| {
                *name == "new"
                    && i >= 4
                    && tokens[..i]
                        .iter()
                        .rev()
                        .take(4)
                        .any(|t| t.as_str() == "Label")
            }) || (stack.last() == Some(&"from")
                && tokens[..i]
                    .iter()
                    .rev()
                    .take(4)
                    .any(|t| t.as_str() == "SharedString"));
            let exempt = stack.iter().any(|name| {
                matches!(
                    *name,
                    "env" | "option_env" | "debug" | "info" | "warn" | "error" | "trace" | "log"
                )
            }) || ALLOW.contains(&(file, literal));
            let letters = if text.starts_with('r') {
                literal.to_string()
            } else {
                escapes.replace_all(literal, "").into_owned()
            };
            if visible && !exempt && letters.chars().any(char::is_alphabetic) {
                let line = source[..token.start()]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count()
                    + 1;
                found.push(format!("{file}:{line}: raw text in UI call/format!: move text to tingskriver_core::strings and use a catalogue constant/function (technical exceptions: tests/danish_ui.rs ALLOW)"));
            }
        }
    }
    found
}

fn walk(dir: &Path, visit: &mut impl FnMut(&Path)) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, visit);
        } else if path.extension().is_some_and(|e| e == "rs") {
            visit(&path);
        }
    }
}

#[test]
fn catalogue_has_no_legacy_brand() {
    for text in strings::ALL {
        assert!(!forbidden(text), "Forbidden brand in catalogue: {text}");
    }
}

#[test]
fn forbidden_brand_positive_control() {
    assert!(forbidden("Åbn mEeTiLy nu"));
    assert!(!forbidden("Tingskriver"));
}

#[test]
fn native_sources_have_no_legacy_brand() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for name in ["tingskriver-app", "tingskriver-overlay"] {
        walk(&root.join(name).join("src"), &mut |path| {
            let source = fs::read_to_string(path).unwrap();
            assert!(!forbidden(&source), "Forbidden brand in {}", path.display());
        });
    }
}

#[test]
fn raw_ui_string_positive_control() {
    for source in [
        r#"div().child("Hello")"#,
        r#"x.label( "Hello")"#,
        r#"Label::new("Hello")"#,
        r#"SharedString::from("Hello")"#,
        r#"format!("Hello {}", name)"#,
        "x.placeholder(\n r#\"Hello\"#)",
    ] {
        assert!(
            !violations(source, "fixture.rs").is_empty(),
            "missed {source}"
        );
    }
    for source in [
        "div().child(strings::MEETINGS)",
        "x.label(strings::version(version))",
        r#"log::info!("Hello")"#,
        r#"x.child(env!("VERSION"))"#,
        r#"// x.child("Hello")"#,
        "#[cfg(test)] mod tests { fn x() { div().child(\"Hello\"); } }",
    ] {
        assert!(
            violations(source, "fixture.rs").is_empty(),
            "false positive: {source}"
        );
    }
    assert_eq!(violations("#[cfg(test)] mod tests { fn x() { div().child(\"Test\"); } } fn production() { div().child(\"Hello\"); }", "fixture.rs").len(), 1);
}

#[test]
fn app_has_no_raw_ui_strings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tingskriver-app/src");
    let mut found = Vec::new();
    walk(&root, &mut |path| {
        let relative = path.strip_prefix(&root).unwrap().to_str().unwrap();
        found.extend(violations(&fs::read_to_string(path).unwrap(), relative));
    });
    assert!(found.is_empty(), "{}", found.join("\n"));
}
