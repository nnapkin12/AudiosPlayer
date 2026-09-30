use std::fs::File;
use std::io::BufReader;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};

use crate::eq::{EqParams, EqShared, EqSource, SourceHooks};
use crate::error::{AppError, AppResult};

/// How long a caller waits for the audio thread before giving up. Rodio can
/// block forever when the output device disappears; the UI must not.
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// Playing, not paused, sources queued, and yet the position has not moved
/// for this long: the stream is dead. Rebuild it.
const STALL_LIMIT: Duration = Duration::from_secs(3);
/// With no output device, try to open one this often.
const REOPEN_EVERY: Duration = Duration::from_secs(5);
const NO_DEVICE: &str = "no audio output device";

pub trait PlayerEngine: Send + Sync {
    /// Turn a library path into one this engine can open. The rodio engine
    /// transcodes files Symphonia rejects; a test engine returns the path.
    fn prepare(&self, path: &Path) -> AppResult<PathBuf>;
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
    /// Drop a source appended by `set_gapless_next` that has not started.
    /// The current song keeps playing; the sink runs empty at its end.
    fn cancel_gapless_next(&self) -> AppResult<()>;
    /// How many sources have produced sound since the last `set_uri`.
    /// 1 while the loaded song plays, 2 once a gapless append takes over.
    fn started_sources(&self) -> u64;
    fn is_playing(&self) -> bool;
    fn is_empty(&self) -> bool;
    fn sample_rate(&self) -> u32;
    fn set_viz_enabled(&self, enabled: bool);
    fn viz_enabled(&self) -> bool;
    fn spectrum(&self) -> Option<[u8; crate::viz::BANDS]>;
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
    CancelAppend(Sender<AppResult<()>>),
}

struct Shared {
    /// File time, written by the source that is producing sound.
    position_ms: AtomicU64,
    started_sources: AtomicU64,
    duration_ms: AtomicU64,
    speed_bits: AtomicU64,
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
            started_sources: AtomicU64::new(0),
            duration_ms: AtomicU64::new(0),
            speed_bits: AtomicU64::new(1.0f64.to_bits()),
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
        match rx.recv_timeout(SEND_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AppError::msg(
                "audio output stopped responding; check the sound device",
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AppError::msg("audio thread stopped")),
        }
    }
}

impl PlayerEngine for RodioEngine {
    fn prepare(&self, path: &Path) -> AppResult<PathBuf> {
        super::convert::for_playback(path)
    }

    fn sample_rate(&self) -> u32 {
        let rate = self.shared.sample_rate.load(Ordering::Relaxed);
        if rate == 0 {
            48_000
        } else {
            rate
        }
    }

    fn set_viz_enabled(&self, enabled: bool) {
        self.eq.set_viz_enabled(enabled);
    }

    fn viz_enabled(&self) -> bool {
        self.eq.viz_enabled()
    }

    fn spectrum(&self) -> Option<[u8; crate::viz::BANDS]> {
        self.eq.spectrum()
    }

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
        // Counted in frames by the playing source, so it is right even after
        // speed changed mid-song.
        self.shared.position_ms.load(Ordering::Relaxed)
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
        // The shared params are what the filters read; the command only
        // orders the swap after anything already queued. Never fail on it.
        self.eq.set(params.clone());
        let _ = self.send(|reply| Command::SetEq(params, reply));
        Ok(())
    }

    fn set_gapless_next(&self, path: Option<&Path>) -> AppResult<()> {
        let Some(path) = path else {
            return Ok(());
        };
        self.send(|reply| Command::Append(path.to_path_buf(), reply))
    }

    fn cancel_gapless_next(&self) -> AppResult<()> {
        self.send(Command::CancelAppend)
    }

    fn started_sources(&self) -> u64 {
        self.shared.started_sources.load(Ordering::Relaxed)
    }

    fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    fn is_empty(&self) -> bool {
        self.shared.empty.load(Ordering::Relaxed)
    }
}

/// The output device and the sink that plays into it. Both are replaced
/// together when the device goes away.
struct Output {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Sink,
}

struct AudioState {
    output: Option<Output>,
    last_open_attempt: Option<Instant>,
    volume: f32,
    speed: f32,
    /// The file the sink is playing, for a reload after the device is rebuilt.
    current: Option<PathBuf>,
    /// Cancel flag of the last gapless append, until it starts.
    appended: Option<Arc<AtomicBool>>,
    duration_ms: u64,
    last_pos: Duration,
    last_advance: Instant,
}

impl AudioState {
    fn new() -> Self {
        Self {
            output: None,
            last_open_attempt: None,
            volume: 1.0,
            speed: 1.0,
            current: None,
            appended: None,
            duration_ms: 0,
            last_pos: Duration::ZERO,
            last_advance: Instant::now(),
        }
    }

    fn open_output(&self) -> AppResult<Output> {
        let (stream, handle) = OutputStream::try_default().map_err(|_| AppError::msg(NO_DEVICE))?;
        let sink =
            Sink::try_new(&handle).map_err(|_| AppError::msg("could not open audio sink"))?;
        sink.pause();
        sink.set_volume(self.volume);
        sink.set_speed(self.speed);
        Ok(Output {
            _stream: stream,
            handle,
            sink,
        })
    }

    /// Open the device if it is not open. `true` when this call opened it, so
    /// the caller can put the current song back.
    fn ensure_output(&mut self) -> AppResult<bool> {
        if self.output.is_some() {
            return Ok(false);
        }
        self.last_open_attempt = Some(Instant::now());
        let output = self.open_output()?;
        self.output = Some(output);
        self.last_advance = Instant::now();
        Ok(true)
    }

    /// A new, empty sink on the same stream. Dropping the old sink stops it
    /// without waiting on the audio callback, unlike `Sink::clear`.
    fn fresh_sink(&mut self) -> AppResult<()> {
        let Some(output) = self.output.as_mut() else {
            return Err(AppError::msg(NO_DEVICE));
        };
        match Sink::try_new(&output.handle) {
            Ok(sink) => {
                sink.pause();
                sink.set_volume(self.volume);
                sink.set_speed(self.speed);
                output.sink = sink;
                Ok(())
            }
            Err(_) => {
                // The stream is gone. Drop it; the next command reopens.
                self.output = None;
                Err(AppError::msg(NO_DEVICE))
            }
        }
    }

    fn sink(&self) -> Option<&Sink> {
        self.output.as_ref().map(|output| &output.sink)
    }

    /// Playing, with sources queued, but the position has not moved for
    /// `STALL_LIMIT`: the device stopped calling back.
    fn stalled(&mut self) -> bool {
        let Some(sink) = self.sink() else {
            return false;
        };
        let active = !sink.is_paused() && !sink.empty();
        let pos = sink.get_pos();
        if !active || pos != self.last_pos {
            self.last_pos = pos;
            self.last_advance = Instant::now();
            return false;
        }
        self.last_advance.elapsed() > STALL_LIMIT
    }
}

fn audio_thread(rx: mpsc::Receiver<Command>, shared: Arc<Shared>, eq: Arc<EqShared>) {
    let mut state = AudioState::new();
    if let Err(error) = state.ensure_output() {
        eprintln!("Audios! audio: {error}; will retry");
    }

    loop {
        match rx.recv_timeout(Duration::from_millis(40)) {
            Ok(command) => handle_command(&mut state, &shared, &eq, command),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if state.stalled() {
            eprintln!("Audios! audio: output stalled; rebuilding the stream");
            rebuild(&mut state, &shared, &eq, true);
        } else if state.output.is_none()
            && state
                .last_open_attempt
                .is_none_or(|at| at.elapsed() > REOPEN_EVERY)
        {
            rebuild(&mut state, &shared, &eq, false);
        }
        publish(&state, &shared);
    }
}

/// Drop the stream (if any), open a new one, and put the current song back
/// where it was. `resume` keeps it playing; otherwise it comes back paused.
fn rebuild(state: &mut AudioState, shared: &Arc<Shared>, eq: &Arc<EqShared>, resume: bool) {
    let was_playing = state
        .sink()
        .is_some_and(|sink| !sink.is_paused() && !sink.empty());
    let position = state.last_pos;
    state.output = None;
    if state.ensure_output().is_err() {
        return;
    }
    let Some(path) = state.current.clone() else {
        return;
    };
    // Seek the decoder before it reaches the sink. `Sink::try_seek` waits on
    // the audio callback and would hang here if the new stream is dead too.
    let start = Duration::from_secs_f64(position.as_secs_f64() * state.speed as f64);
    if load_into(state, shared, eq, &path, true, Some(start)).is_err() {
        return;
    }
    state.last_pos = position;
    if resume && was_playing {
        if let Some(sink) = state.sink() {
            sink.play();
        }
    }
}

fn handle_command(
    state: &mut AudioState,
    shared: &Arc<Shared>,
    eq: &Arc<EqShared>,
    command: Command,
) {
    match command {
        Command::SetUri(path, reply) => {
            let result = state
                .ensure_output()
                .and_then(|_| load_into(state, shared, eq, &path, true, None));
            if result.is_ok() {
                state.current = Some(path);
            }
            let _ = reply.send(result);
        }
        Command::Play(reply) => {
            let result = (|| {
                let fresh = state.ensure_output()?;
                // The device came back: reload the song that was up.
                if fresh {
                    if let Some(path) = state.current.clone() {
                        load_into(state, shared, eq, &path, true, None)?;
                    }
                }
                if let Some(sink) = state.sink() {
                    sink.play();
                }
                Ok(())
            })();
            let _ = reply.send(result);
        }
        Command::Pause(reply) => {
            if let Some(sink) = state.sink() {
                sink.pause();
            }
            let _ = reply.send(Ok(()));
        }
        Command::Stop(reply) => {
            state.current = None;
            state.duration_ms = 0;
            state.last_pos = Duration::ZERO;
            shared.duration_ms.store(0, Ordering::Relaxed);
            shared.position_ms.store(0, Ordering::Relaxed);
            // Dropping the sink is the non-blocking stop. No device is fine.
            let result = if state.output.is_some() {
                state.fresh_sink()
            } else {
                Ok(())
            };
            let _ = reply.send(result);
        }
        Command::Seek(position_ms, reply) => {
            let result = match state.sink() {
                Some(sink) => sink
                    .try_seek(Duration::from_millis(position_ms))
                    .map_err(|error| AppError::msg(format!("seek: {error}"))),
                None => Err(AppError::msg(NO_DEVICE)),
            };
            if result.is_ok() {
                state.last_pos = Duration::from_millis(position_ms);
                state.last_advance = Instant::now();
            }
            let _ = reply.send(result);
        }
        Command::SetVolume(volume, reply) => {
            state.volume = volume.clamp(0.0, 3.0) as f32;
            if let Some(sink) = state.sink() {
                sink.set_volume(state.volume);
            }
            let _ = reply.send(Ok(()));
        }
        Command::SetSpeed(speed, reply) => {
            state.speed = speed as f32;
            if let Some(sink) = state.sink() {
                sink.set_speed(state.speed);
            }
            let _ = reply.send(Ok(()));
        }
        Command::SetEq(params, reply) => {
            eq.set(params);
            let _ = reply.send(Ok(()));
        }
        Command::Append(path, reply) => {
            let result = if state.output.is_some() {
                load_into(state, shared, eq, &path, false, None)
            } else {
                Err(AppError::msg(NO_DEVICE))
            };
            let _ = reply.send(result);
        }
        Command::CancelAppend(reply) => {
            if let Some(flag) = state.appended.take() {
                flag.store(true, Ordering::Relaxed);
            }
            let _ = reply.send(Ok(()));
        }
    }
}

/// Reports one source's life to `Shared`.
struct Tracking {
    shared: Arc<Shared>,
    cancel: Arc<AtomicBool>,
    duration_ms: u64,
    sample_rate: u32,
}

impl SourceHooks for Tracking {
    fn started(&mut self) {
        self.shared
            .duration_ms
            .store(self.duration_ms, Ordering::Relaxed);
        self.shared
            .sample_rate
            .store(self.sample_rate, Ordering::Relaxed);
        self.shared.started_sources.fetch_add(1, Ordering::Relaxed);
    }

    fn progressed(&mut self, frames: u64) {
        let ms = frames * 1000 / self.sample_rate.max(1) as u64;
        self.shared.position_ms.store(ms, Ordering::Relaxed);
    }

    fn finished(&mut self) {}

    fn seeked(&mut self, frames: u64) {
        self.progressed(frames);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

fn load_into(
    state: &mut AudioState,
    shared: &Arc<Shared>,
    eq: &Arc<EqShared>,
    path: &Path,
    replace: bool,
    start_at: Option<Duration>,
) -> AppResult<()> {
    let mut decoder = decoder_for(path)?;
    if let Some(start) = start_at.filter(|start| *start > Duration::ZERO) {
        // Best effort. A format that cannot seek starts from the top.
        let _ = decoder.try_seek(start);
    }
    let duration = decoder
        .total_duration()
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let rate = decoder.sample_rate();
    let sample_rate = if rate == 0 { 48_000 } else { rate };
    if replace {
        state.fresh_sink()?;
        state.appended = None;
        state.duration_ms = duration;
        state.last_pos = Duration::ZERO;
        state.last_advance = Instant::now();
        shared.duration_ms.store(duration, Ordering::Relaxed);
        shared.position_ms.store(
            start_at.map_or(0, |d| d.as_millis() as u64),
            Ordering::Relaxed,
        );
        shared.sample_rate.store(sample_rate, Ordering::Relaxed);
        shared.started_sources.store(0, Ordering::Relaxed);
    }
    let cancel = Arc::new(AtomicBool::new(false));
    if !replace {
        // One append outstanding at a time; a newer one replaces the older.
        if let Some(old) = state.appended.replace(Arc::clone(&cancel)) {
            old.store(true, Ordering::Relaxed);
        }
    }
    let Some(sink) = state.sink() else {
        return Err(AppError::msg(NO_DEVICE));
    };
    let hooks = Tracking {
        shared: Arc::clone(shared),
        cancel,
        duration_ms: duration,
        sample_rate,
    };
    // Decode → EQ Source (float biquads) → sink. Volume / ReplayGain stay on the sink.
    let mut source = EqSource::new(decoder, Arc::clone(eq)).with_hooks(Box::new(hooks));
    if let Some(start) = start_at {
        source = source.starting_at(start);
    }
    sink.append(source);
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

fn publish(state: &AudioState, shared: &Shared) {
    let Some(sink) = state.sink() else {
        // No device: hold the last position, report paused, and do not look
        // empty, or the player would step to the next song.
        shared.playing.store(false, Ordering::Relaxed);
        return;
    };
    shared.empty.store(sink.empty(), Ordering::Relaxed);
    shared
        .playing
        .store(!sink.is_paused() && !sink.empty(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-second 440 Hz mono WAV.
    fn write_wav(path: &Path, seconds: u32) {
        let rate = 44_100u32;
        let frames = rate * seconds;
        let data_len = frames * 2;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let t = i as f32 / rate as f32;
            let sample =
                ((t * 440.0 * std::f32::consts::TAU).sin() * 0.05 * i16::MAX as f32) as i16;
            out.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, out).unwrap();
    }

    /// Needs a real output device. Run with:
    /// `cargo test --manifest-path src-tauri/Cargo.toml live_engine -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_engine_plays_seeks_and_stops_without_hanging() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("tone.wav");
        write_wav(&wav, 3);
        let engine = RodioEngine::new();
        engine.set_volume(0.2).unwrap();

        let started = Instant::now();
        engine.set_uri(&wav).unwrap();
        engine.play().unwrap();
        std::thread::sleep(Duration::from_millis(600));
        assert!(engine.is_playing(), "should be playing");
        assert!(engine.position_ms() > 200, "position should advance");
        // Rodio's WAV duration is approximate; the player takes the longer
        // of this and the tag duration.
        assert!(engine.duration_ms() > 0);

        engine.seek(2_000).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(engine.position_ms() >= 2_000);

        // Replacing the song and stopping must both come back quickly.
        engine.set_uri(&wav).unwrap();
        engine.play().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        engine.stop().unwrap();
        assert!(!engine.is_playing());
        assert!(engine.is_empty());

        // Position is file time: half a second at 1x then half at 2x is
        // about 1.5 s into the file, not 1 s of wall clock.
        engine.set_uri(&wav).unwrap();
        engine.play().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        engine.set_speed(2.0).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let pos = engine.position_ms();
        assert!((1_200..1_900).contains(&pos), "file position was {pos}");

        // Gapless: append, let the first finish at 2x, the second takes over
        // inside the sink and the hop is counted.
        engine.set_uri(&wav).unwrap();
        engine.set_gapless_next(Some(&wav)).unwrap();
        engine.play().unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(engine.started_sources(), 1);
        std::thread::sleep(Duration::from_millis(1_500));
        assert_eq!(engine.started_sources(), 2, "second source took over");
        assert!(engine.is_playing());
        assert!(
            engine.position_ms() < 1_500,
            "position restarted for song 2"
        );

        // A cancelled append yields nothing, so the sink runs dry instead.
        engine.set_uri(&wav).unwrap();
        engine.set_gapless_next(Some(&wav)).unwrap();
        engine.cancel_gapless_next().unwrap();
        engine.play().unwrap();
        std::thread::sleep(Duration::from_millis(1_900));
        assert!(engine.is_empty(), "cancelled append must not play");
        assert_eq!(engine.started_sources(), 1);
        assert!(
            started.elapsed() < Duration::from_secs(12),
            "no call blocked"
        );
    }
}
