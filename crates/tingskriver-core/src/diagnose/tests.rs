use super::*;
struct Fake(State);
impl SystemProbe for Fake {
    fn check(&self, _: Check) -> State {
        self.0
    }
}
#[test]
fn every_check_handles_all_states_with_danish_actions() {
    for state in [
        State::Ok,
        State::Missing,
        State::Unknown,
        State::NotApplicable,
    ] {
        let report = Report::collect(&Fake(state));
        assert_eq!(
            report.findings.len(),
            if state == State::NotApplicable {
                0
            } else {
                CHECKS.len()
            }
        );
        for finding in report.findings {
            assert!(!finding.title.is_empty());
            assert!(!finding.explanation.is_empty());
            if state == State::Missing {
                assert_ne!(finding.action, Action::None);
            }
            assert_eq!(finding.severity == Severity::Error, state == State::Missing);
        }
    }
}
#[test]
fn backend_matrix_and_simulations() {
    struct Pair(State, State);
    impl SystemProbe for Pair {
        fn check(&self, c: Check) -> State {
            if c == Check::Avx2 {
                self.0
            } else {
                self.1
            }
        }
    }
    for a in [
        State::Ok,
        State::Missing,
        State::Unknown,
        State::NotApplicable,
    ] {
        for v in [
            State::Ok,
            State::Missing,
            State::Unknown,
            State::NotApplicable,
        ] {
            assert_eq!(
                matches!(recommended_backend(&Pair(a, v)), Backend::Cpu { .. }),
                a != State::Ok || v != State::Ok
            );
        }
    }
    for avx in [None, Some("0"), Some("1")] {
        for vulkan in [None, Some("0"), Some("1")] {
            let probe = SimulatedProbe::new(Fake(State::Ok), avx, vulkan);
            assert_eq!(
                matches!(recommended_backend(&probe), Backend::Cpu { .. }),
                avx == Some("1") || vulkan == Some("1")
            );
            let decision = recommended_backend(&probe);
            let context = crate::whisper_engine::whisper_context_acceleration_for(
                crate::whisper_engine::WhisperCompiledBackend::Vulkan,
                crate::audio::GpuType::Vulkan,
                crate::audio::PerformanceTier::High,
            )
            .with_diagnose_policy(matches!(decision, Backend::Cpu { .. }));
            assert_eq!(context.use_gpu, avx != Some("1") && vulkan != Some("1"));
            assert!(!context.flash_attn);
            assert_eq!(probe.check(Check::Microphone), State::Ok);
        }
    }
}
#[test]
fn export_cannot_capture_neighboring_sensitive_content() {
    struct Surroundings {
        transcript: &'static str,
        audio: &'static [u8],
        path: &'static str,
    }
    let private = Surroundings {
        transcript: "HEMMELIG sætning fra mødet",
        audio: b"PRIVATE_AUDIO_9281",
        path: "C:\\Users\\SensitiveUser9281\\recording.wav",
    };
    let report = Report::collect(&Fake(State::Missing));
    for export in [report.to_text(), report.to_json().unwrap()] {
        for secret in [
            private.transcript,
            std::str::from_utf8(private.audio).unwrap(),
            private.path,
            "SensitiveUser9281",
        ] {
            assert!(!export.contains(secret));
        }
    }
}
