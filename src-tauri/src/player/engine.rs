use std::fs::File;
use std::io::BufReader;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::Duration;

use rodio::{Decoder, OutputStream, Sink, Source};

use crate::eq::{EqParams, EqShared, EqSource};
use crate::error::{AppError, AppResult};

pub trait PlayerEngine: Send + Sync {
    fn set_uri(&self, path: &Path) -> AppResult<()>;
    fn play(&self) -> AppResult<()>;
    fn pause(&self) -> AppResult<()>;
    fn stop(&self) -> AppResult<()>;
    fn seek(&self, position_ms: u64) -> AppResult<()>;
    fn position_ms(&self) -> u64;
    fn duration_ms(&self) -> u64;
    fn set_volume(&self, volume: f64) -> AppResult<()>;
    fn set_speed(&self, speed: f64) -> AppResult<()>;
    fn speed(&self) -> f64;
    fn set_eq(&self, params: EqParams) -> AppResult<()>;
    fn set_gapless_next(&self, path: Option<&Path>) -> AppResult<()>;
    fn is_playing(&self) -> bool;
    fn queued_sources(&self) -> usize;
    fn is_empty(&self) -> bool;
}

enum Command {
    SetUri(PathBuf, Sender<AppResult<()>>),
    Play(Sender<AppResult<()>>),
    Pause(Sender<AppResult<()>>),
    Stop(Sender<AppResult<()>>),
    Seek(u64, Sender<AppResult<()>>),
    SetVolume(f64, Sender<AppResult<()>>),
    SetSpeed(f64, Sender<AppResult<()>>),
    SetEq(EqParams, Sender<AppResult<()>>),
    Append(PathBuf, Sender<AppResult<()>>),
}

struct Shared {
    position_ms: AtomicU64,
    duration_ms: AtomicU64,
    speed_bits: AtomicU64,
    queued: AtomicUsize,
    empty: AtomicBool,
    playing: AtomicBool,
    sample_rate: AtomicU32,
}

pub struct RodioEngine {
    tx: Sender<Command>,
    shared: Arc<Shared>,
    eq: Arc<EqShared>,
}

impl RodioEngine {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        let shared = Arc::new(Shared {
            position_ms: AtomicU64::new(0),
            duration_ms: AtomicU64::new(0),
            speed_bits: AtomicU64::new(1.0f64.to_bits()),
            queued: AtomicUsize::new(0),
            empty: AtomicBool::new(true),
            playing: AtomicBool::new(false),
            sample_rate: AtomicU32::new(48_000),
        });
        let eq = Arc::new(EqShared::default());
        let thread_shared = Arc::clone(&shared);
        let thread_eq = Arc::clone(&eq);
        std::thread::Builder::new()
            .name("audios-rodio".into())
            .spawn(move || audio_thread(rx, thread_shared, thread_eq))
            .expect("audio thread");
        Self { tx, shared, eq }
    }

    fn send(&self, build: impl FnOnce(Sender<AppResult<()>>) -> Command) -> AppResult<()> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(build(tx))
            .map_err(|_| AppError::msg("audio thread stopped"))?;
        rx.recv()
            .map_err(|_| AppError::msg("audio thread stopped"))?
    }

    pub fn sample_rate(&self) -> u32 {
        let rate = self.shared.sample_rate.load(Ordering::Relaxed);
        if rate == 0 {
            48_000
        } else {
            rate
        }
    }

    pub fn set_viz_enabled(&self, enabled: bool) {
        self.eq.set_viz_enabled(enabled);
    }

    pub fn viz_enabled(&self) -> bool {
        self.eq.viz_enabled()
    }

    pub fn spectrum(&self) -> Option<[u8; crate::viz::BANDS]> {
        self.eq.spectrum()
    }
}

impl PlayerEngine for RodioEngine {
    fn set_uri(&self, path: &Path) -> AppResult<()> {
        self.send(|reply| Command::SetUri(path.to_path_buf(), reply))
    }

    fn play(&self) -> AppResult<()> {
        self.send(Command::Play)
    }

    fn pause(&self) -> AppResult<()> {
        self.send(Command::Pause)
    }

    fn stop(&self) -> AppResult<()> {
        self.send(Command::Stop)
    }

    fn seek(&self, file_ms: u64) -> AppResult<()> {
        // Rodio's speed filter seeks in output time, then multiplies back
        // into the file. Callers pass a position in the recording.
        let speed = self.speed();
        let output_ms = (file_ms as f64 / speed).round() as u64;
        self.send(|reply| Command::Seek(output_ms, reply))
    }

    fn position_ms(&self) -> u64 {
        let wall = self.shared.position_ms.load(Ordering::Relaxed) as f64;
        (wall * self.speed()).round() as u64
    }

    fn duration_ms(&self) -> u64 {
        self.shared.duration_ms.load(Ordering::Relaxed)
    }

    fn set_volume(&self, volume: f64) -> AppResult<()> {
        self.send(|reply| Command::SetVolume(volume, reply))
    }

    fn set_speed(&self, speed: f64) -> AppResult<()> {
        self.shared
            .speed_bits
            .store(speed.to_bits(), Ordering::Relaxed);
        self.send(|reply| Command::SetSpeed(speed, reply))
    }

    fn speed(&self) -> f64 {
        let speed = f64::from_bits(self.shared.speed_bits.load(Ordering::Relaxed));
        if speed.is_finite() && speed > 0.0 {
            speed
        } else {
            1.0
        }
    }

    fn set_eq(&self, params: EqParams) -> AppResult<()> {
        self.eq.set(params.clone());
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Command::SetEq(params, tx)).is_err() {
            return Ok(());
        }
        let _ = rx.recv();
        Ok(())
    }

    fn set_gapless_next(&self, path: Option<&Path>) -> AppResult<()> {
        let Some(path) = path else {
            return Ok(());
        };
        self.send(|reply| Command::Append(path.to_path_buf(), reply))
    }

    fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    fn queued_sources(&self) -> usize {
        self.shared.queued.load(Ordering::Relaxed)
    }

    fn is_empty(&self) -> bool {
        self.shared.empty.load(Ordering::Relaxed)
    }
}

fn audio_thread(rx: mpsc::Receiver<Command>, shared: Arc<Shared>, eq: Arc<EqShared>) {
    let Ok((_stream, handle)) = OutputStream::try_default() else {
        while let Ok(command) = rx.recv() {
            reply_err(command, AppError::msg("no audio output device"));
        }
        return;
    };
    let Ok(sink) = Sink::try_new(&handle) else {
        while let Ok(command) = rx.recv() {
            reply_err(command, AppError::msg("could not open audio sink"));
        }
        return;
    };
    sink.pause();

    loop {
        match rx.recv_timeout(Duration::from_millis(40)) {
            Ok(command) => handle_command(&sink, &shared, &eq, command),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        publish(&sink, &shared);
    }
}

fn handle_command(sink: &Sink, shared: &Shared, eq: &Arc<EqShared>, command: Command) {
    match command {
        Command::SetUri(path, reply) => {
            let _ = reply.send(load_into(sink, shared, eq, &path, true));
        }
        Command::Play(reply) => {
            sink.play();
            let _ = reply.send(Ok(()));
        }
        Command::Pause(reply) => {
            sink.pause();
            let _ = reply.send(Ok(()));
        }
        Command::Stop(reply) => {
            sink.clear();
            sink.pause();
            shared.duration_ms.store(0, Ordering::Relaxed);
            shared.position_ms.store(0, Ordering::Relaxed);
            let _ = reply.send(Ok(()));
        }
        Command::Seek(position_ms, reply) => {
            let result = sink
                .try_seek(Duration::from_millis(position_ms))
                .map_err(|error| AppError::msg(format!("seek: {error}")));
            let _ = reply.send(result);
        }
        Command::SetVolume(volume, reply) => {
            sink.set_volume(volume.clamp(0.0, 1.5) as f32);
            let _ = reply.send(Ok(()));
        }
        Command::SetSpeed(speed, reply) => {
            sink.set_speed(speed as f32);
            let _ = reply.send(Ok(()));
        }
        Command::SetEq(params, reply) => {
            eq.set(params);
            let _ = reply.send(Ok(()));
        }
        Command::Append(path, reply) => {
            let _ = reply.send(load_into(sink, shared, eq, &path, false));
        }
    }
}

fn load_into(
    sink: &Sink,
    shared: &Shared,
    eq: &Arc<EqShared>,
    path: &Path,
    replace: bool,
) -> AppResult<()> {
    let decoder = decoder_for(path)?;
    let duration = decoder
        .total_duration()
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    if replace {
        sink.clear();
        sink.pause();
        shared.duration_ms.store(duration, Ordering::Relaxed);
        shared.position_ms.store(0, Ordering::Relaxed);
        let rate = decoder.sample_rate();
        shared
            .sample_rate
            .store(if rate == 0 { 48_000 } else { rate }, Ordering::Relaxed);
    }
    // Decode → EQ Source (float biquads) → sink. Volume / ReplayGain stay on the sink.
    sink.append(EqSource::new(decoder, Arc::clone(eq)));
    Ok(())
}

pub(crate) fn probe_audio(path: &Path) -> AppResult<()> {
    decoder_for(path).map(|_| ())
}

fn decoder_for(path: &Path) -> AppResult<Decoder<BufReader<File>>> {
    // Symphonia can panic on some MP4/AAC files instead of returning Err.
    let file = File::open(path)?;
    let opened =
        catch_unwind(AssertUnwindSafe(|| Decoder::new(BufReader::new(file)))).map_err(|_| {
            AppError::msg(format!(
                "cannot decode {}",
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string())
            ))
        })?;
    opened.map_err(|error| {
        AppError::msg(format!(
            "cannot decode {}: {error}",
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string())
        ))
    })
}

fn publish(sink: &Sink, shared: &Shared) {
    shared
        .position_ms
        .store(sink.get_pos().as_millis() as u64, Ordering::Relaxed);
    shared.queued.store(sink.len(), Ordering::Relaxed);
    shared.empty.store(sink.empty(), Ordering::Relaxed);
    shared
        .playing
        .store(!sink.is_paused() && !sink.empty(), Ordering::Relaxed);
}

fn reply_err(command: Command, error: AppError) {
    match command {
        Command::SetUri(_, reply)
        | Command::Play(reply)
        | Command::Pause(reply)
        | Command::Stop(reply)
        | Command::Seek(_, reply)
        | Command::SetVolume(_, reply)
        | Command::SetSpeed(_, reply)
        | Command::SetEq(_, reply)
        | Command::Append(_, reply) => {
            let _ = reply.send(Err(error));
        }
    }
}
