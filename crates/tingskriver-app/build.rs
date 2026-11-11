use std::process::Command;

fn output(program: &str, args: &[&str]) -> Option<String> {
    let result = Command::new(program).args(args).output().ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_string())
}

fn main() {
    for name in ["HEAD", "refs/heads", "packed-refs"] {
        if let Some(path) = output("git", &["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let commit = output("git", &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let epoch = std::env::var("SOURCE_DATE_EPOCH").ok();
    let args = epoch.as_ref().map(|v| format!("@{v}"));
    let date = if let Some(date) = args {
        output("date", &["-u", "-d", &date, "+%Y-%m-%d"])
    } else {
        output("date", &["-u", "+%Y-%m-%d"])
    }
    .or_else(|| {
        output(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "[DateTime]::UtcNow.ToString('yyyy-MM-dd')",
            ],
        )
    })
    .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=TINGSKRIVER_COMMIT={commit}");
    println!("cargo:rustc-env=TINGSKRIVER_BUILD_DATE={date}");
}
