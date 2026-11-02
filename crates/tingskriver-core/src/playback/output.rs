use super::{Frame, PlaybackError, Shared};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{traits::Consumer, HeapCons};
use std::sync::{atomic::Ordering, Arc};

/// The only object used by an output callback. No locks, allocation or file I/O.
pub struct Render {
    pub(super) consumer: HeapCons<Frame>,
    pub(super) shared: Arc<Shared>,
}
impl Render {
    /// Writes mono frames; returns the number of media frames (underruns are silence).
    pub fn render(&mut self, output: &mut [f32]) -> usize {
        output.fill(0.0);
        let previous_position = self.shared.position.load(Ordering::Acquire);
        let generation = self.shared.generation.load(Ordering::SeqCst);
        // Bound callback work even when a seek invalidated a full queue.
        for _ in 0..8192 {
            match self.consumer.first() {
                Some(frame) if frame.generation != generation => {
                    self.consumer.try_pop();
                }
                _ => break,
            }
        }
        if !self.shared.playing.load(Ordering::Acquire)
            || self.shared.ended.load(Ordering::Acquire) == generation + 1
        {
            return 0;
        }
        let mut count = 0;
        let mut position = previous_position;
        for sample in output {
            let Some(frame) = self.consumer.try_pop() else {
                break;
            };
            if frame.generation != generation {
                break;
            }
            if frame.end {
                self.shared.ended.store(generation + 1, Ordering::Release);
                break;
            }
            *sample = frame.sample;
            position = frame.position;
            count += 1;
        }
        // A concurrent seek owns the new position; an in-flight callback must
        // never overwrite it after completing its old generation.
        if self.shared.generation.load(Ordering::SeqCst) == generation {
            let _ = self.shared.position.compare_exchange(
                previous_position,
                position,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        count
    }
}
pub trait AudioOutput {
    fn sample_rate(&self) -> u32;
    fn start(&mut self, render: Render) -> Result<(), PlaybackError>;
}
/// Pulls frames without a device or wall clock. Caller owns pacing and storage.
pub struct FakeOutput {
    rate: u32,
    render: Option<Render>,
}
impl FakeOutput {
    pub fn new(rate: u32) -> Self {
        Self { rate, render: None }
    }
    pub fn pull(&mut self, samples: &mut [f32]) -> usize {
        match &mut self.render {
            Some(render) => render.render(samples),
            None => {
                samples.fill(0.0);
                0
            }
        }
    }
}
impl AudioOutput for FakeOutput {
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn start(&mut self, render: Render) -> Result<(), PlaybackError> {
        self.render = Some(render);
        Ok(())
    }
}
pub struct CpalOutput {
    device: cpal::Device,
    config: cpal::StreamConfig,
    format: cpal::SampleFormat,
    stream: Option<cpal::Stream>,
}
impl CpalOutput {
    pub fn default_device() -> Result<Self, PlaybackError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(PlaybackError::Output)?;
        let config = device
            .default_output_config()
            .map_err(|_| PlaybackError::Output)?;
        Ok(Self {
            format: config.sample_format(),
            config: config.into(),
            device,
            stream: None,
        })
    }
    fn build<T>(&self, mut render: Render) -> Result<cpal::Stream, PlaybackError>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        let channels = self.config.channels as usize;
        if channels == 0 {
            return Err(PlaybackError::Output);
        }
        let shared = render.shared.clone();
        self.device
            .build_output_stream(
                &self.config,
                move |data: &mut [T], _| {
                    for frame in data.chunks_mut(channels) {
                        let mut mono = [0.0];
                        render.render(&mut mono);
                        for sample in frame {
                            *sample = T::from_sample(mono[0]);
                        }
                    }
                },
                move |_| {
                    shared.playing.store(false, Ordering::Release);
                    shared.fault.store(true, Ordering::Release);
                },
                None,
            )
            .map_err(|_| PlaybackError::Output)
    }
}
impl AudioOutput for CpalOutput {
    fn sample_rate(&self) -> u32 {
        self.config.sample_rate.0
    }
    fn start(&mut self, render: Render) -> Result<(), PlaybackError> {
        let stream = match self.format {
            cpal::SampleFormat::F32 => self.build::<f32>(render),
            cpal::SampleFormat::I16 => self.build::<i16>(render),
            cpal::SampleFormat::U16 => self.build::<u16>(render),
            _ => Err(PlaybackError::Output),
        }?;
        stream.play().map_err(|_| PlaybackError::Output)?;
        self.stream = Some(stream);
        Ok(())
    }
}
