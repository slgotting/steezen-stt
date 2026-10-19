use crate::audio::{self, Sound, WHISPER_RATE};
use crate::corrector::{self, Correction};
use crate::formatter;
use crate::hooks;
use crate::output::Output;
use crate::settings::{Provider, Settings, Stage, cache_dir, data_dir, write_private};
use crate::latency::Latency;
use crate::transcribe::{self, Attempt, LocalEngine, Msg, Notice, Trigger};
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
    #[serde(default)]
    pub audio: Option<String>,
}

#[derive(Default)]
pub struct Probe {
    pub id: u64,
    pub running: bool,
    pub stop: bool,
    pub total: u32,
    pub key: String,
    pub runs: Vec<(Result<String, String>, Option<transcribe::Timing>)>,
}

pub struct ProbeSummary {
    pub count: usize,
    pub mean: f64,
    pub sd: f64,
    pub min: u64,
    pub max: u64,
}

impl Probe {
    pub fn summary(&self) -> Option<ProbeSummary> {
        let waits: Vec<u64> = self.runs.iter().filter(|(r, _)| r.is_ok()).filter_map(|(_, t)| t.map(|t| t.wait_ms)).collect();
        let n = waits.len() as f64;
        let mean = waits.iter().sum::<u64>() as f64 / n;
        let sd = (waits.iter().map(|&w| (w as f64 - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt();
        Some(ProbeSummary { count: waits.len(), mean, sd, min: *waits.iter().min()?, max: *waits.iter().max()? })
    }
}

pub fn hello_path() -> std::path::PathBuf {
    data_dir().join("hello.wav")
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
    pub local: Arc<LocalEngine>,
    pub latency: Arc<Latency>,
    pub output: Output,
    pub cache: corrector::Cache,
    pub cmd: Mutex<Option<Sender<Cmd>>>,
    pub key_check: Mutex<KeyCheck>,
    pub missed: Mutex<Missed>,
    pub jev: Mutex<JevStats>,
    pub probe: Mutex<Probe>,
}

fn history_path() -> std::path::PathBuf {
    data_dir().join("history.json")
}

pub fn recordings_dir() -> std::path::PathBuf {
    data_dir().join("recordings")
}

fn save_recording(samples: &[f32], keep: usize) -> Option<String> {
    if keep == 0 {
        return None;
    }
    let name = format!("{}.wav", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"));
    write_private(&recordings_dir().join(&name), &audio::wav_bytes(samples, WHISPER_RATE)).ok()?;
    let mut saved: Vec<_> = std::fs::read_dir(recordings_dir()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "wav")).collect();
    saved.sort();
    for old in &saved[..saved.len().saturating_sub(keep)] {
        let _ = std::fs::remove_file(old);
    }
    Some(name)
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
            local: Arc::new(LocalEngine::default()),
            latency: Arc::new(Latency::load()),
            output: Output::new(),
            cache: corrector::Cache::load(cache_dir().join("corrector-cache.json")),
            cmd: Mutex::new(None),
            key_check: Mutex::new(KeyCheck::Unchecked),
            missed: Mutex::new(Missed::default()),
            jev: Mutex::new(JevStats::default()),
            probe: Mutex::new(Probe::default()),
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

    pub fn test_baseline(self: &Arc<Self>, runs: u32, gap: Duration) {
        let settings = self.settings();
        let samples = match std::fs::read(hello_path()).map_err(|e| e.to_string()).and_then(|b| audio::read_wav(&b)) {
            Ok((samples, rate)) => audio::resample(&samples, rate, WHISPER_RATE),
            Err(e) => return self.set_notice(format!("Couldn't read the hello clip: {e}"), true),
        };
        let id = self.probe.lock().unwrap().id + 1;
        *self.probe.lock().unwrap() = Probe { id, running: true, stop: false, total: runs, key: crate::latency::key(settings.provider, settings.model(settings.provider)), runs: Vec::new() };
        let shared = self.clone();
        std::thread::spawn(move || {
            for i in 0..runs {
                if i > 0 {
                    let until = Instant::now() + gap;
                    while Instant::now() < until && !shared.probe.lock().unwrap().stop {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                if shared.probe.lock().unwrap().stop {
                    break;
                }
                let run = transcribe::timed_cloud(&samples, &settings);
                shared.probe.lock().unwrap().runs.push(run);
                shared.repaint();
            }
            shared.probe.lock().unwrap().running = false;
            shared.repaint();
        });
    }

    pub fn retry(self: &Arc<Self>, name: &str) {
        let settings = Settings { output_cursor: false, output_clipboard: true, ..self.settings() };
        match std::fs::read(recordings_dir().join(name)).map_err(|e| e.to_string()).and_then(|b| audio::read_wav(&b)) {
            Ok((samples, rate)) => process(self.clone(), settings, audio::resample(&samples, rate, WHISPER_RATE), false, Some(name.to_string())),
            Err(e) => self.set_notice(format!("Couldn't read recording {name}: {e}"), true),
        }
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
    let audio = save_recording(&samples, settings.keep_recordings);
    process(shared.clone(), settings.clone(), samples, format, audio);
}

fn process(shared: Arc<Shared>, settings: Settings, samples: Vec<f32>, format: bool, audio: Option<String>) {
    shared.jobs.fetch_add(1, Ordering::SeqCst);
    shared.repaint();
    std::thread::spawn(move || {
        let (entry, late) = pipeline(&shared, &settings, &samples, format, audio);
        shared.jobs.fetch_sub(1, Ordering::SeqCst);
        let time = entry.as_ref().map(|e| e.time);
        if let Some(entry) = entry {
            shared.history.lock().unwrap().push(entry);
            shared.save_history();
        }
        shared.repaint();
        for a in late.into_iter().flat_map(|rx| rx.into_iter()).filter_map(|m| match m {
            Msg::Done(a) if a.provider != Provider::Local => Some(a),
            _ => None,
        }) {
            let mut history = shared.history.lock().unwrap();
            if let Some(e) = history.iter_mut().rev().find(|e| Some(e.time) == time)
                && let Some(st) = e.steps.iter_mut().find(|st| st.label == waiting_label(&a.provider.info().label)) {
                    *st = attempt_step(&a, " · lost", "its reply arrived after another result won, so it was discarded");
                    if st.state == StepState::Ok {
                        st.state = StepState::Skipped;
                    }
                }
            drop(history);
            shared.save_history();
            shared.repaint();
        }
    });
}

fn waiting_label(name: &str) -> String {
    format!("{name} · still waiting")
}

fn timing_text(a: &Attempt) -> (String, String) {
    match a.timing {
        Some(t) => (format!("send {} · reply {}", fmt_ms(t.send_ms), fmt_ms(t.wait_ms)), format!("sent in {}, reply after {}", fmt_ms(t.send_ms), fmt_ms(t.wait_ms))),
        None => (fmt_ms(a.ms), format!("in {}", fmt_ms(a.ms))),
    }
}

fn attempt_step(a: &Attempt, suffix: &str, outcome: &str) -> Step {
    let name = match a.provider {
        Provider::Local => format!("Local {}", a.model.trim_start_matches("ggml-").trim_end_matches(".bin")),
        p => p.info().label.to_string(),
    };
    let (short, long) = timing_text(a);
    match &a.result {
        Ok(_) => Step { label: format!("{name} · {short}{suffix}"), detail: format!("{} ({}) {long}; {outcome}", a.provider.info().label, a.model), state: StepState::Ok },
        Err(e) => Step { label: format!("{name} · failed"), detail: e.clone(), state: StepState::Failed },
    }
}

fn fallback_notifier(shared: &Arc<Shared>) -> transcribe::Notify {
    let shared = shared.clone();
    Arc::new(move |n: Notice| {
        if !shared.settings.read().unwrap().notify_fallback {
            return;
        }
        if matches!(n, Notice::LocalStarted(_)) {
            audio::play(Sound::Warning);
        }
        let (title, body) = n.text();
        notify(title, body);
    })
}

fn pipeline(shared: &Arc<Shared>, s: &Settings, samples: &[f32], force_format: bool, audio: Option<String>) -> (Option<Entry>, Option<std::sync::mpsc::Receiver<Msg>>) {
    let mut late = None;
    let entry = transcribe_and_deliver(shared, s, samples, force_format, audio, &mut late);
    (entry, late)
}

fn transcribe_and_deliver(shared: &Arc<Shared>, s: &Settings, samples: &[f32], force_format: bool, audio: Option<String>, late: &mut Option<std::sync::mpsc::Receiver<Msg>>) -> Option<Entry> {
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
        audio,
    };

    if hooks::has(&s.scripts, Stage::BeforeTranscription) {
        let path = std::env::temp_dir().join(format!("{}-{}.wav", crate::settings::APP_NAME, std::process::id()));
        if std::fs::write(&path, audio::wav_bytes(samples, WHISPER_RATE)).is_ok() {
            let audio_path = path.to_string_lossy().to_string();
            warnings.extend(hooks::notify(&s.scripts, Stage::BeforeTranscription, "", &[("WH_AUDIO", &audio_path)], timeout));
            let _ = std::fs::remove_file(&path);
        }
    }

    let secs = entry.duration_secs as f64;
    let (remote, local) = transcribe::plan(s);
    let primary = remote.first().map(|p| (p.provider.info().label, p.model(p.provider).to_string()));
    let race = transcribe::race(remote, local, secs, transcribe::real_runner(samples.into(), shared.local.clone()), shared.latency.clone(), fallback_notifier(shared));
    let mut started_step = race.local_started.map(|(trigger, at)| {
        let (label, primary) = primary.clone().unwrap_or_default();
        let detail = match (trigger, race.threshold) {
            (Trigger::Slow, Some(t)) => format!("{label} ({primary}) had not replied {} after the upload finished; for {secs:.1} s of audio it usually replies within {} (expected time plus one standard deviation), so local transcription started alongside it.", fmt_ms(t.as_millis() as u64), fmt_ms(t.as_millis() as u64)),
            _ => format!("{label} ({primary}) failed, so local transcription started at once."),
        };
        Step { label: format!("Local started · {}", fmt_ms(at)), detail, state: StepState::Skipped }
    });
    for (i, a) in race.attempts.iter().enumerate() {
        if a.provider == Provider::Local && let Some(st) = started_step.take() {
            entry.steps.push(st);
        }
        let outcome = if Some(i) == race.winner { "this result was used" } else { "not used" };
        entry.steps.push(attempt_step(a, "", outcome));
    }
    entry.steps.extend(started_step);
    for name in &race.pending {
        let detail = format!("{name} was still working when another result won; its result will be discarded.");
        let name = if name == "Local" { name.clone() } else { name.split(" (").next().unwrap_or(name).to_string() };
        step(&mut entry.steps, waiting_label(&name), detail, StepState::Skipped);
    }
    let failures: Vec<String> = race.attempts.iter().filter_map(|a| a.result.as_ref().err().filter(|e| !e.contains("cancelled")).map(|e| format!("{} ({}): {e}", a.provider.info().label, a.model))).collect();
    *late = Some(race.late);
    match race.winner.map(|i| &race.attempts[i]) {
        Some(win) => {
            entry.raw = win.result.clone().unwrap_or_default();
            entry.provider = win.provider;
            entry.model = win.model.clone();
            if !failures.is_empty() {
                warnings.push(format!("Used {} ({}) after: {}", win.provider.info().label, win.model, failures.join("; ")));
            }
        }
        None => {
            let e = if failures.is_empty() { "No transcription attempt could run".to_string() } else { failures.join("\n") };
            shared.set_notice(&e, true);
            entry.error = Some(e);
            return Some(entry);
        }
    }
    if entry.raw.is_empty() {
        shared.set_notice("No speech detected", false);
        entry.error = Some("No speech detected".into());
        return entry.audio.is_some().then_some(entry);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::Timing;

    #[test]
    fn probe_summary_averages_only_good_replies() {
        let run = |ok: bool, wait_ms: u64| (if ok { Ok("hello".to_string()) } else { Err("down".to_string()) }, Some(Timing { send_ms: 100, wait_ms }));
        let probe = Probe { runs: vec![run(true, 500), run(true, 1000), run(false, 9000), run(true, 1500)], ..Probe::default() };
        let s = probe.summary().unwrap();
        assert_eq!((s.count, s.mean, s.min, s.max), (3, 1000.0, 500, 1500));
        assert!((s.sd - 500.0).abs() < 1e-9);
        assert!(Probe { runs: vec![run(false, 1)], ..Probe::default() }.summary().is_none());
    }
}
