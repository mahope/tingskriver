//! Optional text hook: a script the user put in the hook folder gets the
//! dictated text on stdin, and what it prints on stdout is inserted instead.
//!
//! Safety rules, all covered by tests:
//! - only a script the user chose (a plain `.cmd` or `.ps1` file name inside
//!   the hook folder) runs, and never through a shell command line: the text
//!   goes to stdin only, so it is never expanded, quoted or put in arguments;
//! - the script has [`TIMEOUT`] to answer and is killed when it does not;
//! - fail-open: any failure (missing script, start error, timeout, exit code
//!   other than 0, empty or non-UTF-8 output) gives the original text back;
//! - the dictated text, the script's output and its stderr are never logged
//!   (stderr is discarded), only the kind of failure and the duration.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// The folder under the app data folder where hook scripts live.
pub const HOOK_DIR: &str = "dictation_hooks";

/// How long a hook may take before it is killed and the original text is used.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// A hook answer longer than this is treated as a failure.
const MAX_OUTPUT: usize = 1024 * 1024;

pub fn hook_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(HOOK_DIR)
}

/// `Ok(path)` when `name` is a plain `.cmd`/`.ps1` file name that is safe to
/// start; the error is a short Danish message for the settings page.
pub fn script_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Vælg et script".to_string());
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    if !matches!(ext.as_deref(), Some("cmd" | "ps1")) {
        return Err("Scriptet skal være en .cmd- eller .ps1-fil".to_string());
    }
    // Letters, digits, space, dot, dash and underscore only: no path
    // separators, no cmd.exe or PowerShell metacharacters, no `..`.
    let plain = |c: char| c.is_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_');
    if !name.chars().all(plain) || name.starts_with('.') || name.contains("..") {
        return Err("Scriptets filnavn må kun have bogstaver, tal, mellemrum, punktum, bindestreg og understreg".to_string());
    }
    Ok(dir.join(name))
}

/// The scripts in the hook folder, for the settings page. A missing folder is
/// an empty list.
pub async fn list_scripts(dir: &Path) -> Vec<String> {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let mut names = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if script_path(dir, &name).is_ok() && entry.file_type().await.is_ok_and(|t| t.is_file()) {
            names.push(name);
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// Program and arguments that start the script at `path`.
fn launcher(path: &Path) -> (String, Vec<String>) {
    let path = path.to_string_lossy().into_owned();
    if path.to_ascii_lowercase().ends_with(".ps1") {
        let args = [
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ];
        let mut args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        args.push(path);
        ("powershell.exe".to_string(), args)
    } else {
        (
            "cmd.exe".to_string(),
            vec!["/D".into(), "/Q".into(), "/C".into(), path],
        )
    }
}

/// Run the hook script `name` from `dir` on `text`. Always returns text to
/// insert: the script's answer, or `text` itself on any failure.
pub async fn apply(dir: &Path, name: &str, text: String) -> String {
    let path = match script_path(dir, name) {
        Ok(path) => path,
        Err(e) => {
            log::warn!("Dictation text hook not run: {e}");
            return text;
        }
    };
    match tokio::fs::metadata(&path).await {
        Ok(m) if m.is_file() => {}
        _ => {
            log::warn!("Dictation text hook not run: the script is missing");
            return text;
        }
    }
    let (program, args) = launcher(&path);
    run(&program, &args, text, TIMEOUT).await
}

/// Start `program`, give it `text` on stdin and return its stdout with one
/// final line break removed; `text` itself on any failure.
async fn run(program: &str, args: &[String], text: String, timeout: Duration) -> String {
    let started = Instant::now();
    match run_inner(program, args, &text, timeout).await {
        Ok(out) => {
            log::info!(
                "Dictation text hook finished in {} ms",
                started.elapsed().as_millis()
            );
            out
        }
        Err(kind) => {
            log::warn!(
                "Dictation text hook failed ({kind}) after {} ms; inserting the original text",
                started.elapsed().as_millis()
            );
            text
        }
    }
}

async fn run_inner(
    program: &str,
    args: &[String],
    text: &str,
    timeout: Duration,
) -> Result<String, &'static str> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command.spawn().map_err(|_| "could not start")?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;

    let exchange = async {
        let input = text.as_bytes().to_vec();
        let write = async move {
            // A script that ignores stdin closes it early; that is fine.
            let _ = stdin.write_all(&input).await;
            drop(stdin);
        };
        let read = async {
            let mut out = Vec::new();
            (&mut stdout)
                .take(MAX_OUTPUT as u64 + 1)
                .read_to_end(&mut out)
                .await
                .map(|_| out)
        };
        let (_, out) = tokio::join!(write, read);
        let out = out.map_err(|_| "could not read the answer")?;
        let status = child
            .wait()
            .await
            .map_err(|_| "could not wait for the script")?;
        Ok::<_, &'static str>((status, out))
    };
    // On timeout the future is dropped, and `kill_on_drop` kills the script.
    let (status, out) = tokio::time::timeout(timeout, exchange)
        .await
        .map_err(|_| "timeout")??;
    if !status.success() {
        return Err("non-zero exit code");
    }
    if out.len() > MAX_OUTPUT {
        return Err("answer too long");
    }
    let mut answer = String::from_utf8(out).map_err(|_| "answer is not UTF-8")?;
    if answer.ends_with('\n') {
        answer.pop();
        if answer.ends_with('\r') {
            answer.pop();
        }
    }
    if answer.trim().is_empty() {
        return Err("empty answer");
    }
    Ok(answer)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> (String, Vec<String>) {
        ("sh".to_string(), vec!["-c".to_string(), script.to_string()])
    }

    async fn run_sh(script: &str, text: &str, timeout: Duration) -> String {
        let (program, args) = sh(script);
        run(&program, &args, text.to_string(), timeout).await
    }

    #[tokio::test]
    async fn stdout_replaces_the_text_and_one_final_line_break_is_removed() {
        assert_eq!(
            run_sh("tr a-z A-Z", "hej verden", TIMEOUT).await,
            "HEJ VERDEN"
        );
        assert_eq!(
            run_sh("printf 'ny tekst\\n\\n'", "x", TIMEOUT).await,
            "ny tekst\n"
        );
        assert_eq!(
            run_sh("printf 'ny tekst\\r\\n'", "x", TIMEOUT).await,
            "ny tekst"
        );
    }

    #[tokio::test]
    async fn text_is_not_expanded_by_a_shell() {
        let text = "$(touch /nonexistent/x) `id` \"; echo hacked; $HOME %PATH%";
        assert_eq!(run_sh("cat", text, TIMEOUT).await, text);
    }

    #[tokio::test]
    async fn every_failure_inserts_the_original_text() {
        let text = "den oprindelige tekst";
        assert_eq!(run_sh("echo andet; exit 3", text, TIMEOUT).await, text);
        assert_eq!(run_sh("true", text, TIMEOUT).await, text);
        assert_eq!(run_sh("printf '\\377\\376'", text, TIMEOUT).await, text);
        assert_eq!(
            run("/findes/ikke", &[], text.to_string(), TIMEOUT).await,
            text
        );
    }

    #[tokio::test]
    async fn a_slow_script_is_killed_and_the_original_text_is_used() {
        let started = Instant::now();
        let out = run_sh(
            "sleep 30; echo for sent",
            "oprindelig",
            Duration::from_millis(200),
        )
        .await;
        assert_eq!(out, "oprindelig");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_script_that_ignores_stdin_still_works() {
        let big = "ord ".repeat(200_000);
        assert_eq!(run_sh("echo klar", &big, TIMEOUT).await, "klar");
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn only_plain_cmd_and_ps1_names_are_accepted() {
        let dir = Path::new("hooks");
        assert!(script_path(dir, "ret tekst.ps1").is_ok());
        assert!(script_path(dir, "Ret-1_a.CMD").is_ok());
        for bad in [
            "",
            "  ",
            "a.exe",
            "a.bat",
            "a",
            "a.cmd.txt",
            "..\\a.cmd",
            "../a.cmd",
            "sub/a.cmd",
            "C:\\a.cmd",
            "a&b.cmd",
            "a%PATH%.cmd",
            "a^b.cmd",
            "a;b.ps1",
            "a`b.ps1",
            "a$b.ps1",
            "a\"b.cmd",
            ".hidden.cmd",
            "a..b.cmd",
        ] {
            assert!(script_path(dir, bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn launchers_pass_the_path_and_never_the_text() {
        let (program, args) = launcher(Path::new("h/a.ps1"));
        assert_eq!(program, "powershell.exe");
        assert_eq!(args.last().map(String::as_str), Some("h/a.ps1"));
        assert!(args.contains(&"-NonInteractive".to_string()));
        let (program, args) = launcher(Path::new("h/a.cmd"));
        assert_eq!(
            (program.as_str(), args.last().map(String::as_str)),
            ("cmd.exe", Some("h/a.cmd"))
        );
    }

    #[tokio::test]
    async fn a_missing_script_gives_the_original_text() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            apply(dir.path(), "findes-ikke.cmd", "tekst".into()).await,
            "tekst"
        );
        assert_eq!(apply(dir.path(), "../x.cmd", "tekst".into()).await, "tekst");
        assert!(list_scripts(&dir.path().join("mangler")).await.is_empty());
    }

    #[tokio::test]
    async fn the_script_list_only_has_runnable_names() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.ps1", "A.cmd", "notes.txt", "x&y.cmd"] {
            tokio::fs::write(dir.path().join(name), "").await.unwrap();
        }
        assert_eq!(
            list_scripts(dir.path()).await,
            vec!["A.cmd".to_string(), "b.ps1".to_string()]
        );
    }
}
