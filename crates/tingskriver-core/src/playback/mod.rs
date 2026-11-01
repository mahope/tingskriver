//! Bounded streaming playback, independent of UI and audio hardware.
mod decode;
mod mp4;
mod output;
use decode::Track;
pub use output::{AudioOutput, CpalOutput, FakeOutput, Render};
use ringbuf::{
    traits::{Observer, Producer, Split},
    HeapRb,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum PlaybackError {
    #[error("Lydfilen kunne ikke åbnes")]
    Open,
    #[error("Lydformatet understøttes ikke")]
    Unsupported,
    #[error("Lydfilen kunne ikke afkodes")]
    Decode,
    #[error("Søgning i lydfilen mislykkedes")]
    Seek,
    #[error("Hastigheden skal være mellem 0,5 og 2,0")]
    Speed,
    #[error("Sporstyrken skal være mellem 0 og 1")]
    Gain,
    #[error("Lydudgangen er ikke tilgængelig")]
    Output,
    #[error("Afspilleren er stoppet")]
    Stopped,
}
#[derive(Clone, Debug)]
pub enum Source {
    Mixed(PathBuf),
    Tracks { mikrofon: PathBuf, system: PathBuf },
}
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Position(Duration),
    Ended,
    Error(PlaybackError),
}
#[derive(Clone, Copy)]
struct Frame {
    sample: f32,
    position: u64,
    generation: u64,
    end: bool,
}
struct Shared {
    playing: AtomicBool,
    position: AtomicU64,
    generation: AtomicU64,
    ended: AtomicU64,
    fault: AtomicBool,
}
enum Command {
    Reset(
        Duration,
        f32,
        [f32; 2],
        mpsc::SyncSender<Result<(), PlaybackError>>,
    ),
    Stop,
}

pub struct Player<O: AudioOutput> {
    output: O,
    shared: Arc<Shared>,
    commands: mpsc::Sender<Command>,
    worker: Option<JoinHandle<()>>,
    speed: f32,
    gains: [f32; 2],
}
impl<O: AudioOutput> Player<O> {
    pub fn open(
        source: Source,
        mut output: O,
    ) -> Result<(Self, mpsc::Receiver<Event>), PlaybackError> {
        let rate = output.sample_rate();
        if rate == 0 {
            return Err(PlaybackError::Output);
        }
        let paths = match source {
            Source::Mixed(p) => vec![p],
            Source::Tracks { mikrofon, system } => vec![mikrofon, system],
        };
        let tracks = paths
            .iter()
            .map(|p| Track::open(p))
            .collect::<Result<Vec<_>, _>>()?;
        let (mut producer, consumer) = HeapRb::<Frame>::new(8192).split();
        let shared = Arc::new(Shared {
            playing: AtomicBool::new(false),
            position: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            ended: AtomicU64::new(0),
            fault: AtomicBool::new(false),
        });
        let (events_tx, events_rx) = mpsc::sync_channel(64);
        output.start(Render {
            consumer,
            shared: shared.clone(),
        })?;
        let (commands, rx) = mpsc::channel();
        let state = shared.clone();
        let worker = thread::Builder::new()
            .name("audio-playback".into())
            .spawn(move || {
                let mut tracks = tracks;
                let mut time = 0.0;
                let mut anchor = 0.0;
                let mut frame_index = 0_u64;
                let mut speed = 1.0;
                let mut gains = [1.0, 1.0];
                let mut generation = 0;
                let mut finished = false;
                let mut reported_end = false;
                let mut last_position = u64::MAX;
                let mut pending_error = None;
                loop {
                    match rx.try_recv() {
                        Ok(Command::Stop) | Err(mpsc::TryRecvError::Disconnected) => break,
                        Ok(Command::Reset(t, s, g, ack)) => {
                            let result = tracks.iter_mut().try_for_each(|track| track.seek(t));
                            if result.is_ok() {
                                time = t.as_secs_f64();
                                anchor = time;
                                frame_index = 0;
                                speed = s;
                                gains = g;
                                generation = state.generation.load(Ordering::SeqCst);
                                finished = false;
                                reported_end = false;
                            }
                            let _ = ack.send(result);
                        }
                        Err(mpsc::TryRecvError::Empty) => {}
                    }
                    let pos = state.position.load(Ordering::Relaxed);
                    if pos != last_position
                        && events_tx
                            .try_send(Event::Position(Duration::from_nanos(pos)))
                            .is_ok()
                    {
                        last_position = pos;
                    }
                    if state.ended.load(Ordering::Acquire) == generation + 1 && !reported_end {
                        reported_end = events_tx.try_send(Event::Ended).is_ok();
                    }
                    if state.fault.swap(false, Ordering::AcqRel) {
                        pending_error = Some(PlaybackError::Output);
                    }
                    if let Some(error) = pending_error.clone() {
                        if events_tx.try_send(Event::Error(error)).is_ok() {
                            pending_error = None;
                        }
                    }
                    if finished || producer.is_full() {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    let mut mixed = 0.0;
                    let mut any = false;
                    let mut failed = false;
                    for (i, track) in tracks.iter_mut().enumerate() {
                        match track.at(time) {
                            Ok(Some(s)) => {
                                mixed += s * gains[i];
                                any = true;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                pending_error = Some(e);
                                failed = true;
                                break;
                            }
                        }
                    }
                    if failed {
                        finished = true;
                        continue;
                    }
                    let next = if any {
                        anchor + (frame_index + 1) as f64 * speed as f64 / rate as f64
                    } else {
                        time
                    };
                    let frame = Frame {
                        sample: mixed.clamp(-1.0, 1.0),
                        position: (next * 1e9).round() as u64,
                        generation,
                        end: !any,
                    };
                    if producer.try_push(frame).is_ok() {
                        time = next;
                        frame_index += 1;
                        finished = !any;
                    }
                }
            })
            .map_err(|_| PlaybackError::Stopped)?;
        Ok((
            Self {
                output,
                shared,
                commands,
                worker: Some(worker),
                speed: 1.0,
                gains: [1.0, 1.0],
            },
            events_rx,
        ))
    }
    pub fn play(&self) {
        self.shared.playing.store(true, Ordering::Release);
    }
    pub fn pause(&self) {
        self.shared.playing.store(false, Ordering::Release);
    }
    pub fn position(&self) -> Duration {
        Duration::from_nanos(self.shared.position.load(Ordering::Acquire))
    }
    fn reset(&mut self, t: Duration) -> Result<(), PlaybackError> {
        let playing = self.shared.playing.swap(false, Ordering::SeqCst);
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        self.shared.ended.store(0, Ordering::Release);
        let (tx, rx) = mpsc::sync_channel(1);
        self.commands
            .send(Command::Reset(t, self.speed, self.gains, tx))
            .map_err(|_| PlaybackError::Stopped)?;
        rx.recv().map_err(|_| PlaybackError::Stopped)??;
        self.shared
            .position
            .store(t.as_nanos().min(u64::MAX as u128) as u64, Ordering::Release);
        self.shared.playing.store(playing, Ordering::Release);
        Ok(())
    }
    pub fn seek(&mut self, t: Duration) -> Result<(), PlaybackError> {
        self.reset(t)
    }
    pub fn set_speed(&mut self, speed: f32) -> Result<(), PlaybackError> {
        if !speed.is_finite() || !(0.5..=2.0).contains(&speed) {
            return Err(PlaybackError::Speed);
        }
        self.speed = speed;
        self.reset(self.position())
    }
    /// Zero gain mutes a track. Gains must be finite and in 0..=1.
    pub fn set_track_gains(&mut self, mikrofon: f32, system: f32) -> Result<(), PlaybackError> {
        if [mikrofon, system]
            .iter()
            .any(|g| !g.is_finite() || !(0.0..=1.0).contains(g))
        {
            return Err(PlaybackError::Gain);
        }
        self.gains = [mikrofon, system];
        self.reset(self.position())
    }
    pub fn output_mut(&mut self) -> &mut O {
        &mut self.output
    }
}
impl<O: AudioOutput> Drop for Player<O> {
    fn drop(&mut self) {
        self.pause();
        let _ = self.commands.send(Command::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[cfg(test)]
mod tests;
