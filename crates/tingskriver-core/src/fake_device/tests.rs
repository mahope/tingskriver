use super::*;

fn event(seconds: u64, event: DeviceEvent) -> TimedEvent {
    TimedEvent {
        at: Duration::from_secs(seconds),
        event,
    }
}

#[test]
fn callbacks_age_and_reset_without_replaying() {
    let mut fake = FakeDevice::new(vec![
        event(0, DeviceEvent::Buffer(Buffer::Sound)),
        event(2, DeviceEvent::Buffer(Buffer::Silence)),
        event(3, DeviceEvent::Buffer(Buffer::Zeros)),
        event(6, DeviceEvent::Buffer(Buffer::Sound)),
    ])
    .unwrap();
    fake.advance_to(Duration::from_secs(4)).unwrap();
    assert_eq!(fake.since_frame(), Some(Duration::from_secs(1)));
    assert_eq!(fake.since_sound(), Some(Duration::from_secs(4)));
    fake.reset();
    fake.advance_to(Duration::from_secs(5)).unwrap();
    assert_eq!(fake.since_frame(), None);
    fake.advance_to(Duration::from_secs(6)).unwrap();
    assert_eq!(fake.since_sound(), Some(Duration::ZERO));
    assert!(fake.advance_to(Duration::from_secs(1)).is_err());
    assert_eq!(fake.since_frame(), Some(Duration::ZERO));
}

#[test]
fn errors_are_delivered_once_and_snapshots_recover() {
    let kind = StreamKind::Microphone;
    let snapshot = DeviceSnapshot {
        default_name: Some("returned".into()),
        present: vec!["returned".into()],
    };
    let mut fake = FakeDevice::new(vec![
        event(0, DeviceEvent::SnapshotError(kind, "unavailable".into())),
        event(1, DeviceEvent::StreamError(kind, "removed".into())),
        event(2, DeviceEvent::Devices(kind, snapshot.clone())),
    ])
    .unwrap();
    fake.advance_to(Duration::ZERO).unwrap();
    assert!(fake.snapshot(kind).is_err());
    assert_eq!(
        fake.advance_to(Duration::from_secs(1)).unwrap(),
        vec![(kind, "removed".into())]
    );
    assert!(fake.advance_to(Duration::from_secs(1)).unwrap().is_empty());
    fake.advance_to(Duration::from_secs(2)).unwrap();
    assert_eq!(fake.snapshot(kind).unwrap(), snapshot);
}

#[test]
fn unordered_scripts_are_rejected() {
    assert!(FakeDevice::new(vec![
        event(2, DeviceEvent::Buffer(Buffer::Sound)),
        event(1, DeviceEvent::Buffer(Buffer::Zeros))
    ])
    .is_err());
}

#[test]
fn three_hours_of_zero_callbacks_use_only_virtual_time() {
    let mut fake = FakeDevice::new(
        (0..=10_800)
            .map(|second| event(second, DeviceEvent::Buffer(Buffer::Zeros)))
            .collect(),
    )
    .unwrap();
    fake.advance_to(Duration::from_secs(10_800)).unwrap();
    assert_eq!(fake.since_frame(), Some(Duration::ZERO));
    assert_eq!(fake.since_sound(), Some(Duration::from_secs(10_800)));
    // Advancing to the same time does not replay callbacks after a reset.
    fake.reset();
    fake.advance_to(Duration::from_secs(10_800)).unwrap();
    assert_eq!(fake.since_frame(), None);
}
