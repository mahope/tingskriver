#[path = "../src/audio/callback_queue.rs"]
mod callback_queue;

use callback_queue::{queue, CaptureStamp};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::{Duration, Instant},
};

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNTING.with(|active| {
            if active.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNTING.with(|active| {
            if active.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.realloc(ptr, layout, size)
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocation_check<T: Copy + Default>() {
    let (mut input, mut output) = queue::<T>(4096, 2);
    let data = [T::default(); 2048];
    let mut drained = [T::default(); 4096];
    ALLOCATIONS.with(|count| count.set(0));
    COUNTING.with(|active| active.set(true));
    for _ in 0..1000 {
        let started = Instant::now();
        assert!(input.copy_from(&data, CaptureStamp::default()));
        assert!(input.copy_from(&data, CaptureStamp::default()));
        assert!(!input.copy_from(&data, CaptureStamp::default()));
        #[cfg(debug_assertions)]
        input.record_duration(started.elapsed());
        #[cfg(not(debug_assertions))]
        let _ = started;
        assert!(output.pop_into(&mut drained).is_some());
        assert!(output.pop_into(&mut drained).is_some());
    }
    COUNTING.with(|active| active.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    #[cfg(debug_assertions)]
    eprintln!(
        "{}: queue p99 <= {} us",
        std::any::type_name::<T>(),
        output.callback_p99_us()
    );
}

#[test]
#[ignore = "synthetic before/after timing; run on an idle machine with --nocapture"]
fn synchronous_dsp_versus_callback_queue() {
    use std::sync::Arc;
    use tingskriver_core::audio::{
        devices::{AudioDevice, DeviceType as HardwareType},
        pipeline::AudioCapture,
        recording_state::{DeviceType, RecordingState},
    };
    let state = RecordingState::new();
    state.start_recording().unwrap();
    let capture = AudioCapture::new(
        Arc::new(AudioDevice::new(
            "Fake microphone".into(),
            HardwareType::Input,
        )),
        state,
        48000,
        2,
        DeviceType::Microphone,
        None,
    );
    let data = [0.01f32; 960];
    let mut before = Vec::with_capacity(2000);
    ALLOCATIONS.with(|count| count.set(0));
    COUNTING.with(|active| active.set(true));
    for _ in 0..2000 {
        let start = Instant::now();
        capture.process_audio_data(&data);
        before.push(start.elapsed().as_nanos());
    }
    COUNTING.with(|active| active.set(false));
    let before_allocations = ALLOCATIONS.with(Cell::get);
    let (mut input, mut output) = queue::<f32>(48000, 2);
    let mut drained = [0.0; 48000];
    let mut after = Vec::with_capacity(2000);
    ALLOCATIONS.with(|count| count.set(0));
    COUNTING.with(|active| active.set(true));
    for _ in 0..2000 {
        let start = Instant::now();
        assert!(input.copy_from(&data, CaptureStamp::default()));
        after.push(start.elapsed().as_nanos());
        output.pop_into(&mut drained).unwrap();
    }
    COUNTING.with(|active| active.set(false));
    let after_allocations = ALLOCATIONS.with(Cell::get);
    before.sort_unstable();
    after.sort_unstable();
    eprintln!("synthetic 48 kHz stereo, 480 frames: synchronous DSP p99={} ns, allocations={}; ring copy p99={} ns, allocations={}", before[1979], before_allocations, after[1979], after_allocations);
    assert!(before_allocations > 0);
    assert_eq!(after_allocations, 0);
}

#[test]
fn all_supported_callback_formats_allocate_nothing_even_on_overflow() {
    allocation_check::<f32>();
    allocation_check::<i8>();
    allocation_check::<i16>();
    allocation_check::<i32>();
}

#[test]
fn backlog_preserves_capture_time_and_mute_instead_of_worker_time() {
    let (mut input, mut output) = queue::<f32>(8, 2);
    input.copy_from(
        &[1.0, 2.0],
        CaptureStamp {
            position: Duration::from_secs(3),
            muted: true,
        },
    );
    input.copy_from(
        &[3.0, 4.0],
        CaptureStamp {
            position: Duration::from_secs(10),
            muted: false,
        },
    );
    let mut data = [0.0; 8];
    let (len, stamp) = output.pop_into(&mut data).unwrap();
    assert_eq!(len, 2);
    assert_eq!(&data[..len], &[1.0, 2.0]);
    assert_eq!(stamp.position, Duration::from_secs(3));
    assert!(stamp.muted);
    let (_, stamp) = output.pop_into(&mut data).unwrap();
    assert_eq!(stamp.position, Duration::from_secs(10));
    assert!(!stamp.muted);
}

#[test]
fn concurrent_consumer_drains_every_published_buffer_in_order() {
    let (mut input, mut output) = queue::<u32>(64, 2);
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut data = [0; 64];
        for expected in 0..10000 {
            loop {
                if let Some((len, _)) = output.pop_into(&mut data) {
                    assert_eq!(len, 2);
                    assert_eq!(&data[..2], &[expected; 2]);
                    break;
                }
                std::thread::yield_now();
                assert!(Instant::now() < deadline, "consumer stalled");
            }
        }
        assert!(output.pop_into(&mut data).is_none());
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    for n in 0..10000 {
        while !input.copy_from(&[n; 2], CaptureStamp::default()) {
            std::thread::yield_now();
            assert!(Instant::now() < deadline, "producer stalled");
        }
    }
    worker.join().unwrap();
}

#[test]
#[cfg(debug_assertions)]
fn histogram_rounds_up_and_uses_the_99th_percentile() {
    let (input, output) = queue::<f32>(8, 1);
    assert_eq!(output.callback_count(), 0);
    assert_eq!(output.callback_p99_us(), 0);
    for _ in 0..99 {
        input.record_duration(Duration::from_nanos(1001));
    }
    input.record_duration(Duration::from_millis(5));
    assert_eq!(output.callback_count(), 100);
    assert_eq!(output.callback_p99_us(), 2);
    input.record_duration(Duration::from_millis(5));
    assert_eq!(output.callback_p99_us(), 8192);
}

#[test]
fn callback_preserves_frames_and_drops_whole_buffers_when_full() {
    let (mut input, mut output) = queue::<i16>(8, 2);
    assert!(input.copy_from(&[1, 2, 3, 4], CaptureStamp::default()));
    assert!(!input.copy_from(&[5, 6, 7, 8, 9, 10], CaptureStamp::default()));
    let mut samples = [0; 8];
    assert_eq!(output.pop_into(&mut samples).unwrap().0, 4);
    assert_eq!(&samples[..4], &[1, 2, 3, 4]);
    assert_eq!(output.dropped_samples(), 6);
}

#[test]
fn partial_frames_are_never_queued() {
    let (mut input, mut output) = queue::<f32>(8, 2);
    assert!(!input.copy_from(&[1.0, 2.0, 3.0], CaptureStamp::default()));
    assert!(output.pop_into(&mut [0.0; 8]).is_none());
}
