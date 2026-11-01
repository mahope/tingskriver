//! Content-free environment diagnostics. Probes never perform repair actions.
use serde::Serialize;
#[cfg(any(windows, test))]
mod microphone;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum State {
    Ok,
    Missing,
    Unknown,
    NotApplicable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Check {
    Avx2,
    Vulkan,
    Microphone,
    SystemAudio,
    Shortcut,
    Clipboard,
    ElevatedWindow,
}
pub const CHECKS: [Check; 7] = [
    Check::Avx2,
    Check::Vulkan,
    Check::Microphone,
    Check::SystemAudio,
    Check::Shortcut,
    Check::Clipboard,
    Check::ElevatedWindow,
];
/// All system access is behind this interface; values contain no device names or content.
pub trait SystemProbe {
    fn check(&self, check: Check) -> State;
    fn os_version(&self) -> Option<(u32, u32, u32)> {
        None
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Action {
    OpenSettings(&'static str),
    InstallVulkanRuntime,
    UseCpuPath,
    RunAsSameUser,
    ChooseOtherShortcut,
    CloseClipboardOwner,
    None,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Backend {
    Gpu,
    Cpu { reason: &'static str },
}
pub fn recommended_backend(probe: &impl SystemProbe) -> Backend {
    if probe.check(Check::Avx2) != State::Ok {
        Backend::Cpu {
            reason: "avx2_unavailable",
        }
    } else if probe.check(Check::Vulkan) != State::Ok {
        Backend::Cpu {
            reason: "vulkan_unavailable",
        }
    } else {
        Backend::Gpu
    }
}
pub struct SimulatedProbe<P> {
    inner: P,
    no_avx2: bool,
    no_vulkan: bool,
}
impl<P> SimulatedProbe<P> {
    pub fn new(inner: P, avx2: Option<&str>, vulkan: Option<&str>) -> Self {
        Self {
            inner,
            no_avx2: avx2 == Some("1"),
            no_vulkan: vulkan == Some("1"),
        }
    }
}
impl<P: SystemProbe> SystemProbe for SimulatedProbe<P> {
    fn check(&self, check: Check) -> State {
        if (check == Check::Avx2 && self.no_avx2) || (check == Check::Vulkan && self.no_vulkan) {
            State::Missing
        } else {
            self.inner.check(check)
        }
    }
    fn os_version(&self) -> Option<(u32, u32, u32)> {
        self.inner.os_version()
    }
}
#[derive(Default)]
pub struct NativeProbe {
    /// Windows modifier flags and virtual key. None means the app has not supplied its binding.
    pub shortcut: Option<(u32, u32)>,
}
#[cfg(not(windows))]
impl SystemProbe for NativeProbe {
    fn check(&self, _: Check) -> State {
        State::NotApplicable
    }
}
pub fn system_probe() -> SimulatedProbe<NativeProbe> {
    SimulatedProbe::new(
        NativeProbe::default(),
        std::env::var("TINGSKRIVER_SIMULATE_NO_AVX2")
            .ok()
            .as_deref(),
        std::env::var("TINGSKRIVER_SIMULATE_NO_VULKAN")
            .ok()
            .as_deref(),
    )
}
/// Only Windows uses this Vulkan policy; other platforms keep their existing backend.
pub fn cpu_required() -> bool {
    let probe = system_probe();
    (cfg!(windows) || probe.no_avx2 || probe.no_vulkan)
        && matches!(recommended_backend(&probe), Backend::Cpu { .. })
}
#[derive(Debug, Serialize)]
pub struct Finding {
    pub id: &'static str,
    pub state: State,
    pub severity: Severity,
    pub title: &'static str,
    pub explanation: &'static str,
    pub action: Action,
}
fn describe(check: Check) -> (&'static str, &'static str, &'static str, Action) {
    match check {
        Check::Avx2 => ("cpu.avx2", "CPU-understøttelse", "AVX2 mangler. Lokal talegenkendelse kan kræve en anden CPU-version af programmet.", Action::UseCpuPath),
        Check::Vulkan => ("runtime.vulkan", "Grafikunderstøttelse", "Vulkan kan ikke indlæses. Talegenkendelse bruger CPU og kan være langsommere.", Action::InstallVulkanRuntime),
        Check::Microphone => ("permission.microphone", "Mikrofontilladelse", "Windows blokerer mikrofonen. Programmet kan derfor ikke høre dig.", Action::OpenSettings("ms-settings:privacy-microphone")),
        Check::SystemAudio => ("audio.output", "Systemlyd", "Ingen afspilningsenhed er tilgængelig. Mødets lyd kan derfor ikke optages.", Action::OpenSettings("ms-settings:sound")),
        Check::Shortcut => ("shortcut.conflict", "Genvej", "Genvejen kan ikke reserveres. Et andet program kan allerede bruge den.", Action::ChooseOtherShortcut),
        Check::Clipboard => ("clipboard.locked", "Udklipsholder", "Udklipsholderen er optaget. Indsættelse af diktering kan blive forsinket.", Action::CloseClipboardOwner),
        Check::ElevatedWindow => ("window.integrity", "Forhøjet vindue", "Det aktive program har højere rettigheder. Windows kan blokere indsættelse af diktering.", Action::RunAsSameUser),
    }
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: chrono::DateTime<chrono::Utc>,
    pub os: &'static str,
    pub os_version: Option<(u32, u32, u32)>,
    pub avx2: State,
    pub recommended_backend: Backend,
    pub selected_backend: Backend,
    pub findings: Vec<Finding>,
}
impl Report {
    pub fn collect(probe: &impl SystemProbe) -> Self {
        let started_at = chrono::Utc::now();
        // Snapshot once: transient checks must not change halfway through a report.
        struct Snapshot([State; 7]);
        impl SystemProbe for Snapshot {
            fn check(&self, c: Check) -> State {
                self.0[c as usize]
            }
        }
        let snapshot = Snapshot(CHECKS.map(|c| probe.check(c)));
        let findings = CHECKS.into_iter().filter_map(|c| {
            let state = snapshot.check(c);
            if state == State::NotApplicable { return None; }
            let (id, title, explanation, action) = describe(c);
            let (severity, explanation, action) = match state {
                State::Missing => (Severity::Error, explanation, action),
                State::Unknown => (Severity::Warning, "Kontrollen kunne ikke afgøre tilstanden. Undersøg indstillingen manuelt, hvis funktionen ikke virker.", action),
                _ => (Severity::Info, "Kontrollen fandt ingen problemer.", Action::None),
            };
            Some(Finding {id, state, severity, title, explanation, action})
        }).collect();
        Self {
            started_at,
            finished_at: chrono::Utc::now(),
            os: std::env::consts::OS,
            os_version: probe.os_version(),
            avx2: snapshot.check(Check::Avx2),
            recommended_backend: recommended_backend(&snapshot),
            selected_backend: if crate::whisper_engine::WhisperCompiledBackend::current()
                == crate::whisper_engine::WhisperCompiledBackend::Cpu
            {
                Backend::Cpu {
                    reason: "compiled_cpu",
                }
            } else if cfg!(windows)
                || snapshot.check(Check::Avx2) == State::Missing
                || snapshot.check(Check::Vulkan) == State::Missing
            {
                recommended_backend(&snapshot)
            } else {
                Backend::Gpu
            },
            findings,
        }
    }
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
    pub fn to_text(&self) -> String {
        let mut text = format!(
            "Selvdiagnose {} til {}\nOS: {} {:?}\nAVX2: {:?}\nValgt backend: {:?}\n",
            self.started_at,
            self.finished_at,
            self.os,
            self.os_version,
            self.avx2,
            self.selected_backend
        );
        for f in &self.findings {
            text.push_str(&format!(
                "{}: {} ({:?})\n{}\nHandling: {:?}\n",
                f.id, f.title, f.severity, f.explanation, f.action
            ));
        }
        text
    }
}
