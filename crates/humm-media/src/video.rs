//! ffmpeg-backed [`VideoStreamer`]/[`FrameStream`] (F-083).
//!
//! Ported from Buzz's `FfmpegFrameReader`/`FfmpegVideoPlayer`
//! (`ffmpeg_video_player.py:82-271`): a background thread spawns
//! `ffmpeg -ss <s> -i <path> -f rawvideo -pix_fmt rgba -s WxH -r <fps> -an -`
//! and reads fixed-size RGBA frames into a bounded (`MAX_BUFFER`) deque with
//! synthesized timestamps `start + n·(1000/fps)`. Deliberate deviation from
//! Buzz (ADR — deviations ledger #10): `rgba` instead of `rgb24`, since egui
//! uploads RGBA textures natively and this saves a conversion.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use humm_core::boundary::{FrameStream, VideoFrame, VideoStreamInfo, VideoStreamer};
use humm_core::domain::Millis;
use humm_core::error::MediaError;

use crate::{binary_from_env, map_spawn_error};

/// Maximum buffered frames before the reader thread throttles
/// (`ffmpeg_video_player.py:98`, Buzz's `MAX_BUFFER`).
const MAX_BUFFER: usize = 60;

/// How long the reader thread sleeps between throttle-loop polls while the
/// buffer is full (`ffmpeg_video_player.py:139`, Buzz's `time.sleep(0.005)`).
const THROTTLE_POLL: Duration = Duration::from_millis(5);

/// Opens ffmpeg rawvideo `FrameStream`s (`ffmpeg_video_player.py:82-271`).
pub struct FfmpegVideoStreamer {
    binary: PathBuf,
}

impl FfmpegVideoStreamer {
    /// Uses `HUMM_FFMPEG` if set, else `ffmpeg` on `PATH`.
    pub fn new() -> Self {
        Self::with_binary(binary_from_env("HUMM_FFMPEG", "ffmpeg"))
    }

    /// Uses an explicit binary path/name, bypassing `HUMM_FFMPEG`.
    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        FfmpegVideoStreamer {
            binary: path.into(),
        }
    }
}

impl Default for FfmpegVideoStreamer {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoStreamer for FfmpegVideoStreamer {
    fn open(
        &self,
        path: &Path,
        start: Millis,
        info: &VideoStreamInfo,
    ) -> Result<Box<dyn FrameStream>, MediaError> {
        let stream = FfmpegFrameStream::spawn(&self.binary, path, start, *info)?;
        Ok(Box::new(stream))
    }
}

/// The pure frame-selection rule, factored out of the reader loop so it's
/// unit-testable without ffmpeg. Exact port of Buzz's
/// `get_frame_for_position` (`ffmpeg_video_player.py:177-200`): drain
/// entries more than one frame period stale while more than one remains
/// buffered, then pop-return the front entry if it's not more than one
/// frame period in the future, else leave the buffer untouched and report
/// no fresh frame.
///
/// Generic over the buffered payload (`T`) so tests exercise the rule with
/// cheap synthetic markers instead of real RGBA byte buffers.
pub(crate) fn select_frame<T>(
    buf: &mut VecDeque<(Millis, T)>,
    position: Millis,
    frame_ms: f64,
) -> Option<(Millis, T)> {
    if buf.is_empty() {
        return None;
    }

    while buf.len() > 1 {
        let ts = buf.front().expect("checked non-empty above").0;
        if (ts.0 as f64) < (position.0 as f64) - frame_ms {
            buf.pop_front();
        } else {
            break;
        }
    }

    let ts = buf.front()?.0;
    if (ts.0 as f64) <= (position.0 as f64) + frame_ms {
        buf.pop_front()
    } else {
        None
    }
}

/// State shared between the reader thread and the `FrameStream` handle.
struct SharedState {
    frames: Mutex<VecDeque<(Millis, Vec<u8>)>>,
}

/// A live `ffmpeg` rawvideo subprocess feeding a bounded frame buffer.
pub struct FfmpegFrameStream {
    binary: PathBuf,
    path: PathBuf,
    info: VideoStreamInfo,
    child: Arc<Mutex<Child>>,
    stop: Arc<AtomicBool>,
    state: Arc<SharedState>,
}

impl FfmpegFrameStream {
    fn spawn(
        binary: &Path,
        path: &Path,
        start: Millis,
        info: VideoStreamInfo,
    ) -> Result<Self, MediaError> {
        let start_secs = (start.0 as f64 / 1000.0).max(0.0);
        let mut child = Command::new(binary)
            .args(["-ss", &start_secs.to_string()])
            .arg("-i")
            .arg(path)
            .args(["-f", "rawvideo"])
            .args(["-pix_fmt", "rgba"])
            .args(["-s", &format!("{}x{}", info.width, info.height)])
            .args(["-r", &info.fps.to_string()])
            .arg("-an")
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| {
                map_spawn_error(
                    "ffmpeg",
                    "HUMM_FFMPEG",
                    "install it (e.g. `brew install ffmpeg`)",
                    binary,
                    err,
                )
            })?;

        let stdout = child.stdout.take().expect("stdout was piped at spawn");
        // RGBA: 4 bytes/pixel (deviations ledger #10; Buzz's rgb24 is 3).
        let frame_size = info.width as usize * info.height as usize * 4;

        let state = Arc::new(SharedState {
            frames: Mutex::new(VecDeque::new()),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(child));

        let reader_state = Arc::clone(&state);
        let reader_stop = Arc::clone(&stop);
        let reader_child = Arc::clone(&child);
        let fps = info.fps;
        thread::spawn(move || {
            read_loop(
                stdout,
                frame_size,
                fps,
                start,
                reader_state,
                reader_stop,
                reader_child,
            );
        });

        Ok(FfmpegFrameStream {
            binary: binary.to_path_buf(),
            path: path.to_path_buf(),
            info,
            child,
            stop,
            state,
        })
    }

    /// Signals the reader thread to stop and kills the subprocess, unblocking
    /// any in-flight blocking read. Idempotent — safe to call more than once
    /// (`seek` calls it explicitly, `Drop` calls it again on the way out).
    fn stop_current(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock() {
            // Single-PID `Child::kill()` is correct here (D5b-4 audit, M9):
            // this ffmpeg child is never spawned via
            // `crate::spawn_own_process_group` (unlike `demucs.rs`/
            // `fetch.rs`), so it stays in humm's own foreground process
            // group — a terminal Ctrl-C already reaches it directly, with
            // no detached group for a second Ctrl-C to orphan.
            let _ = child.kill();
        }
    }
}

impl FrameStream for FfmpegFrameStream {
    fn frame_for(&mut self, position: Millis) -> Option<VideoFrame> {
        let frame_ms = 1000.0 / self.info.fps;
        let mut frames = self.state.frames.lock().expect("frames lock poisoned");
        let (ts, rgba) = select_frame(&mut frames, position, frame_ms)?;
        Some(VideoFrame {
            ts,
            rgba,
            width: self.info.width,
            height: self.info.height,
        })
    }

    fn seek(&mut self, position: Millis) -> Result<(), MediaError> {
        // Buzz has no separate seek path either — `seek()` just calls
        // `start()` again (`ffmpeg_video_player.py:264-271`): kill the
        // current reader, respawn a fresh one at the new position.
        self.stop_current();
        *self = FfmpegFrameStream::spawn(&self.binary, &self.path, position, self.info)?;
        Ok(())
    }
}

impl Drop for FfmpegFrameStream {
    fn drop(&mut self) {
        self.stop_current();
    }
}

/// The reader thread body: spawns no subprocess itself (already spawned by
/// the caller) — reads fixed-size RGBA frames from `stdout` until a short
/// read/error/stop signal, buffering them with synthesized timestamps,
/// throttling while the buffer is full. Kills+waits the subprocess on the
/// way out, mirroring Buzz's `_read_loop` `finally` block
/// (`ffmpeg_video_player.py:174`).
fn read_loop(
    mut stdout: ChildStdout,
    frame_size: usize,
    fps: f64,
    start: Millis,
    state: Arc<SharedState>,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Child>>,
) {
    let frame_ms = 1000.0 / fps;
    let mut buf = vec![0u8; frame_size];
    let mut n: u64 = 0;

    'read: loop {
        // Throttle when the buffer is full (`ffmpeg_video_player.py:135-141`).
        loop {
            if stop.load(Ordering::SeqCst) {
                break 'read;
            }
            let len = state.frames.lock().expect("frames lock poisoned").len();
            if len < MAX_BUFFER {
                break;
            }
            thread::sleep(THROTTLE_POLL);
        }

        if stop.load(Ordering::SeqCst) {
            break;
        }

        match read_full_or_eof(&mut stdout, &mut buf) {
            Ok(read) if read == frame_size => {
                let ts = Millis(start.0 + ((n as f64) * frame_ms).round() as i64);
                state
                    .frames
                    .lock()
                    .expect("frames lock poisoned")
                    .push_back((ts, buf.clone()));
                n += 1;
            }
            // Short read (EOF mid-frame or clean EOF) or I/O error: done.
            _ => break,
        }
    }

    if let Ok(mut child) = child.lock() {
        // Single-PID is correct here too — see `stop_current`'s comment
        // above (D5b-4 audit, M9): same never-process-grouped ffmpeg child.
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Reads until `buf` is full or EOF, looping over short reads the way a
/// single Python `BufferedReader.read(n)` call would. Returns the number of
/// bytes actually read, which is `< buf.len()` only at EOF.
fn read_full_or_eof(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match reader.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(ref err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -- select_frame: exact port of ffmpeg_video_player.py:177-200 --------

    #[test]
    fn empty_buffer_yields_none() {
        let mut buf: VecDeque<(Millis, &str)> = VecDeque::new();
        assert_eq!(select_frame(&mut buf, Millis(0), 40.0), None);
    }

    #[test]
    fn single_frame_at_requested_position_is_returned_and_popped() {
        let mut buf = VecDeque::from([(Millis(0), "a")]);
        assert_eq!(
            select_frame(&mut buf, Millis(0), 40.0),
            Some((Millis(0), "a"))
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn stale_frames_are_drained_while_more_than_one_remains() {
        // frame_ms=40; position=100 -> drain threshold is ts < 60.
        let mut buf = VecDeque::from([(Millis(0), "a"), (Millis(40), "b")]);
        assert_eq!(
            select_frame(&mut buf, Millis(100), 40.0),
            Some((Millis(40), "b"))
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn future_frame_is_not_popped_and_none_is_returned() {
        let mut buf = VecDeque::from([(Millis(1000), "x")]);
        assert_eq!(select_frame(&mut buf, Millis(0), 40.0), None);
        // Not popped: still there for when position catches up.
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.front(), Some(&(Millis(1000), "x")));
    }

    #[test]
    fn lone_stale_frame_is_still_returned_when_nothing_newer_is_buffered() {
        // len==1 the whole time, so the drain loop never runs (Buzz's
        // `while len(self._frames) > 1` never fires) — the stale frame is
        // still the best available and is returned rather than discarded.
        let mut buf = VecDeque::from([(Millis(0), "only")]);
        assert_eq!(
            select_frame(&mut buf, Millis(10_000), 40.0),
            Some((Millis(0), "only"))
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn multiple_stale_frames_drain_down_to_the_first_non_stale_one() {
        let mut buf = VecDeque::from([
            (Millis(0), "a"),
            (Millis(40), "b"),
            (Millis(80), "c"),
            (Millis(120), "d"),
        ]);
        // position=200, frame_ms=40 -> drain threshold ts < 160: drains a,b,c.
        assert_eq!(
            select_frame(&mut buf, Millis(200), 40.0),
            Some((Millis(120), "d"))
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn frame_exactly_one_period_ahead_is_still_returned() {
        // ts <= position + frame_ms is inclusive.
        let mut buf = VecDeque::from([(Millis(140), "a")]);
        assert_eq!(
            select_frame(&mut buf, Millis(100), 40.0),
            Some((Millis(140), "a"))
        );
    }

    #[test]
    fn frame_just_past_one_period_ahead_is_not_returned() {
        let mut buf = VecDeque::from([(Millis(141), "a")]);
        assert_eq!(select_frame(&mut buf, Millis(100), 40.0), None);
        assert_eq!(buf.len(), 1);
    }

    // -- Constructor plumbing ------------------------------------------------

    #[test]
    fn with_binary_overrides_the_resolved_path() {
        let streamer = FfmpegVideoStreamer::with_binary("custom-ffmpeg");
        assert_eq!(streamer.binary, PathBuf::from("custom-ffmpeg"));
    }

    #[test]
    fn open_with_missing_binary_reports_actionable_error() {
        let streamer = FfmpegVideoStreamer::with_binary("/nonexistent/ffmpeg-binary-for-test");
        let info = VideoStreamInfo {
            width: 320,
            height: 240,
            fps: 25.0,
        };
        let err = match streamer.open(Path::new("/irrelevant"), Millis(0), &info) {
            Ok(_) => panic!("missing binary should fail to spawn"),
            Err(err) => err,
        };
        match err {
            MediaError::BinaryMissing { hint, .. } => {
                assert!(hint.contains("HUMM_FFMPEG"), "hint was: {hint}");
            }
            other => panic!("expected BinaryMissing, got {other:?}"),
        }
    }
}
