# Audio callback and error budgets

CPAL's sample callback copies native samples into a preallocated single-producer,
single-consumer `ringbuf`. It also snapshots the recording clock and dictation
mute flag using atomics. It does not allocate, log, acquire mutexes, convert
samples or run DSP. Integer conversion, mono conversion, resampling, microphone
filters and the hand-off to the disk writer run on one `audio-dsp` thread per
device. CPAL errors use a separate bounded ring consumed by the same worker.

The sample ring holds half a second at the device's sample rate and channel
count. A full ring drops the entire incoming buffer, preserving channel/frame
alignment; a metadata ring bounds the number of pending callbacks to 4096.
Capture timestamps survive the queue, so gaps remain gaps on the disk writer's
shared timeline. Queued audio keeps its original pause and dictation state.
The number of dropped native samples is logged when the stream stops.

Normal stop drops CPAL first, then drains and joins the DSP worker and flushes
the resampler tail, before closing the writer. A DSP panic stops the recording
through the error path instead of silently abandoning the consumer.

Recoverable errors stop recording at 10 errors within the preceding 60 seconds,
rather than 10 errors over the entire meeting. The total-error limit of 15 uses
the same rolling window. Nonrecoverable errors still stop immediately. An
internal stop runs the normal teardown and emits `recording-stopped` with a
`reason` string, alongside the existing save metadata. An ordinary user stop
has `reason: null`.

## Automated checks

```sh
cargo test --locked -p tingskriver-core --test callback_queue -- --nocapture
cargo test --locked -p tingskriver-core --lib worker_tests
cargo test --locked -p tingskriver-core --lib internal_stop_tests
cargo test --locked -p tingskriver-core --lib audio::recording_state::poisoning_tests
cargo test --locked -p tingskriver-core --test callback_queue synchronous_dsp_versus_callback_queue -- --ignored --nocapture
```

The allocation test counts allocations/reallocations on the producer test
thread, including overflow, for f32/i8/i16/i32. Other tests cover concurrent
producer/consumer ordering, capture timestamps and mute, queue drain while
paused, resampler tail placement, worker panic, stop events and 20 simulated
recoverable stream errors spread over an hour using an injected clock. No
physical audio device or hour-long sleep is needed.

The ignored timing test compares the synchronous DSP entry point used by the
old callback against the ring producer, with synthetic 48 kHz stereo input
(480 frames per callback). It excludes OS scheduling, actual CPAL hardware and
disk I/O, and therefore cannot establish the Windows p99 acceptance criterion.

## Windows acceptance check

Use a debug build and record both microphone and system audio with the intended
devices. Include mute, dictation, pause/resume and a device disconnect/reconnect.
At stream stop, each worker logs `Audio callback p99 <= … us (debug histogram)`.
This is an upper bound from power-of-two microsecond buckets for the complete
sample callback (including clock/mute snapshots). Release builds omit the timer
and histogram entirely. Check callback p99 below 1000 µs with ETW/WPA or Tracy
on Mads' Windows machine; the histogram is a quick diagnostic, not a substitute
for that trace. Check the final tracks remain aligned and no unexpected device
switch occurs during mute or pause.
