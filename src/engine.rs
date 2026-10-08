use crate::audio::{self, Sound, WHISPER_RATE};
use crate::corrector::{self, Correction};
use crate::formatter;
use crate::hooks;
use crate::output::Output;
use crate::settings::{Provider, Settings, Stage, cache_dir, data_dir, write_private};
use crate::transcribe::{self, LocalEngine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    PttDown(bool),
    PttUp,
    Toggle,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Recording,
    Transcribing,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KeyCheck {
    Unchecked,
    Checking,
    Valid,
    Invalid(String),
}

#[derive(Default)]
pub struct Missed {
    pub transcripts: usize,
    pub words: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum StepState {
    Ok,
    Skipped,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub label: String,
    pub detail: String,
    pub state: StepState,
}

#[derive(Default)]
pub struct JevStats {
    pub requests: usize,
    pub fixes: usize,
    pub last: Option<Instant>,
}

pub fn fmt_ms(ms: u64) -> String {
    if ms < 1000 { format!("{ms} ms") } else { format!("{:.1} s", ms as f64 / 1000.0) }
}

fn step(steps: &mut Vec<Step>, label: impl Into<String>, detail: impl Into<String>, state: StepState) {
    steps.push(Step { label: label.into(), detail: detail.into(), state });
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub time: chrono::DateTime<chrono::Local>,
    pub provider: Provider,
    pub model: String,
    pub duration_secs: f32,
    pub raw: String,
    pub text: String,
    pub corrections: Vec<Correction>,
    pub error: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub dict_text: String,
}

pub struct Shared {
    pub settings: RwLock<Settings>,
    pub history: Mutex<Vec<Entry>>,
    pub recording_since: Mutex<Option<Instant>>,
    pub jobs: AtomicUsize,
    pub notice: Mutex<Option<(String, bool)>>,
    pub models: Mutex<HashMap<Provider, Result<Vec<String>, String>>>,
    pub download: Mutex<Option<(String, u64, u64)>>,
    pub ctx: OnceLock<eframe::egui::Context>,
    pub http: reqwest::blocking::Client,
    pub local: LocalEngine,
    pub output: Output,
    pub cache: corrector::Cache,
    pub cmd: Mutex<Option<Sender<Cmd>>>,
    pub key_check: Mutex<KeyCheck>,
    pub missed: Mutex<Missed>,
    pub jev: Mutex<JevStats>,
}

fn history_path() -> std::path::PathBuf {
    data_dir().join("history.json")
}

impl Shared {
    pub fn new(settings: Settings) -> Arc<Self> {
        let history = std::fs::read_to_string(history_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Arc::new(Self {
            settings: RwLock::new(settings),
            history: Mutex::new(history),
            recording_since: Mutex::new(None),
            jobs: AtomicUsize::new(0),
            notice: Mutex::new(None),
            models: Mutex::new(HashMap::new()),
            download: Mutex::new(None),
            ctx: OnceLock::new(),
            http: reqwest::blocking::Client::builder().connect_timeout(Duration::from_secs(10)).build().unwrap_or_default(),
            local: LocalEngine::default(),
            output: Output::new(),
            cache: corrector::Cache::load(cache_dir().join("corrector-cache.json")),
            cmd: Mutex::new(None),
            key_check: Mutex::new(KeyCheck::Unchecked),
            missed: Mutex::new(Missed::default()),
            jev: Mutex::new(JevStats::default()),
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn phase(&self) -> Phase {
        if self.recording_since.lock().unwrap().is_some() {
            Phase::Recording
        } else if self.jobs.load(Ordering::SeqCst) > 0 {
            Phase::Transcribing
        } else {
            Phase::Idle
        }
    }

    pub fn send(&self, cmd: Cmd) {
        if let Some(tx) = self.cmd.lock().unwrap().as_ref() {
            let _ = tx.send(cmd);
        }
    }

    pub fn repaint(&self) {
        if let Some(ctx) = self.ctx.get() {
            ctx.request_repaint();
        }
    }

    pub fn set_notice(&self, text: impl Into<String>, error: bool) {
        let text = text.into();
        if error && self.settings.read().unwrap().notify_error {
            notify("Transcription error", &text);
        }
        *self.notice.lock().unwrap() = Some((text, error));
        self.repaint();
    }

    pub fn save_history(&self) {
        let limit = self.settings.read().unwrap().history_limit;
        let mut history = self.history.lock().unwrap();
        let excess = history.len().saturating_sub(limit);
        history.drain(..excess);
        if let Ok(bytes) = serde_json::to_vec(&*history) {
            let _ = write_private(&history_path(), &bytes);
        }
    }

    pub fn refresh_models(self: &Arc<Self>, provider: Provider) {
        let shared = self.clone();
        std::thread::spawn(move || {
            let result = transcribe::list_models(provider, &shared.settings(), &shared.http);
            shared.models.lock().unwrap().insert(provider, result);
            shared.repaint();
        });
    }

    pub fn check_key(self: &Arc<Self>, key: String, model: String, announce: bool) {
        *self.key_check.lock().unwrap() = KeyCheck::Checking;
        let shared = self.clone();
        std::thread::spawn(move || {
            let result = corrector::validate_key(&key, &model, &shared.http);
            if announce && result.is_ok() {
                shared.set_notice("Jev key works. Smart corrections are on.", false);
            }
            *shared.key_check.lock().unwrap() = match result {
                Ok(()) => KeyCheck::Valid,
                Err(e) => KeyCheck::Invalid(e),
            };
            shared.repaint();
        });
    }

    pub fn download_model(self: &Arc<Self>, name: String) {
        let shared = self.clone();
        std::thread::spawn(move || {
            *shared.download.lock().unwrap() = Some((name.clone(), 0, 0));
            let result = transcribe::download_model(&name, &shared.download);
            *shared.download.lock().unwrap() = None;
            match result {
                Ok(()) => shared.set_notice(format!("Downloaded {name}"), false),
                Err(e) => shared.set_notice(format!("Download of {name} failed: {e}"), true),
            }
        });
    }
}

pub fn notify(title: &str, body: &str) {
    let _ = notify_rust::Notification::new().summary(title).body(body).appname(crate::settings::APP_NAME).show();
}

fn play_if(enabled: bool, sound: Sound) {
    if enabled {
        audio::play(sound);
    }
}

pub fn spawn(shared: Arc<Shared>) {
    let (tx, rx) = std::sync::mpsc::channel();
    *shared.cmd.lock().unwrap() = Some(tx);
    std::thread::spawn(move || run(shared, rx));
}

const AUTOREPEAT_GRACE: Duration = Duration::from_millis(60);

fn run(shared: Arc<Shared>, rx: Receiver<Cmd>) {
    let mut recording: Option<audio::Recording> = None;
    let mut pending_stop: Option<Instant> = None;
    let mut format = false;
    loop {
        let timeout = pending_stop.map_or(Duration::from_secs(5), |t| t.saturating_duration_since(Instant::now()));
        let cmd = match rx.recv_timeout(timeout) {
            Ok(cmd) => Some(cmd),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let settings = shared.settings();
        match cmd {
            Some(Cmd::PttDown(_)) if pending_stop.take().is_some() => {}
            Some(Cmd::PttDown(f)) if recording.is_none() => {
                recording = start(&shared, &settings);
                format = f;
            }
            Some(Cmd::PttUp) if recording.is_some() => pending_stop = Some(Instant::now() + AUTOREPEAT_GRACE),
            Some(Cmd::Toggle) => match recording.take() {
                Some(rec) => stop(&shared, &settings, rec, format),
                None => {
                    recording = start(&shared, &settings);
                    format = false;
                }
            },
            Some(Cmd::Cancel) => {
                pending_stop = None;
                if recording.take().is_some() {
                    *shared.recording_since.lock().unwrap() = None;
                    play_if(settings.sound_cancel, Sound::Cancel);
                    shared.set_notice("Recording cancelled", false);
                }
            }
            Some(_) => {}
            None => {
                if pending_stop.take().is_some()
                    && let Some(rec) = recording.take() {
                        stop(&shared, &settings, rec, format);
                    }
                if settings.local_unload_minutes > 0 {
                    shared.local.unload_if_idle(Duration::from_secs(settings.local_unload_minutes as u64 * 60));
                }
            }
        }
    }
}

fn start(shared: &Arc<Shared>, settings: &Settings) -> Option<audio::Recording> {
    match audio::start(&settings.input_device) {
        Ok(rec) => {
            *shared.recording_since.lock().unwrap() = Some(Instant::now());
            play_if(settings.sound_start, Sound::Start);
            hooks::fire(&settings.scripts, Stage::RecordingStart, &[]);
            shared.repaint();
            Some(rec)
        }
        Err(e) => {
            shared.set_notice(format!("Couldn't start recording: {e}"), true);
            None
        }
    }
}

fn stop(shared: &Arc<Shared>, settings: &Settings, rec: audio::Recording, format: bool) {
    *shared.recording_since.lock().unwrap() = None;
    let (samples, elapsed) = rec.finish();
    play_if(settings.sound_stop, Sound::Stop);
    if elapsed < Duration::from_millis(settings.min_recording_ms as u64) || samples.is_empty() {
        shared.set_notice("Recording too short; ignored", false);
        return;
    }
    shared.jobs.fetch_add(1, Ordering::SeqCst);
    shared.repaint();
    let shared = shared.clone();
    let settings = settings.clone();
    std::thread::spawn(move || {
        let entry = pipeline(&shared, &settings, &samples, format);
        shared.jobs.fetch_sub(1, Ordering::SeqCst);
        if let Some(entry) = entry {
            shared.history.lock().unwrap().push(entry);
            shared.save_history();
        }
        shared.repaint();
    });
}

fn pipeline(shared: &Shared, s: &Settings, samples: &[f32], force_format: bool) -> Option<Entry> {
    let timeout = Duration::from_secs(s.script_timeout_secs.max(1) as u64);
    let mut warnings = Vec::new();
    let mut entry = Entry {
        time: chrono::Local::now(),
        provider: s.provider,
        model: s.model(s.provider).to_string(),
        duration_secs: samples.len() as f32 / WHISPER_RATE as f32,
        raw: String::new(),
        text: String::new(),
        corrections: Vec::new(),
        error: None,
        steps: Vec::new(),
        dict_text: String::new(),
    };
    let provider = s.provider.info().label;

    if hooks::has(&s.scripts, Stage::BeforeTranscription) {
        let path = std::env::temp_dir().join(format!("{}-{}.wav", crate::settings::APP_NAME, std::process::id()));
        if std::fs::write(&path, audio::wav_bytes(samples, WHISPER_RATE)).is_ok() {
            let audio_path = path.to_string_lossy().to_string();
            warnings.extend(hooks::notify(&s.scripts, Stage::BeforeTranscription, "", &[("WH_AUDIO", &audio_path)], timeout));
            let _ = std::fs::remove_file(&path);
        }
    }

    let started = Instant::now();
    match transcribe::transcribe(samples, s, &shared.http, &shared.local) {
        Ok(text) => {
            entry.raw = text;
            let ms = fmt_ms(started.elapsed().as_millis() as u64);
            step(&mut entry.steps, format!("{provider} · {ms}"), format!("Transcribed by {provider} ({}) in {ms}", entry.model), StepState::Ok);
        }
        Err(e) => {
            step(&mut entry.steps, format!("{provider} · failed"), e.clone(), StepState::Failed);
            shared.set_notice(&e, true);
            entry.error = Some(e);
            return Some(entry);
        }
    }
    if entry.raw.is_empty() {
        shared.set_notice("No speech detected", false);
        return None;
    }

    let mut text = entry.raw.clone();
    let key = s.typesafe_key();
    if !s.corrector_enabled {
        step(&mut entry.steps, "Jev · off", "Dictionary corrections are turned off in Settings → Dictionary", StepState::Skipped);
    } else if key.is_empty() {
        let words = corrector::trigger_words(&text, &s.dictionary);
        let detail = if words.is_empty() { "No Jev API key. This transcript had no dictionary words anyway.".to_string() } else { format!("No Jev API key, so these were not checked: {}", words.join(", ")) };
        step(&mut entry.steps, "Jev · no key", detail, StepState::Skipped);
        if !words.is_empty() {
            let mut missed = shared.missed.lock().unwrap();
            missed.transcripts += 1;
            for w in words {
                if missed.words.len() < 3 && !missed.words.iter().any(|m| m.eq_ignore_ascii_case(&w)) {
                    missed.words.push(w);
                }
            }
        }
    } else {
        let started = Instant::now();
        match corrector::correct(&text, &s.dictionary, &key, &s.corrector_model, &shared.cache, &shared.http) {
            Ok((corrected, corrections)) if corrections.is_empty() => {
                step(&mut entry.steps, "Jev · not needed", "Jev is ready, but no dictionary words appeared, so nothing was sent.", StepState::Skipped);
                text = corrected;
            }
            Ok((corrected, corrections)) => {
                let ms = started.elapsed().as_millis() as u64;
                let fixed = corrections.iter().filter(|c| c.to.is_some()).count();
                let asked = corrections.iter().filter(|c| !c.cached).count();
                let words: Vec<String> = corrections.iter().map(|c| match &c.to {
                    Some(to) => format!("{} → {to} ({:.0}%)", c.from, c.probability * 100.0),
                    None => format!("{} kept ({:.0}% {})", c.from, c.probability * 100.0, c.choice),
                }).collect();
                let source = match asked {
                    0 => "all answers from the local cache".to_string(),
                    n if n == corrections.len() => format!("asked Jev in {}", fmt_ms(ms)),
                    n => format!("asked Jev about {n} in {}, rest from cache", fmt_ms(ms)),
                };
                step(&mut entry.steps, format!("Jev · fixed {fixed} of {}", corrections.len()), format!("{}; {source}", words.join(", ")), StepState::Ok);
                let mut stats = shared.jev.lock().unwrap();
                stats.requests += (asked > 0) as usize;
                stats.fixes += fixed;
                stats.last = Some(Instant::now());
                drop(stats);
                entry.dict_text = corrected.clone();
                text = corrected;
                entry.corrections = corrections;
            }
            Err(e) => {
                step(&mut entry.steps, "Jev · error", e.clone(), StepState::Failed);
                warnings.push(format!("Dictionary skipped: {e}"));
            }
        }
    }
    match formatter::wanted(&text, force_format, s, &shared.http) {
        Ok((false, why)) if !why.is_empty() => step(&mut entry.steps, "Format · not asked", why, StepState::Skipped),
        Ok((false, _)) => {}
        Ok((true, why)) => {
            let started = Instant::now();
            match formatter::format(&text, s, &shared.http) {
                Ok(formatted) => {
                    let ms = fmt_ms(started.elapsed().as_millis() as u64);
                    step(&mut entry.steps, format!("Formatted · {ms}"), format!("{why}; {} formatted it in {ms}", s.format_model), StepState::Ok);
                    text = formatted;
                }
                Err(e) => {
                    step(&mut entry.steps, "Format · failed", e.clone(), StepState::Failed);
                    warnings.push(format!("Formatting skipped: {e}"));
                }
            }
        }
        Err(e) => {
            step(&mut entry.steps, "Format · check failed", e.clone(), StepState::Failed);
            warnings.push(format!("Format check skipped: {e}"));
        }
    }
    let (text, runs) = hooks::filter(&s.scripts, text, timeout);
    for run in runs {
        let ms = fmt_ms(run.ms);
        match run.error {
            None => step(&mut entry.steps, format!("{} · {ms}", run.name), format!("Script '{}' ran in {ms}", run.name), StepState::Ok),
            Some(e) => {
                step(&mut entry.steps, format!("{} · failed", run.name), e.clone(), StepState::Failed);
                warnings.push(e);
            }
        }
    }
    entry.text = text;

    let delivered = match (s.output_cursor, s.output_clipboard) {
        (true, _) if s.output_enter => "Pasted + Enter",
        (true, _) => "Pasted",
        (false, true) => "Copied",
        (false, false) => "Saved to history",
    };
    match shared.output.deliver(&entry.text, s) {
        Ok(()) => step(&mut entry.steps, delivered, format!("{delivered}{}", if s.output_cursor && s.output_clipboard { ", and left on the clipboard" } else { "" }), StepState::Ok),
        Err(e) => {
            step(&mut entry.steps, format!("{delivered} · failed"), e.clone(), StepState::Failed);
            warnings.push(e);
        }
    }
    play_if(s.sound_complete, Sound::Complete);
    if s.notify_complete {
        notify("Transcription complete", &entry.text);
    }
    warnings.extend(hooks::notify(&s.scripts, Stage::AfterDelivery, &entry.text, &[("WH_TEXT", &entry.text)], timeout));

    match warnings.is_empty() {
        true => *shared.notice.lock().unwrap() = None,
        false => {
            let joined = warnings.join("\n");
            shared.set_notice(&joined, true);
            entry.error = Some(joined);
        }
    }
    Some(entry)
}
