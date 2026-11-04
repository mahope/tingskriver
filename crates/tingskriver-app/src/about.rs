//! Offline, allowlisted information: never read user settings or meeting data.
use tingskriver_core::strings;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMMIT: &str = env!("TINGSKRIVER_COMMIT");
pub const BUILD_DATE: &str = env!("TINGSKRIVER_BUILD_DATE");

pub use crate::update::UpdateStatus;

/// Snapshot only: opening About never starts an update check.
pub fn update_status() -> UpdateStatus {
    match crate::update::status() {
        UpdateStatus::Inactive => UpdateStatus::Unavailable,
        status => status,
    }
}

pub fn metadata() -> String {
    format!(
        "{} {}\n{}: {}\n{}: {}\n{}: {}\n{}: {} / {}",
        strings::APP_NAME,
        VERSION,
        strings::ABOUT_BUILD,
        BUILD_DATE,
        strings::ABOUT_COMMIT,
        COMMIT,
        strings::ABOUT_CHANNEL,
        if VERSION.contains('-') {
            strings::ABOUT_BETA
        } else {
            strings::ABOUT_STABLE
        },
        strings::ABOUT_PLATFORM,
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

pub async fn diagnostics() -> String {
    use tingskriver_core::{
        audio::hardware_detector::HardwareProfile,
        parakeet_engine::commands::parakeet_get_current_model,
        whisper_engine::commands::whisper_get_current_model,
    };
    let whisper = whisper_get_current_model().await.ok().flatten();
    let parakeet = parakeet_get_current_model().await.ok().flatten();
    format!(
        "{}\nWhisper: {}\nParakeet: {}",
        diagnostics_for(Some(HardwareProfile::detect().gpu_type)),
        safe_model(whisper.as_deref()),
        safe_model(parakeet.as_deref())
    )
}

// Return catalogue-owned strings, never the runtime value (which may be a path).
fn safe_model(value: Option<&str>) -> &'static str {
    let Some(value) = value else {
        return strings::ABOUT_MODEL_NOT_LOADED;
    };
    tingskriver_core::config::WHISPER_MODEL_CATALOG
        .iter()
        .map(|entry| entry.0)
        .chain(["parakeet-tdt-0.6b-v3-int8", "parakeet-tdt-0.6b-v2-int8"])
        .find(|known| *known == value)
        .unwrap_or(strings::UNKNOWN)
}

fn diagnostics_for(gpu: Option<tingskriver_core::audio::hardware_detector::GpuType>) -> String {
    use tingskriver_core::audio::hardware_detector::GpuType;
    let backend = match gpu {
        Some(GpuType::None) => "CPU",
        Some(GpuType::Metal) => "Metal",
        Some(GpuType::Cuda) => "CUDA",
        Some(GpuType::Vulkan) => "Vulkan",
        Some(GpuType::OpenCL) => "OpenCL",
        None => strings::UNKNOWN,
    };
    format!("{}\n{}: {}", metadata(), strings::ABOUT_GPU, backend)
}

pub const DOCUMENTS: &[(&str, &str)] = &[
    (strings::ABOUT_LICENSE, include_str!("../../../LICENSE.md")),
    (
        strings::ABOUT_THIRD_PARTY,
        include_str!("../../../THIRD_PARTY_LICENSES.md"),
    ),
    (strings::ABOUT_NOTICE, include_str!("../../../NOTICE.md")),
    (
        strings::ABOUT_PRIVACY,
        include_str!("../../../PRIVACY_POLICY.md"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn displayed_version_matches_workspace_and_installer_source() {
        let manifest = include_str!("../../../Cargo.toml");
        let workspace = manifest.split("[workspace.package]").nth(1).unwrap();
        let version = workspace
            .lines()
            .find_map(|line| line.strip_prefix("version = "))
            .unwrap()
            .trim_matches('"');
        assert_eq!(VERSION, version);
        assert!(metadata().contains(VERSION));
        let release = include_str!("../../../.github/workflows/app-release.yml");
        assert!(release.contains("cargo metadata --no-deps --format-version 1 --locked"));
        let nsis = include_str!("../../../.github/workflows/windows-build.yml");
        assert!(nsis.contains("cargo metadata --no-deps --format-version 1 --locked"));
        assert!(nsis.contains("c.version=process.argv[1]"));
        assert!(release.contains("-Version \"${{ steps.version.outputs.version }}\""));
    }
    #[test]
    fn diagnosis_contains_only_fixed_build_information() {
        assert_eq!(
            diagnostics_for(None),
            format!(
                "{}\n{}: {}",
                metadata(),
                strings::ABOUT_GPU,
                strings::UNKNOWN
            )
        );
        for gpu in [
            tingskriver_core::audio::hardware_detector::GpuType::None,
            tingskriver_core::audio::hardware_detector::GpuType::Cuda,
        ] {
            let report = diagnostics_for(Some(gpu));
            assert!(!report.contains('@'));
            assert!(!report.contains("/home/"));
            assert!(!report.contains("C:\\Users"));
        }
        assert_eq!(safe_model(Some("large-v3")), "large-v3");
        assert_eq!(
            safe_model(Some("parakeet-tdt-0.6b-v3-int8")),
            "parakeet-tdt-0.6b-v3-int8"
        );
        assert_eq!(safe_model(None), strings::ABOUT_MODEL_NOT_LOADED);
        for private in [
            "/home/private/model.bin",
            "C:\\Users\\private\\model.bin",
            "person@example.invalid",
            "sk-secret",
        ] {
            assert_eq!(safe_model(Some(private)), strings::UNKNOWN);
        }
        assert!(DOCUMENTS.iter().all(|(_, body)| !body.is_empty()));
        assert_eq!(
            UpdateStatus::default().label(),
            strings::ABOUT_UPDATES_UNAVAILABLE
        );
    }
}
