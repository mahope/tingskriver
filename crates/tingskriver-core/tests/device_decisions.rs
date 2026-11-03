//! The same temporal scenarios exercise production decisions and a silence mutant.
use std::time::{Duration, Instant};
use tingskriver_core::audio::device_follow::*;
use tingskriver_core::fake_device::*;

const MIC: StreamKind = StreamKind::Microphone;
const A: &str = "Headset";
const B: &str = "Laptop";

struct Step {
    second: u64,
    events: Vec<DeviceEvent>,
    expected: Option<(&'static str, SwitchReason)>,
    fail_swap: bool,
    backoff_ms: Option<u64>,
    lost: bool,
    retry: bool,
}
struct Scenario {
    name: &'static str,
    steps: Vec<Step>,
}
fn step(
    second: u64,
    events: Vec<DeviceEvent>,
    expected: Option<(&'static str, SwitchReason)>,
) -> Step {
    Step {
        second,
        events,
        expected,
        fail_swap: false,
        backoff_ms: None,
        lost: false,
        retry: false,
    }
}
fn devices(default: Option<&str>, present: &[&str]) -> DeviceEvent {
    DeviceEvent::Devices(
        MIC,
        DeviceSnapshot {
            default_name: default.map(str::to_owned),
            present: present.iter().map(|s| (*s).to_owned()).collect(),
        },
    )
}
fn buffer(buffer: Buffer) -> DeviceEvent {
    DeviceEvent::Buffer(buffer)
}
fn error() -> DeviceEvent {
    DeviceEvent::StreamError(MIC, A.into())
}
fn scenarios() -> Vec<Scenario> {
    let mut failed = step(
        1,
        vec![devices(Some(B), &[A, B])],
        Some((B, SwitchReason::DefaultChanged)),
    );
    failed.fail_swap = true;
    failed.backoff_ms = Some(500);
    let mut failed_again = step(2, vec![], Some((B, SwitchReason::StreamMissing)));
    failed_again.fail_swap = true;
    failed_again.backoff_ms = Some(1000);
    let mut lost = step(1, vec![devices(None, &[])], None);
    lost.lost = true;
    vec![
        Scenario {
            name: "enumeration failure during mute then recovery",
            steps: vec![
                Step {
                    retry: true,
                    ..step(
                        1,
                        vec![
                            DeviceEvent::SnapshotError(MIC, "temporarily unavailable".into()),
                            buffer(Buffer::Zeros),
                        ],
                        None,
                    )
                },
                step(
                    2,
                    vec![devices(Some(A), &[A, B]), buffer(Buffer::Sound)],
                    None,
                ),
            ],
        },
        Scenario {
            name: "mute for sixty seconds",
            steps: (1..=60)
                .map(|s| step(s, vec![buffer(Buffer::Silence)], None))
                .collect(),
        },
        Scenario {
            name: "zero-valued frames from live endpoint",
            steps: (1..=60)
                .map(|s| step(s, vec![buffer(Buffer::Zeros)], None))
                .collect(),
        },
        Scenario {
            name: "brief silence then sound",
            steps: vec![
                step(1, vec![buffer(Buffer::Silence)], None),
                step(2, vec![buffer(Buffer::Sound)], None),
            ],
        },
        Scenario {
            name: "repeated mute and unmute",
            steps: (1..=12)
                .map(|s| {
                    step(
                        s,
                        vec![buffer(if s % 3 == 0 {
                            Buffer::Sound
                        } else {
                            Buffer::Zeros
                        })],
                        None,
                    )
                })
                .collect(),
        },
        Scenario {
            name: "endpoint removed",
            steps: vec![step(
                1,
                vec![devices(Some(B), &[B])],
                Some((B, SwitchReason::ActiveDisappeared)),
            )],
        },
        Scenario {
            name: "follow Windows default",
            steps: vec![step(
                1,
                vec![devices(Some(B), &[A, B]), buffer(Buffer::Sound)],
                Some((B, SwitchReason::DefaultChanged)),
            )],
        },
        Scenario {
            name: "default returns to device just left",
            steps: vec![
                step(
                    1,
                    vec![devices(Some(B), &[A, B])],
                    Some((B, SwitchReason::DefaultChanged)),
                ),
                step(
                    2,
                    vec![devices(Some(A), &[A, B])],
                    Some((A, SwitchReason::DefaultChanged)),
                ),
            ],
        },
        Scenario {
            name: "missing buffers at watchdog threshold",
            steps: vec![
                step(1, vec![], None),
                step(2, vec![], Some((B, SwitchReason::NoFrames))),
            ],
        },
        Scenario {
            name: "disappears during mute",
            steps: vec![
                step(1, vec![buffer(Buffer::Zeros)], None),
                step(
                    2,
                    vec![buffer(Buffer::Zeros), devices(Some(B), &[B])],
                    Some((B, SwitchReason::ActiveDisappeared)),
                ),
            ],
        },
        Scenario {
            name: "failed swaps back off then recover",
            steps: vec![
                failed,
                failed_again,
                step(3, vec![], Some((B, SwitchReason::StreamMissing))),
                step(4, vec![buffer(Buffer::Sound)], None),
            ],
        },
        Scenario {
            name: "loopback is excluded from microphone fallback",
            steps: vec![step(
                1,
                vec![devices(Some(A), &["Stereo Mix", A, B]), error()],
                Some((B, SwitchReason::StreamError)),
            )],
        },
        Scenario {
            name: "stream error while endpoint stays listed",
            steps: vec![step(1, vec![error()], Some((B, SwitchReason::StreamError)))],
        },
        Scenario {
            name: "all devices disappear then return",
            steps: vec![
                lost,
                step(
                    2,
                    vec![devices(Some(A), &[A])],
                    Some((A, SwitchReason::StreamMissing)),
                ),
            ],
        },
    ]
}

fn run(scenario: &Scenario, content_based_mutant: bool) -> Vec<String> {
    let mut script = vec![
        TimedEvent {
            at: Duration::ZERO,
            event: devices(Some(A), &[A, B]),
        },
        TimedEvent {
            at: Duration::ZERO,
            event: buffer(Buffer::Sound),
        },
    ];
    for step in &scenario.steps {
        script.extend(step.events.iter().cloned().map(|event| TimedEvent {
            at: Duration::from_secs(step.second),
            event,
        }));
    }
    let mut fake = FakeDevice::new(script).unwrap();
    fake.advance_to(Duration::ZERO).unwrap();
    let origin = Instant::now(); // Only an anchor; all elapsed time below is virtual.
    let mut watchdog = MicWatchdog::new(origin);
    let mut follower = DeviceFollower::new(true, None, Some(A.into()), false, None, None);
    let mut mismatches = Vec::new();
    for step in &scenario.steps {
        let elapsed = Duration::from_secs(step.second);
        for (kind, device) in fake.advance_to(elapsed).unwrap() {
            follower.stream_failed(kind, &device, StreamFailure::Error);
        }
        watchdog.check(&mut follower, &fake, origin + elapsed);
        // Deliberately resurrect the old bug: sound content determines failure,
        // even though live silent callbacks continue to arrive.
        if content_based_mutant
            && fake
                .since_sound()
                .is_some_and(|age| age >= Duration::from_secs(5))
        {
            if let Some(active) = follower.active(MIC).map(str::to_owned) {
                follower.stream_failed(MIC, &active, StreamFailure::NoFrames);
            }
        }
        let eval = follower.evaluate(&fake);
        let expected: Vec<_> = step
            .expected
            .as_ref()
            .map(|(to, reason)| SwitchAction {
                kind: MIC,
                from: follower.active(MIC).map(str::to_owned),
                to: (*to).into(),
                reason: reason.clone(),
            })
            .into_iter()
            .collect();
        if eval.switches != expected
            || !eval.lost.is_empty() != step.lost
            || eval.retry != (step.lost || step.retry)
        {
            mismatches.push(format!(
                "{} at {}s: expected {expected:?}, lost={}, got {eval:?}",
                scenario.name, step.second, step.lost
            ));
        }
        for (kind, _) in eval.lost {
            follower.stream_lost(kind);
        }
        for action in eval.switches {
            if step.fail_swap {
                let delay = follower.swap_failed(action.kind);
                assert_eq!(Some(delay.as_millis() as u64), step.backoff_ms);
            } else {
                watchdog.swap_succeeded(
                    &mut follower,
                    action.kind,
                    &action.to,
                    &fake,
                    origin + elapsed,
                );
                assert_eq!(follower.failures(action.kind), 0);
            }
        }
    }
    mismatches
}

#[test]
fn temporal_decision_table_preserves_mute_and_device_recovery() {
    let table = scenarios();
    assert!(table.len() >= 10);
    let failures: Vec<_> = table
        .iter()
        .flat_map(|scenario| run(scenario, false))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn same_table_rejects_content_based_silence_failover() {
    let caught: Vec<_> = scenarios()
        .iter()
        .filter(|scenario| !run(scenario, true).is_empty())
        .map(|s| s.name)
        .collect();
    assert!(
        !caught.is_empty(),
        "decision table no longer detects the mute regression"
    );
    assert!(caught.contains(&"mute for sixty seconds"));
    assert!(caught.contains(&"zero-valued frames from live endpoint"));
    println!("Content-based mutant rejected by: {caught:?}");
}
