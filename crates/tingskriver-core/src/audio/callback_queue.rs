//! Single producer (CPAL), single consumer (DSP). All storage is allocated
//! before starting the stream. Overflow drops a whole buffer, never a channel.
use ringbuf::{
    traits::{Consumer, Observer, Producer, Split},
    HeapCons, HeapProd, HeapRb,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;

#[derive(Clone, Copy, Default, Debug)]
pub struct CaptureStamp {
    pub position: Duration,
    pub muted: bool,
}

pub struct Input<T: Copy> {
    producer: HeapProd<T>,
    channels: usize,
    dropped: Arc<AtomicU64>,
    stamps: HeapProd<(usize, CaptureStamp)>,
    #[cfg(debug_assertions)]
    timings: Arc<[AtomicU64; 32]>,
}

pub struct Output<T: Copy> {
    consumer: HeapCons<T>,
    dropped: Arc<AtomicU64>,
    stamps: HeapCons<(usize, CaptureStamp)>,
    #[cfg(debug_assertions)]
    timings: Arc<[AtomicU64; 32]>,
}

pub fn queue<T: Copy>(capacity: usize, channels: usize) -> (Input<T>, Output<T>) {
    assert!(channels > 0 && capacity >= channels);
    let (producer, consumer) = HeapRb::new(capacity).split();
    let dropped = Arc::new(AtomicU64::new(0));
    let (stamp_input, stamp_output) = HeapRb::new(4096).split();
    #[cfg(debug_assertions)]
    let timings = Arc::new(std::array::from_fn(|_| AtomicU64::new(0)));
    (
        Input {
            producer,
            channels,
            dropped: dropped.clone(),
            stamps: stamp_input,
            #[cfg(debug_assertions)]
            timings: timings.clone(),
        },
        Output {
            consumer,
            dropped,
            stamps: stamp_output,
            #[cfg(debug_assertions)]
            timings,
        },
    )
}

impl<T: Copy> Input<T> {
    #[cfg(debug_assertions)]
    pub fn record_duration(&self, duration: Duration) {
        let us = duration.as_nanos().div_ceil(1000).clamp(1, 1 << 31) as u32;
        let bucket = (32 - (us - 1).leading_zeros()).min(31) as usize;
        self.timings[bucket].fetch_add(1, Ordering::Relaxed);
    }

    pub fn copy_from(&mut self, data: &[T], stamp: CaptureStamp) -> bool {
        if data.is_empty() {
            return true;
        }
        if data.len() % self.channels != 0
            || self.producer.vacant_len() < data.len()
            || self.stamps.is_full()
        {
            self.dropped.fetch_add(data.len() as u64, Ordering::Relaxed);
            return false;
        }
        self.producer.push_slice(data);
        // Publish metadata after samples: observing a stamp guarantees its
        // entire buffer is ready, even when the consumer runs concurrently.
        let _ = self.stamps.try_push((data.len(), stamp));
        true
    }
}

impl<T: Copy> Output<T> {
    #[cfg(debug_assertions)]
    pub fn callback_count(&self) -> u64 {
        self.timings.iter().map(|n| n.load(Ordering::Relaxed)).sum()
    }

    /// Conservative p99 upper bound in microseconds, in power-of-two buckets.
    #[cfg(debug_assertions)]
    pub fn callback_p99_us(&self) -> u64 {
        let counts: [u64; 32] = std::array::from_fn(|i| self.timings[i].load(Ordering::Relaxed));
        let total: u64 = counts.iter().sum();
        if total == 0 {
            return 0;
        }
        let target = (total * 99).div_ceil(100);
        let mut sum = 0;
        for (i, count) in counts.iter().enumerate() {
            sum += count;
            if sum >= target {
                return 1u64 << i;
            }
        }
        1u64 << 31
    }

    pub fn pop_into(&mut self, data: &mut [T]) -> Option<(usize, CaptureStamp)> {
        let &(len, stamp) = self.stamps.first()?;
        assert!(len <= data.len());
        self.consumer.pop_slice(&mut data[..len]);
        let _ = self.stamps.try_pop();
        Some((len, stamp))
    }

    pub fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}
