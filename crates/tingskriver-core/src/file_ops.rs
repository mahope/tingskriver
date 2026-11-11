use std::{fs, io, path::Path};

/// Rename a file or folder, retrying briefly. On Windows a virus scanner or the
/// indexer can hold a freshly written file for a moment, which makes the
/// rename fail with "access denied" although it succeeds a moment later.
#[allow(clippy::disallowed_methods)] // Synchronous disk worker.
pub(crate) fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    retry_rename(from, || fs::rename(from, to))
}

#[allow(clippy::disallowed_methods)] // Synchronous disk worker.
fn retry_rename(from: &Path, mut rename: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    const ATTEMPTS: u32 = 10;
    let mut attempt = 1;
    loop {
        match rename() {
            Ok(()) => return Ok(()),
            Err(e) if attempt >= ATTEMPTS => return Err(e),
            Err(e) => {
                log::debug!("Rename {} failed (attempt {attempt}): {e}", from.display());
                std::thread::sleep(std::time::Duration::from_millis(100 * u64::from(attempt)));
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::disallowed_methods)] // Synchronous test of the disk worker.
    fn model_activation_retries_transient_rename_errors() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("model.part");
        let to = dir.path().join("model.bin");
        fs::write(&from, b"verified").unwrap();
        let mut attempts = 0;
        retry_rename(&from, || {
            attempts += 1;
            if attempts < 3 {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                fs::rename(&from, &to)
            }
        })
        .unwrap();
        assert_eq!(attempts, 3);
        assert_eq!(fs::read(&to).unwrap(), b"verified");
        assert!(!from.exists());
    }
}
