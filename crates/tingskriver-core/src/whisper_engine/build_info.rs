//! Build capabilities, independent of the developer's runtime environment.
pub fn compiled_backend() -> &'static str {
    backend_for(
        cfg!(feature = "cuda"),
        cfg!(feature = "vulkan"),
        cfg!(any(feature = "metal", target_os = "macos")),
        cfg!(feature = "hipblas"),
    )
}

fn backend_for(cuda: bool, vulkan: bool, metal: bool, hipblas: bool) -> &'static str {
    match (cuda, vulkan, metal, hipblas) {
        (true, _, _, _) => "CUDA",
        (_, true, _, _) => "Vulkan",
        (_, _, true, _) => "Metal",
        (_, _, _, true) => "HIP/ROCm",
        _ => "CPU",
    }
}

pub fn os_version() -> String {
    #[cfg(target_os = "windows")]
    {
        format!(
            "{} (build {})",
            sysinfo::System::long_os_version().unwrap_or_else(|| "Windows".into()),
            sysinfo::System::kernel_version().unwrap_or_else(|| "unknown".into()),
        )
    }
    #[cfg(not(target_os = "windows"))]
    std::env::consts::OS.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_backend_matches_enabled_features() {
        let expected = if cfg!(feature = "cuda") {
            "CUDA"
        } else if cfg!(feature = "vulkan") {
            "Vulkan"
        } else if cfg!(any(feature = "metal", target_os = "macos")) {
            "Metal"
        } else if cfg!(feature = "hipblas") {
            "HIP/ROCm"
        } else {
            "CPU"
        };
        assert_eq!(compiled_backend(), expected);
    }

    #[test]
    fn backend_depends_only_on_build_capabilities() {
        assert_eq!(backend_for(false, false, false, false), "CPU");
        assert_eq!(backend_for(true, false, false, false), "CUDA");
        assert_eq!(backend_for(false, true, false, false), "Vulkan");
        assert_eq!(backend_for(false, false, true, false), "Metal");
        assert_eq!(backend_for(false, false, false, true), "HIP/ROCm");
        assert_eq!(backend_for(true, true, true, true), "CUDA");
    }
}
