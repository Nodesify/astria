// astria-audio: audio/video transcription for knowledge graph ingestion.
//
// External-binary mode, like `add --postgres` requiring `psql`: transcription
// runs in `whisper-cli` (whisper.cpp) and video demuxing in `ffmpeg`. Nothing
// is vendored and no API key is involved — when the binaries or a model file
// are missing, callers get an actionable `TranscribeError::Unavailable` and
// the pipeline skips the file with a notice instead of breaking the build.
//
// Output shape mirrors astria-pdf: transcript text is wrapped as markdown
// (`# <filename>` + paragraphs) so the document extractor can consume it.

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use astria_core::env_var;

/// Hard cap on one downloaded media file (bytes). yt-dlp gets `--max-filesize`
/// and the produced file is re-checked — the advisory flag is not trusted
/// alone.
const MAX_MEDIA_BYTES: usize = 200 * 1024 * 1024;
/// Wall-clock deadline for one yt-dlp run; a wedged download is killed
/// rather than waited on forever.
const MEDIA_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Wall-clock deadline for one ffmpeg demux. Decoding the audio track of the
/// largest file that reaches it takes a small fraction of this; the bound is
/// for a tool that has stopped making progress.
const DEMUX_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Transcription allowance every file gets before its size counts (see
/// `transcription_allowance`).
const TRANSCRIBE_FLOOR: Duration = Duration::from_secs(30 * 60);

/// Why a media file could not be transcribed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscribeError {
    /// Tooling or model missing. The message names what to install or set;
    /// callers print it once per run and skip the file.
    Unavailable(String),
    /// A tool ran and failed (unreadable media, nonzero exit, no output).
    Failed(String),
}

impl std::fmt::Display for TranscribeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranscribeError::Unavailable(message) => write!(f, "{message}"),
            TranscribeError::Failed(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for TranscribeError {}

/// The external tools located on PATH.
#[derive(Debug, Clone)]
pub struct Tooling {
    /// `whisper-cli` (whisper.cpp) — the transcriber. Required.
    pub whisper: PathBuf,
    /// `ffmpeg` — demuxes video audio tracks and resamples audio to the
    /// 16 kHz mono PCM whisper-cli expects. Optional: audio-only repos
    /// transcribe without it (whisper.cpp decodes common audio formats).
    pub ffmpeg: Option<PathBuf>,
}

impl Tooling {
    /// True when `path` has a video container extension (needs ffmpeg demux).
    pub fn is_video(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                let lower = e.to_lowercase();
                astria_core::VIDEO_EXTENSIONS.contains(&format!(".{lower}").as_str())
            })
            .unwrap_or(false)
    }

    /// Locate the tools on this machine's PATH.
    pub fn probe() -> Result<Tooling, TranscribeError> {
        let path_var = std::env::var_os("PATH").unwrap_or_default();
        let dirs = std::env::split_paths(&path_var).collect::<Vec<_>>();
        Self::probe_in(dirs.iter().cloned())
    }

    /// `probe` against an explicit directory list (testable PATH stand-in).
    pub fn probe_in<I: Iterator<Item = PathBuf>>(dirs: I) -> Result<Tooling, TranscribeError> {
        let dirs: Vec<PathBuf> = dirs.collect();
        let suffix = std::env::consts::EXE_SUFFIX;
        match locate(dirs.iter().cloned(), "whisper-cli", suffix) {
            Some(whisper) => Ok(Tooling {
                whisper,
                ffmpeg: locate(dirs.iter().cloned(), "ffmpeg", suffix),
            }),
            None => Err(TranscribeError::Unavailable(
                "whisper-cli (whisper.cpp) not found on PATH - video/audio files are skipped. \
                 Install whisper.cpp to enable transcription: https://github.com/ggml-org/whisper.cpp"
                    .into(),
            )),
        }
    }
}

/// Transcribe a media file into markdown (astria-pdf output shape).
///
/// `project_root` anchors the per-project model search path
/// (`<root>/.astria/models/`).
pub fn transcribe_to_markdown(path: &Path, project_root: &Path) -> Result<String, TranscribeError> {
    let tooling = Tooling::probe()?;
    let model = resolve_model(project_root)?;

    // Scratch dir for the demuxed wav + whisper txt output; removed on drop.
    let scratch = scratch_dir(path)?;
    let _guard = ScratchDirGuard {
        path: scratch.clone(),
    };

    // Video always needs ffmpeg to demux the audio track. Audio goes through
    // ffmpeg too when available (guaranteed 16 kHz mono PCM); without it,
    // whisper-cli still decodes common audio formats itself.
    let wav = if Tooling::is_video(path) || tooling.ffmpeg.is_some() {
        let Some(ffmpeg) = &tooling.ffmpeg else {
            return Err(TranscribeError::Unavailable(
                "ffmpeg not found on PATH - required to demux the audio track of video files \
                 (.mp4, .mov, ...). Audio files transcribe without it. \
                 Install ffmpeg: https://ffmpeg.org/download.html"
                    .into(),
            ));
        };
        demux(ffmpeg, path, &scratch)?
    } else {
        path.to_path_buf()
    };

    let text = run_whisper(&tooling.whisper, &model, &wav, &scratch)?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("recording");
    Ok(transcript_markdown(filename, &text))
}

/// Resolve the whisper ggml model file: `$ASTRIA_WHISPER_MODEL`, then the
/// alphabetically first `*.bin` in `<root>/.astria/models/`, then in
/// `~/.astria/models/`.
pub fn resolve_model(project_root: &Path) -> Result<PathBuf, TranscribeError> {
    resolve_model_in(
        env_var("WHISPER_MODEL"),
        Some(&project_root.join(".astria/models")),
        std::env::home_dir().map(|h| h.join(".astria/models")),
    )
}

/// `resolve_model` with injected inputs (env value, project model dir, home
/// model dir) so the search order is testable without process state.
fn resolve_model_in(
    env_model: Option<String>,
    project_models: Option<&Path>,
    home_models: Option<PathBuf>,
) -> Result<PathBuf, TranscribeError> {
    if let Some(model) = env_model {
        let model = PathBuf::from(model);
        if model.is_file() {
            return Ok(model);
        }
        return Err(TranscribeError::Unavailable(format!(
            "ASTRIA_WHISPER_MODEL points to {}, which does not exist",
            model.display()
        )));
    }
    if let Some(dir) = project_models {
        if let Some(model) = first_model_bin(dir) {
            return Ok(model);
        }
    }
    if let Some(dir) = home_models {
        if let Some(model) = first_model_bin(&dir) {
            return Ok(model);
        }
    }
    Err(TranscribeError::Unavailable(
        "no Whisper ggml model found. Set ASTRIA_WHISPER_MODEL to a .bin model file, or place \
         one in <root>/.astria/models/ or ~/.astria/models/ - e.g. ggml-base.en.bin from \
         https://huggingface.co/ggml-org/whisper.cpp (~148 MB)"
            .into(),
    ))
}

/// Alphabetically first `*.bin` in `dir`, or None when the dir is absent
/// or holds no model files.
fn first_model_bin(dir: &Path) -> Option<PathBuf> {
    let mut bins: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("bin"))
                .unwrap_or(false)
        })
        .collect();
    bins.sort();
    bins.into_iter().next()
}

/// Find `name` (plus `exe_suffix` where the platform uses one) on a PATH-style
/// directory list. Pure PATH scan — no process spawn, no new dependency.
fn locate<I: Iterator<Item = PathBuf>>(dirs: I, name: &str, exe_suffix: &str) -> Option<PathBuf> {
    for dir in dirs {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !exe_suffix.is_empty() {
            let suffixed = dir.join(format!("{name}{exe_suffix}"));
            if suffixed.is_file() {
                return Some(suffixed);
            }
        }
    }
    None
}

/// Demux/resample any media input to 16 kHz mono PCM wav via ffmpeg.
fn demux(ffmpeg: &Path, input: &Path, scratch: &Path) -> Result<PathBuf, TranscribeError> {
    let wav = scratch.join("audio.wav");
    let output = Command::new(ffmpeg)
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg("-vn")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg("-acodec")
        .arg("pcm_s16le")
        .arg(&wav)
        .stdout(Stdio::null())
        .stderr(create_log(&scratch.join("ffmpeg.log"))?)
        .spawn()
        .map_err(|e| TranscribeError::Failed(format!("failed to run ffmpeg: {e}")))?
        .bounded_output(
            &scratch.join("ffmpeg.log"),
            tool_limit(DEMUX_TIMEOUT),
            "ffmpeg",
        )?;
    if !output.status.success() {
        return Err(TranscribeError::Failed(format!(
            "ffmpeg demux failed ({}): {}",
            output.status, output.stderr
        )));
    }
    if !wav.is_file() {
        return Err(TranscribeError::Failed(
            "ffmpeg produced no audio track (does the file contain one?)".into(),
        ));
    }
    Ok(wav)
}

/// Run whisper-cli over a wav file and read back the plain-text transcript.
fn run_whisper(
    whisper: &Path,
    model: &Path,
    wav: &Path,
    scratch: &Path,
) -> Result<String, TranscribeError> {
    let out_base = scratch.join("transcript");
    let output = Command::new(whisper)
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav)
        .arg("-otxt")
        .arg("-of")
        .arg(&out_base)
        .stdout(Stdio::null())
        .stderr(create_log(&scratch.join("whisper.log"))?)
        .spawn()
        .map_err(|e| TranscribeError::Failed(format!("failed to run whisper-cli: {e}")))?
        .bounded_output(
            &scratch.join("whisper.log"),
            tool_limit(transcription_allowance(
                std::fs::metadata(wav).map(|m| m.len()).unwrap_or(0),
                is_wave(wav),
            )),
            "whisper-cli",
        )?;
    if !output.status.success() {
        return Err(TranscribeError::Failed(format!(
            "whisper-cli failed ({}): {}",
            output.status, output.stderr
        )));
    }
    let txt = scratch.join("transcript.txt");
    std::fs::read_to_string(&txt).map_err(|e| {
        TranscribeError::Failed(format!(
            "whisper-cli produced no transcript file ({}): {e}",
            txt.display()
        ))
    })
}

/// What a finished tool run left behind: how it exited, and the last of what
/// it wrote to standard error — the shape of `Command::output`.
struct ToolOutput {
    status: ExitStatus,
    stderr: String,
}

/// `Command::output` under a deadline, for a tool started with its standard
/// error going to `log`. A tool still running when the limit passes is
/// killed and reported, so a hostile media file cannot hold the build open.
trait BoundedOutput {
    fn bounded_output(
        self,
        log: &Path,
        limit: Option<Duration>,
        what: &str,
    ) -> Result<ToolOutput, TranscribeError>;
}

impl BoundedOutput for std::process::Child {
    fn bounded_output(
        mut self,
        log: &Path,
        limit: Option<Duration>,
        what: &str,
    ) -> Result<ToolOutput, TranscribeError> {
        let status = match limit {
            Some(limit) => wait_bounded(&mut self, Instant::now() + limit, what)?,
            None => self
                .wait()
                .map_err(|e| TranscribeError::Failed(format!("{what} wait failed: {e}")))?,
        };
        let stderr = stderr_tail(&file_tail(log, MAX_STDERR_BYTES));
        Ok(ToolOutput { status, stderr })
    }
}

/// Truncated, writable handle for a tool's error output. A file, not a
/// pipe: a pipe has to be read for as long as anything holds its write end,
/// a grandchild that outlives the tool included, and reading it would wait
/// on one.
fn create_log(log: &Path) -> Result<Stdio, TranscribeError> {
    let file = std::fs::File::create(log)
        .map_err(|e| TranscribeError::Failed(format!("cannot create {}: {e}", log.display())))?;
    Ok(file.into())
}

/// Last `keep` bytes of a file, or nothing when it cannot be read.
fn file_tail(path: &Path, keep: usize) -> Vec<u8> {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let start = file
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0)
        .saturating_sub(keep as u64);
    let mut bytes = Vec::new();
    if file.seek(std::io::SeekFrom::Start(start)).is_ok() {
        let _ = file.take(keep as u64).read_to_end(&mut bytes);
    }
    bytes
}

/// Wall-clock allowance for one whisper-cli run over `bytes` of input.
/// Transcription cannot be told from a hang by watching it, so the allowance
/// follows the work: `TRANSCRIBE_FLOOR`, then minutes per MB — three for the
/// uncompressed audio ffmpeg writes (a MB is 33 seconds, so five times
/// slower than real time) and twenty for compressed audio the tool decodes
/// itself, where a MB holds minutes. A CPU with a large model stays far
/// inside both; the bound is for a tool that has stopped making progress.
fn transcription_allowance(bytes: u64, pcm: bool) -> Duration {
    const MINUTES_PER_PCM_MB: f64 = 3.0;
    const MINUTES_PER_COMPRESSED_MB: f64 = 20.0;
    let megabytes = bytes as f64 / (1024.0 * 1024.0);
    let minutes_per_mb = if pcm {
        MINUTES_PER_PCM_MB
    } else {
        MINUTES_PER_COMPRESSED_MB
    };
    TRANSCRIBE_FLOOR + Duration::from_secs((megabytes * minutes_per_mb * 60.0) as u64)
}

/// Whether `path` is a RIFF/WAVE file: uncompressed, so its size says how
/// much audio it holds.
fn is_wave(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

// Test seam for `tool_limit`: a limit forced regardless of the variable and
// the default, consumed by the next call on the thread. Nothing outside the
// tests sets it.
#[cfg(test)]
thread_local! {
    static FORCED_TOOL_LIMIT: std::cell::Cell<Option<Option<Duration>>> =
        const { std::cell::Cell::new(None) };
}

/// The limit for one tool run: `ASTRIA_TRANSCRIBE_TIMEOUT` where it holds a
/// number of seconds (`0` for none), else `default`.
fn tool_limit(default: Duration) -> Option<Duration> {
    #[cfg(test)]
    if let Some(limit) = FORCED_TOOL_LIMIT.with(std::cell::Cell::take) {
        return limit;
    }
    parse_limit(env_var("TRANSCRIBE_TIMEOUT").as_deref(), default)
}

/// A year at most: the limits become instants, and not every platform can
/// represent one arbitrarily far ahead.
const MAX_TOOL_LIMIT: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// `tool_limit` with the variable's value passed in, so the parsing is
/// testable without process state. Unrecognized values warn and fall back to
/// the default.
fn parse_limit(raw: Option<&str>, default: Duration) -> Option<Duration> {
    let Some(raw) = raw else {
        return Some(default);
    };
    match raw.trim().parse::<u64>() {
        Ok(0) => None,
        Ok(seconds) => Some(Duration::from_secs(seconds).min(MAX_TOOL_LIMIT)),
        Err(_) => {
            eprintln!(
                "warning: ASTRIA_TRANSCRIBE_TIMEOUT is not a number of seconds, the default \
                 deadlines apply: {raw:?}"
            );
            Some(default)
        }
    }
}

/// Shape a raw transcript into the markdown form the document extractors
/// parse: `# <filename>` header, then collapsed transcript paragraphs.
pub fn transcript_markdown(filename: &str, text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    let mut md = String::new();
    md.push_str("# ");
    md.push_str(filename);
    md.push_str("\n\n");
    for paragraph in text.split("\n\n") {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            continue;
        }
        md.push_str(&trimmed.replace('\n', " "));
        md.push('\n');
        md.push('\n');
    }
    md
}

/// Per-file scratch directory under the system temp dir, unique per process
/// and call; external tools write their outputs here. It is created new and
/// private: it holds the audio and the transcript of a file until the
/// attempt ends, and a directory that already exists is not ours to write
/// into.
fn scratch_dir(input: &Path) -> Result<PathBuf, TranscribeError> {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("media");
    let safe: String = stem
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "astria-transcribe-{}-{nanos}-{safe}",
        std::process::id()
    ));
    std::fs::create_dir(&dir)
        .map_err(|e| TranscribeError::Failed(format!("cannot create scratch dir: {e}")))?;
    #[cfg(unix)]
    if let Err(e) = make_private(&dir) {
        let _ = std::fs::remove_dir(&dir);
        return Err(TranscribeError::Failed(format!(
            "cannot create scratch dir: {e}"
        )));
    }
    Ok(dir)
}

/// Owner-only permissions on a directory that was just created.
#[cfg(unix)]
fn make_private(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// Removes the scratch dir when the transcription attempt ends.
struct ScratchDirGuard {
    path: PathBuf,
}

impl Drop for ScratchDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Last ~200 chars of tool stderr for error messages — long progress dumps
/// carry no extra information, the final lines name the failure.
fn stderr_tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    let tail: String = trimmed
        .chars()
        .rev()
        .take(200)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if tail.len() < trimmed.len() {
        format!("...{tail}")
    } else {
        tail.to_string()
    }
}

// ---------------------------------------------------------------------------
// Media URL download (yt-dlp)
// ---------------------------------------------------------------------------

/// Deadline for the metadata/resolution pass (`yt-dlp -J --simulate`).
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(120);
/// Cap on the `-J` metadata JSON read into memory (playlists are excluded,
/// but hostile metadata must not balloon the process either).
const MAX_INFO_JSON_BYTES: usize = 4 * 1024 * 1024;

/// Locate yt-dlp on PATH (Windows `.exe` suffix included).
pub fn locate_ytdlp() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if cfg!(windows) {
            let candidate = dir.join("yt-dlp.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        let candidate = dir.join("yt-dlp");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Deterministic, filesystem-safe slug for a media URL (used as the output
/// file stem so repeated ingests of the same URL overwrite in place).
pub fn slug_from_url(url: &str) -> String {
    let mut slug: String = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    slug.truncate(80);
    slug
}

/// Wait for a spawned child under a wall-clock deadline, killing it when
/// the budget runs out. A wedged child must be killed, not waited on.
fn wait_bounded(
    child: &mut std::process::Child,
    deadline: std::time::Instant,
    what: &str,
) -> Result<std::process::ExitStatus, TranscribeError> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(TranscribeError::Failed(format!(
                        "{what} exceeded its deadline"
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(e) => return Err(TranscribeError::Failed(format!("{what} wait failed: {e}"))),
        }
    }
}

/// Read a child's pipe on its own thread while the child runs, and hand
/// back what it wrote once it has exited. A pipe holds only a few KiB, so a
/// child whose output runs past that blocks on the full pipe and never
/// exits: waiting first and reading afterwards deadlocks until the deadline
/// kills it. Reading stops at `limit` bytes and the pipe closes with the
/// reader, so a child that keeps writing fails instead of hanging.
fn read_bounded<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
    limit: u64,
) -> Option<std::thread::JoinHandle<Vec<u8>>> {
    pipe.map(|pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut limited = pipe.take(limit);
            let _ = limited.read_to_end(&mut bytes);
            bytes
        })
    })
}

/// As `read_bounded`, but drains everything and keeps only the last `keep`
/// bytes: a child that talks past that is neither blocked nor cut short.
fn read_tail<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
    keep: usize,
) -> Option<std::thread::JoinHandle<Vec<u8>>> {
    pipe.map(|pipe| {
        std::thread::spawn(move || {
            let mut kept = Vec::with_capacity(keep);
            let mut chunk = [0u8; 8192];
            let mut pipe = std::io::BufReader::new(pipe);
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        kept.extend_from_slice(&chunk[..n]);
                        if kept.len() > keep {
                            let excess = kept.len() - keep;
                            kept.drain(..excess);
                        }
                    }
                }
            }
            kept
        })
    })
}

/// How much of a tool's stderr is kept for an error message: the tail of
/// what it wrote, and never more than this.
const MAX_STDERR_BYTES: usize = 64 * 1024;

/// Resolve where the media WOULD download from (`yt-dlp -J --simulate`, one
/// metadata request, no bytes moved) and hand every post-redirect,
/// post-format-selection media URL to the caller's vetting policy. The
/// downloader does its own DNS resolution and redirect following — this
/// pass makes the destination visible to the caller's SSRF rules BEFORE
/// the download runs; a URL the policy rejects fails the ingest. Returns
/// the vetted final URLs (also used to warn when metadata carries none).
fn resolve_and_vet_media_urls(
    ytdlp: &Path,
    url: &str,
    vet_url: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<String>, TranscribeError> {
    let mut command = std::process::Command::new(ytdlp);
    command
        .args([
            "-J",
            "--simulate",
            "--no-playlist",
            "--quiet",
            "--no-warnings",
        ])
        .arg(url)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| TranscribeError::Failed(format!("yt-dlp spawn failed: {e}")))?;
    // Drain while the child runs (see `read_bounded`): a single video's
    // metadata runs to hundreds of KiB, far past what a pipe holds.
    let reader = read_bounded(child.stdout.take(), MAX_INFO_JSON_BYTES as u64);
    let status = wait_bounded(
        &mut child,
        std::time::Instant::now() + RESOLVE_TIMEOUT,
        "yt-dlp media resolution",
    )?;
    let stdout_bytes = reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    if !status.success() {
        return Err(TranscribeError::Failed(format!(
            "yt-dlp could not resolve the media URL: exit {}",
            status.code().unwrap_or(-1)
        )));
    }
    let info: serde_json::Value = serde_json::from_slice(&stdout_bytes).map_err(|e| {
        TranscribeError::Failed(format!("unparseable yt-dlp metadata for {url}: {e}"))
    })?;

    let mut candidates: Vec<String> = Vec::new();
    if let Some(top) = info.get("url").and_then(|v| v.as_str()) {
        candidates.push(top.to_string());
    }
    if let Some(list) = info.get("requested_downloads").and_then(|v| v.as_array()) {
        for entry in list {
            if let Some(u) = entry.get("url").and_then(|v| v.as_str()) {
                candidates.push(u.to_string());
            }
        }
    }
    if let Some(formats) = info.get("formats").and_then(|v| v.as_array()) {
        for entry in formats.iter().take(20) {
            if let Some(u) = entry.get("url").and_then(|v| v.as_str()) {
                candidates.push(u.to_string());
            }
        }
    }
    if candidates.is_empty() {
        // Fail closed: with no resolved destination there is nothing the
        // policy could have checked, and the download must not fly blind.
        return Err(TranscribeError::Failed(format!(
            "yt-dlp metadata for {url} carries no media URL; refusing an unvetted download"
        )));
    }
    let mut vetted = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if !candidate.starts_with("http://") && !candidate.starts_with("https://") {
            return Err(TranscribeError::Failed(format!(
                "resolved media URL uses a disallowed scheme: {candidate}"
            )));
        }
        if let Some(reason) = vet_url(&candidate) {
            return Err(TranscribeError::Failed(format!(
                "resolved media URL rejected by the network policy: {reason}"
            )));
        }
        vetted.push(candidate);
    }
    Ok(vetted)
}

/// Download a video/audio URL to `out_dir` via yt-dlp. With ffmpeg present
/// the result is a 16 kHz mono WAV (ready for whisper-cli); without it the
/// raw bestaudio track is saved for a later run once ffmpeg is installed.
///
/// Security posture: the DESTINATION is vetted before any media byte moves
/// — yt-dlp first runs in `--simulate` metadata mode and every resolved
/// (post-redirect) media URL must pass `vet_url`. The subsequent download
/// still executes inside yt-dlp with its own resolver (there is no way to
/// pin its transport without a local proxy); the bounds below — one file
/// per URL, a size cap, a socket timeout, and a wall-clock deadline —
/// constrain that residual window, and the vetted resolution closes the
/// redirect-to-internal-network path at decision time.
pub fn download_media_to(
    url: &str,
    out_dir: &Path,
    vet_url: &dyn Fn(&str) -> Option<String>,
) -> Result<PathBuf, TranscribeError> {
    let ytdlp = locate_ytdlp().ok_or_else(|| {
        TranscribeError::Unavailable(
            "yt-dlp not found on PATH - media URLs cannot be downloaded. Install yt-dlp: https://github.com/yt-dlp/yt-dlp"
                .to_string(),
        )
    })?;

    // Phase 1: resolve + vet the destination (no media bytes moved).
    resolve_and_vet_media_urls(&ytdlp, url, vet_url)?;

    std::fs::create_dir_all(out_dir)
        .map_err(|e| TranscribeError::Failed(format!("media download dir: {e}")))?;

    let slug = slug_from_url(url);
    // Only ffmpeg availability matters here; whisper is not involved yet.
    let ffmpeg = Tooling::probe().ok().and_then(|t| t.ffmpeg);
    let mut command = std::process::Command::new(&ytdlp);
    command.current_dir(out_dir);
    // Bounds applied to every run: never expand one URL into a playlist,
    // never move more than MAX_MEDIA_BYTES, never wait on a dead socket.
    // --quiet --no-progress keep stderr to error-sized output; `read_tail`
    // drains it while the child runs, so even a flood of errors cannot
    // block it on a full pipe.
    command.args([
        "--no-playlist",
        "--quiet",
        "--no-progress",
        "--max-filesize",
        &format!("{}m", MAX_MEDIA_BYTES / (1024 * 1024)),
        "--socket-timeout",
        "30",
    ]);
    command.stderr(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::null());
    if ffmpeg.is_some() {
        command.args([
            "-x",
            "--audio-format",
            "wav",
            "--postprocessor-args",
            "ffmpeg:-ac 1 -ar 16000",
        ]);
        command.arg("-o").arg(format!("{slug}.wav"));
    } else {
        command.args(["-f", "bestaudio"]);
        command.arg("-o").arg(format!("{slug}.%(ext)s"));
    }
    command.arg(url);

    let mut child = command
        .spawn()
        .map_err(|e| TranscribeError::Failed(format!("yt-dlp spawn failed: {e}")))?;
    let reader = read_tail(child.stderr.take(), MAX_STDERR_BYTES);
    let status = wait_bounded(
        &mut child,
        std::time::Instant::now() + MEDIA_DOWNLOAD_TIMEOUT,
        "yt-dlp download",
    )?;
    let stderr_bytes = reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    if !status.success() {
        return Err(TranscribeError::Failed(format!(
            "yt-dlp failed: {}",
            stderr_tail(&stderr_bytes)
        )));
    }

    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    let entries =
        std::fs::read_dir(out_dir).map_err(|e| TranscribeError::Failed(format!("{e}")))?;
    for entry in entries {
        let entry = entry.map_err(|e| TranscribeError::Failed(format!("{e}")))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&slug) {
            // Enforce the size cap ourselves: --max-filesize is advisory
            // for some formats/extractors, the produced file is the truth.
            let size = entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
            if size > MAX_MEDIA_BYTES as u64 {
                let _ = std::fs::remove_file(entry.path());
                return Err(TranscribeError::Failed(format!(
                    "downloaded media exceeds {} MB limit",
                    MAX_MEDIA_BYTES / (1024 * 1024)
                )));
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                newest = Some((modified, entry.path()));
            }
        }
    }
    newest
        .map(|(_, p)| p)
        .ok_or_else(|| TranscribeError::Failed("yt-dlp produced no file".to_string()))
}

#[cfg(test)]
mod tests {

    #[test]
    fn media_slug_is_deterministic_and_safe() {
        let a = slug_from_url("https://youtu.be/dQw4w9WgXcQ?si=x");
        let b = slug_from_url("https://youtu.be/dQw4w9WgXcQ?si=x");
        assert_eq!(a, b);
        assert!(!a.contains('/'));
        assert!(!a.contains(':'));
        assert!(a.len() <= 80);
    }

    #[test]
    fn download_media_degrades_to_unavailable_without_ytdlp() {
        let dir = tempfile::tempdir().unwrap();
        match download_media_to("https://youtu.be/dQw4w9WgXcQ", dir.path(), &|_| None) {
            Err(TranscribeError::Unavailable(notice)) => {
                assert!(notice.contains("yt-dlp"), "notice: {notice}");
                assert!(notice.contains("https://github.com/yt-dlp/yt-dlp"));
            }
            // yt-dlp installed on this machine: a network/ffmpeg failure or a
            // real download are both acceptable local outcomes.
            Err(TranscribeError::Failed(_)) => {}
            Ok(_) => {}
        }
    }

    /// A stand-in for yt-dlp that answers with hundreds of KB of metadata:
    /// one vetted media URL and an array padded with blank lines. That is
    /// well past what a pipe holds, like real metadata — a single YouTube
    /// video's `-J` dump runs to hundreds of KiB.
    fn fake_ytdlp(dir: &Path) -> PathBuf {
        #[cfg(windows)]
        let path = {
            let path = dir.join("yt-dlp.bat");
            fs::write(
                &path,
                "@echo off\r\n\
                 echo {\"url\":\"https://example.com/a.mp3\",\"padding\":[\r\n\
                 for /L %%i in (1,1,7000) do @echo.&echo.&echo.&echo.&echo.\r\n\
                 echo ]}\r\n",
            )
            .unwrap();
            path
        };
        #[cfg(unix)]
        let path = {
            let path = dir.join("yt-dlp");
            fs::write(
                &path,
                "#!/bin/sh\n\
                 printf '%s' '{\"url\":\"https://example.com/a.mp3\",\"padding\":['\n\
                 yes \"\" | head -n 200000\n\
                 printf '%s' ']}'\n",
            )
            .unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        path
    }

    #[test]
    fn media_resolution_reads_metadata_larger_than_a_pipe() {
        let dir = tempfile::tempdir().unwrap();
        let fake = fake_ytdlp(dir.path());
        let started = std::time::Instant::now();
        let vetted = resolve_and_vet_media_urls(&fake, "https://example.com/watch?v=1", &|_| None)
            .expect("metadata larger than a pipe must still be resolved");
        assert_eq!(vetted, vec!["https://example.com/a.mp3".to_string()]);
        // Far under the resolution deadline: the point is not to wait on a
        // child that is blocked on a full pipe.
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "took {:?}",
            started.elapsed()
        );
    }
    use super::*;
    use std::fs;

    #[test]
    fn transcript_markdown_shapes_header_and_paragraphs() {
        let md = transcript_markdown(
            "standup.mp4",
            "Morning all.\n\nThis sprint we ship the\ngraph builder.\n\n",
        );
        assert_eq!(
            md,
            "# standup.mp4\n\nMorning all.\n\nThis sprint we ship the graph builder.\n\n"
        );
    }

    #[test]
    fn transcript_markdown_empty_text_is_empty() {
        assert_eq!(transcript_markdown("silence.wav", "  \n\n  "), "");
    }

    #[test]
    fn locate_finds_bare_and_suffixed_binaries() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("whisper-cli"), b"").unwrap();
        let found = locate(std::iter::once(dir.path().to_path_buf()), "whisper-cli", "");
        assert_eq!(
            found.as_deref(),
            Some(dir.path().join("whisper-cli")).as_deref()
        );

        let dir2 = tempfile::tempdir().unwrap();
        fs::write(dir2.path().join("whisper-cli.exe"), b"").unwrap();
        let found = locate(
            std::iter::once(dir2.path().to_path_buf()),
            "whisper-cli",
            ".exe",
        );
        assert_eq!(
            found.as_deref(),
            Some(dir2.path().join("whisper-cli.exe")).as_deref()
        );

        assert_eq!(
            locate(std::iter::once(dir2.path().to_path_buf()), "ffmpeg", ".exe"),
            None
        );
    }

    #[test]
    fn probe_in_without_whisper_is_unavailable_with_notice() {
        let empty = tempfile::tempdir().unwrap();
        let err = Tooling::probe_in(std::iter::once(empty.path().to_path_buf())).unwrap_err();
        match err {
            TranscribeError::Unavailable(msg) => {
                assert!(msg.contains("whisper-cli"), "{msg}");
                assert!(msg.contains("https://"), "{msg}");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn probe_in_finds_whisper_and_optional_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("whisper-cli"), b"").unwrap();
        let tooling = Tooling::probe_in(std::iter::once(dir.path().to_path_buf())).unwrap();
        assert_eq!(
            tooling.whisper,
            dir.path().join("whisper-cli"),
            "whisper found, ffmpeg absent"
        );
        assert!(tooling.ffmpeg.is_none());

        fs::write(dir.path().join("ffmpeg"), b"").unwrap();
        let tooling = Tooling::probe_in(std::iter::once(dir.path().to_path_buf())).unwrap();
        assert_eq!(tooling.ffmpeg, Some(dir.path().join("ffmpeg")));
    }

    #[test]
    fn is_video_matches_video_containers_only() {
        assert!(Tooling::is_video(Path::new("clip.MP4")));
        assert!(Tooling::is_video(Path::new("clip.webm")));
        assert!(!Tooling::is_video(Path::new("voice.mp3")));
        assert!(!Tooling::is_video(Path::new("notes.txt")));
    }

    #[test]
    fn resolve_model_prefers_env_then_project_then_home() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let project_models = project.path().join(".astria/models");
        let home_models = home.path().join(".astria/models");
        fs::create_dir_all(&project_models).unwrap();
        fs::create_dir_all(&home_models).unwrap();
        fs::write(project_models.join("z.bin"), b"").unwrap();
        fs::write(home_models.join("a.bin"), b"").unwrap();

        // No env: project dir wins over home, alphabetically first in dir.
        let model =
            resolve_model_in(None, Some(&project_models), Some(home_models.clone())).unwrap();
        assert_eq!(model, project_models.join("z.bin"));

        // Env override wins even when it points elsewhere.
        let elsewhere = tempfile::tempdir().unwrap();
        let env_model = elsewhere.path().join("ggml-tiny.bin");
        fs::write(&env_model, b"").unwrap();
        let model = resolve_model_in(
            Some(env_model.to_string_lossy().into_owned()),
            Some(&project_models),
            Some(home_models),
        )
        .unwrap();
        assert_eq!(model, env_model);

        // Env pointing at a missing file is an actionable Unavailable.
        let err = resolve_model_in(
            Some(
                elsewhere
                    .path()
                    .join("missing.bin")
                    .to_string_lossy()
                    .into_owned(),
            ),
            Some(&project_models),
            None,
        )
        .unwrap_err();
        assert!(
            matches!(err, TranscribeError::Unavailable(ref m) if m.contains("ASTRIA_WHISPER_MODEL")),
            "{err}"
        );

        // Nothing anywhere: actionable message naming every search path.
        let empty_project = tempfile::tempdir().unwrap();
        let empty_home = tempfile::tempdir().unwrap();
        let err = resolve_model_in(
            None,
            Some(&empty_project.path().join(".astria/models")),
            Some(empty_home.path().join(".astria/models")),
        )
        .unwrap_err();
        assert!(
            matches!(err, TranscribeError::Unavailable(ref m) if m.contains("ggml")),
            "{err}"
        );
    }

    #[test]
    fn first_model_bin_ignores_non_bin_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("readme.txt"), b"").unwrap();
        assert_eq!(first_model_bin(dir.path()), None);
        fs::write(dir.path().join("MODEL.BIN"), b"").unwrap();
        assert_eq!(
            first_model_bin(dir.path()),
            Some(dir.path().join("MODEL.BIN"))
        );
    }

    #[test]
    fn stderr_tail_truncates_long_output() {
        let long = "x".repeat(1000);
        let tail = stderr_tail(long.as_bytes());
        assert!(tail.starts_with("..."));
        assert!(tail.len() < 300);

        assert_eq!(stderr_tail(b"short"), "short");
    }

    /// A stand-in for a tool that has stopped making progress: it does
    /// nothing for a long time, the shape of a decoder wedged on a crafted
    /// file.
    fn stalled_tool(dir: &Path, name: &str) -> PathBuf {
        #[cfg(windows)]
        let path = {
            let path = dir.join(format!("{name}.bat"));
            fs::write(&path, "@echo off\r\nping -n 30 127.0.0.1 > nul\r\n").unwrap();
            path
        };
        #[cfg(unix)]
        let path = {
            let path = dir.join(name);
            fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        path
    }

    /// A stand-in for whisper-cli that writes the transcript where it was
    /// told to (`-of`, the seventh argument) and exits successfully.
    fn transcribing_whisper(dir: &Path) -> PathBuf {
        #[cfg(windows)]
        let path = {
            let path = dir.join("whisper-cli.bat");
            fs::write(
                &path,
                "@echo off\r\necho a transcript> \"%7.txt\"\r\nexit /b 0\r\n",
            )
            .unwrap();
            path
        };
        #[cfg(unix)]
        let path = {
            let path = dir.join("whisper-cli");
            fs::write(&path, "#!/bin/sh\nprintf 'a transcript\\n' > \"$7.txt\"\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        path
    }

    /// A stand-in for whisper-cli that fails and says why on standard error.
    fn failing_whisper(dir: &Path) -> PathBuf {
        #[cfg(windows)]
        let path = {
            let path = dir.join("whisper-cli.bat");
            fs::write(
                &path,
                "@echo off\r\necho boom: bad audio 1>&2\r\nexit /b 3\r\n",
            )
            .unwrap();
            path
        };
        #[cfg(unix)]
        let path = {
            let path = dir.join("whisper-cli");
            fs::write(&path, "#!/bin/sh\necho 'boom: bad audio' >&2\nexit 3\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        path
    }

    /// The next tool run on this thread gets `limit`, whatever the defaults
    /// and the variable say.
    fn force_tool_limit(limit: Duration) {
        FORCED_TOOL_LIMIT.with(|forced| forced.set(Some(Some(limit))));
    }

    fn scratch_under(dir: &Path) -> PathBuf {
        let scratch = dir.join("scratch");
        fs::create_dir(&scratch).unwrap();
        scratch
    }

    #[test]
    fn demux_kills_a_stalled_ffmpeg_at_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("clip.mp4");
        let scratch = scratch_under(dir.path());
        force_tool_limit(Duration::from_secs(1));
        let started = Instant::now();
        let error = demux(&stalled_tool(dir.path(), "ffmpeg"), &input, &scratch).unwrap_err();
        assert_eq!(
            error,
            TranscribeError::Failed("ffmpeg exceeded its deadline".into())
        );
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the tool was not killed promptly: {:?}",
            started.elapsed()
        );
        assert!(!scratch.join("audio.wav").exists());
    }

    #[test]
    fn whisper_kills_a_stalled_run_at_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = scratch_under(dir.path());
        force_tool_limit(Duration::from_secs(1));
        let error = run_whisper(
            &stalled_tool(dir.path(), "whisper-cli"),
            &dir.path().join("model.bin"),
            &dir.path().join("audio.wav"),
            &scratch,
        )
        .unwrap_err();
        assert_eq!(
            error,
            TranscribeError::Failed("whisper-cli exceeded its deadline".into())
        );
        assert!(!scratch.join("transcript.txt").exists());
    }

    #[test]
    fn whisper_transcript_is_read_back_and_failures_keep_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.bin");
        let wav = dir.path().join("audio.wav");
        fs::write(&wav, b"RIFF").unwrap();

        let text = run_whisper(
            &transcribing_whisper(dir.path()),
            &model,
            &wav,
            &scratch_under(dir.path()),
        )
        .unwrap();
        assert_eq!(text.trim(), "a transcript");

        let failed = dir.path().join("failed");
        fs::create_dir(&failed).unwrap();
        let error = run_whisper(&failing_whisper(dir.path()), &model, &wav, &failed).unwrap_err();
        match error {
            TranscribeError::Failed(message) => {
                assert!(message.starts_with("whisper-cli failed"), "{message}");
                assert!(message.contains("boom: bad audio"), "{message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn transcription_allowance_follows_the_audio() {
        const MB: u64 = 1024 * 1024;
        let floor = TRANSCRIBE_FLOOR.as_secs();
        assert_eq!(transcription_allowance(0, true).as_secs(), floor);
        // 16 kHz mono PCM: a MB is 33 seconds of audio, worth 3 minutes.
        assert_eq!(
            transcription_allowance(10 * MB, true).as_secs(),
            floor + 10 * 180
        );
        // Compressed: a MB holds minutes of audio, worth 20.
        assert_eq!(
            transcription_allowance(10 * MB, false).as_secs(),
            floor + 10 * 1200
        );
        // The largest file admitted still comes to an end.
        let largest = transcription_allowance(MAX_MEDIA_BYTES as u64, false);
        assert!(largest < Duration::from_secs(7 * 24 * 60 * 60));
    }

    #[test]
    fn transcribe_limit_reads_the_variable() {
        assert_eq!(parse_limit(None, DEMUX_TIMEOUT), Some(DEMUX_TIMEOUT));
        assert_eq!(parse_limit(Some(" 0 "), DEMUX_TIMEOUT), None);
        assert_eq!(
            parse_limit(Some("90"), DEMUX_TIMEOUT),
            Some(Duration::from_secs(90))
        );
        // Not a number of seconds: the defaults stay.
        for raw in ["", "soon", "-5", "1e3"] {
            assert_eq!(
                parse_limit(Some(raw), DEMUX_TIMEOUT),
                Some(DEMUX_TIMEOUT),
                "{raw:?}"
            );
        }
        // No overflow in the platforms' clocks.
        assert_eq!(
            parse_limit(Some("18446744073709551615"), DEMUX_TIMEOUT),
            Some(MAX_TOOL_LIMIT)
        );
    }

    #[test]
    fn scratch_dir_is_new_and_private() {
        let dir = scratch_dir(Path::new("/meetings/standup.mp4")).unwrap();
        let _guard = ScratchDirGuard { path: dir.clone() };
        assert!(dir.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
    }

    /// End-to-end plumbing check, self-gated: runs only on machines with
    /// whisper-cli and a model installed (CI has neither and skips silently).
    /// Synthesizes a 16 kHz wav in-process, then transcribes it.
    #[test]
    fn transcribes_wav_end_to_end_when_tooling_present() {
        let tooling = match Tooling::probe() {
            Ok(t) => t,
            Err(TranscribeError::Unavailable(_)) => {
                eprintln!("skipping: whisper-cli not installed");
                return;
            }
            other => panic!("unexpected probe result: {other:?}"),
        };
        let project = tempfile::tempdir().unwrap();
        let model = match resolve_model(project.path()) {
            Ok(m) => m,
            Err(TranscribeError::Unavailable(_)) => {
                eprintln!("skipping: no whisper model installed");
                return;
            }
            other => panic!("unexpected model result: {other:?}"),
        };

        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("tone.wav");
        write_sine_wav(&wav);

        // Direct whisper path (wav input, no demux needed).
        let scratch = scratch_dir(&wav).unwrap();
        let _guard = ScratchDirGuard {
            path: scratch.clone(),
        };
        let text = run_whisper(&tooling.whisper, &model, &wav, &scratch).unwrap();
        let _ = transcript_markdown("tone.wav", &text);

        // Full public path: with ffmpeg present this demuxes; without, the
        // wav goes straight to whisper-cli.
        let md = transcribe_to_markdown(&wav, project.path());
        match md {
            Ok(_) => {}
            Err(TranscribeError::Failed(msg)) if msg.contains("ffmpeg") => {
                eprintln!("skipping demux leg: {msg}");
            }
            Err(e) => panic!("transcription failed: {e}"),
        }
    }

    /// 1 s 440 Hz 16 kHz mono 16-bit PCM wav, written by hand (std only).
    fn write_sine_wav(path: &Path) {
        let sample_rate = 16_000u32;
        let samples = sample_rate as usize;
        let data_len = (samples * 2) as u32;
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_len).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&16u16.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..samples {
            let t = i as f32 / sample_rate as f32;
            let value = (t * 440.0).sin() * 0.3;
            buf.extend_from_slice(&((value * 32767.0) as i16).to_le_bytes());
        }
        fs::write(path, &buf).unwrap();
    }
}
