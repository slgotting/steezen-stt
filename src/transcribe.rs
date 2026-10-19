use crate::audio::{WHISPER_RATE, wav_bytes};
use crate::latency::{self, Latency, Recorded};
use crate::settings::{Provider, Settings, data_dir};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

pub fn list_models(provider: Provider, settings: &Settings, http: &Client) -> Result<Vec<String>, String> {
    let info = provider.info();
    let (url, key) = match provider {
        Provider::Local => (info.base_url.to_string(), String::new()),
        _ => (format!("{}/models", info.base_url), settings.api_key(provider)),
    };
    if provider != Provider::Local && key.is_empty() {
        return Err(format!("add an API key for {} to list models", info.label));
    }
    let mut req = http.get(url).timeout(Duration::from_secs(15));
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let body: Value = req.send().and_then(|r| r.error_for_status()).and_then(|r| r.json()).map_err(|e| e.to_string())?;
    let (items, field) = match provider {
        Provider::Local => (&body["siblings"], "rfilename"),
        _ => (&body["data"], "id"),
    };
    let mut models: Vec<String> = items
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m[field].as_str())
        .filter(|id| info.model_filter.iter().any(|f| id.contains(f)))
        .filter(|id| provider != Provider::Local || (id.ends_with(".bin") && !id.contains('/') && !id.contains("-encoder")))
        .map(String::from)
        .collect();
    models.sort();
    Ok(models)
}

pub fn installed_local_models() -> Vec<String> {
    let mut models: Vec<String> = std::fs::read_dir(models_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".bin"))
        .collect();
    models.sort();
    models
}

const LOCAL_FAMILIES: &[(&str, f32, u8, u8)] = &[
    ("large-v3-turbo", 809.0, 5, 3),
    ("large-v3", 1550.0, 5, 1),
    ("large-v2", 1550.0, 5, 1),
    ("large-v1", 1550.0, 4, 1),
    ("large", 1550.0, 4, 1),
    ("medium", 769.0, 4, 2),
    ("small", 244.0, 3, 4),
    ("base", 74.0, 2, 5),
    ("tiny", 39.0, 1, 5),
];

const LOCAL_QUANTS: &[(&str, f32, &str)] = &[("-q5_0", 0.69, "5-bit"), ("-q5_1", 0.75, "5-bit"), ("-q8_0", 1.07, "8-bit")];

const LOCAL_TAGS: &[(&str, &str)] = &[(".en", "English only"), ("-tdrz", "marks speaker turns")];

pub struct LocalModelInfo {
    pub quality: u8,
    pub speed: u8,
    pub size_mb: u32,
    pub notes: Vec<&'static str>,
}

pub fn local_model_info(name: &str) -> Option<LocalModelInfo> {
    let stem = name.strip_prefix("ggml-")?.strip_suffix(".bin")?;
    let &(_, params, quality, speed) = LOCAL_FAMILIES.iter().find(|(f, ..)| stem.starts_with(f))?;
    let (bytes, precision) = LOCAL_QUANTS.iter().find(|(q, ..)| stem.contains(q)).map_or((2.0, None), |&(_, b, p)| (b, Some(p)));
    let notes = LOCAL_TAGS.iter().filter(|(t, _)| stem.contains(t)).map(|&(_, n)| n).chain(precision).collect();
    Some(LocalModelInfo { quality, speed, size_mb: (params * bytes) as u32, notes })
}

fn untimed_client() -> Result<Client, String> {
    Client::builder().connect_timeout(Duration::from_secs(10)).timeout(None).build().map_err(|e| e.to_string())
}

pub fn download_model(name: &str, progress: &Mutex<Option<(String, u64, u64)>>) -> Result<(), String> {
    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{name}");
    let mut resp = untimed_client()?.get(url).send().and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let total = resp.content_length().unwrap_or(0);
    std::fs::create_dir_all(models_dir()).map_err(|e| e.to_string())?;
    let part = models_dir().join(format!("{name}.part"));
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        *progress.lock().unwrap() = Some((name.to_string(), done, total));
    }
    std::fs::rename(&part, models_dir().join(name)).map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    pub send_ms: u64,
    pub wait_ms: u64,
}

pub struct Attempt {
    pub provider: Provider,
    pub model: String,
    pub ms: u64,
    pub timing: Option<Timing>,
    pub result: Result<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Slow,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    LocalStarted(Trigger),
    Recovered,
}

pub enum Msg {
    Uploaded,
    Done(Attempt),
}

pub struct Signal {
    cancel: Arc<AtomicBool>,
    tx: Sender<Msg>,
}

impl Signal {
    pub fn on_uploaded(&self) -> Box<dyn FnOnce() + Send> {
        let tx = self.tx.clone();
        Box::new(move || drop(tx.send(Msg::Uploaded)))
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

impl Notice {
    pub fn text(self) -> (&'static str, &'static str) {
        match self {
            Notice::LocalStarted(Trigger::Slow) => ("Transcription is slow", "Remote transcription is responding slowly. Starting local transcription."),
            Notice::LocalStarted(Trigger::Failed) => ("Transcription failed", "Remote transcription failed. Starting local transcription."),
            Notice::Recovered => ("Transcription recovered", "Remote transcription performance has recovered."),
        }
    }
}

pub type Runner = Arc<dyn Fn(&Settings, &Signal) -> Attempt + Send + Sync>;
pub type Notify = Arc<dyn Fn(Notice) + Send + Sync>;

pub struct Race {
    pub attempts: Vec<Attempt>,
    pub winner: Option<usize>,
    pub local_started: Option<(Trigger, u64)>,
    pub threshold: Option<Duration>,
    pub pending: Vec<String>,
    pub late: Receiver<Msg>,
}

pub fn plan(settings: &Settings) -> (Vec<Settings>, Vec<Settings>) {
    let with = |provider: Provider, local_model: &str| Settings { provider, local_model: local_model.to_string(), ..settings.clone() };
    let mut local: Vec<Settings> = local_models(settings).iter().map(|m| with(Provider::Local, m)).collect();
    if settings.provider == Provider::Local {
        if local.is_empty() {
            local.push(settings.clone());
        }
        return (Vec::new(), local);
    }
    let remote = std::iter::once(settings.provider)
        .chain(Provider::ALL.into_iter().filter(|p| *p != Provider::Local && *p != settings.provider && !settings.api_key(*p).is_empty()))
        .map(|p| with(p, &settings.local_model))
        .collect();
    (remote, local)
}

pub fn local_models(settings: &Settings) -> Vec<String> {
    let installed = installed_local_models();
    let configured: Vec<String> = match settings.provider {
        Provider::Local => std::iter::once(settings.local_model.clone()).chain(settings.local_fallback.iter().cloned()).collect(),
        _ => settings.local_fallback.clone(),
    };
    let defaults: Vec<String> = match configured.iter().any(|m| installed.contains(m)) {
        true => Vec::new(),
        false => std::iter::once(settings.local_model.clone()).chain(Provider::Local.info().fallback_models.iter().map(|m| m.to_string())).chain(installed.iter().cloned()).collect(),
    };
    configured.into_iter().chain(defaults).filter(|m| installed.contains(m)).fold(Vec::new(), |mut out, m| {
        if !out.contains(&m) {
            out.push(m);
        }
        out
    })
}

pub fn real_runner(samples: Arc<[f32]>, local: Arc<LocalEngine>) -> Runner {
    Arc::new(move |s: &Settings, signal: &Signal| {
        let started = Instant::now();
        let (result, timing) = match s.provider {
            Provider::Local => (local.transcribe(&samples, s, signal.cancel.clone()), None),
            p => transcribe_cloud(p, &samples, s, signal.on_uploaded()),
        };
        Attempt { provider: s.provider, model: s.model(s.provider).to_string(), ms: started.elapsed().as_millis() as u64, timing, result }
    })
}

fn spawn_lane(plan: Vec<Settings>, runner: Runner, cancel: Arc<AtomicBool>, tx: Sender<Msg>, record: Option<(Arc<Latency>, f64, Notify)>) {
    std::thread::spawn(move || {
        let signal = Signal { cancel, tx };
        for s in plan {
            if signal.cancelled() {
                break;
            }
            let a = runner(&s, &signal);
            if let (Some((latency, secs, notify)), Ok(_), Some(t)) = (&record, &a.result, a.timing)
                && let Recorded::Recovered = latency.record(&latency::key(a.provider, &a.model), *secs, t.send_ms, t.wait_ms) {
                    notify(Notice::Recovered);
                }
            let ok = a.result.is_ok();
            if signal.tx.send(Msg::Done(a)).is_err() || ok {
                break;
            }
        }
    });
}

pub fn race(remote: Vec<Settings>, local_plan: Vec<Settings>, secs: f64, runner: Runner, latency: Arc<Latency>, notify: Notify) -> Race {
    let started = Instant::now();
    let cancel = Arc::new(AtomicBool::new(false));
    let (sender, rx) = std::sync::mpsc::channel();
    let mut tx = (!local_plan.is_empty()).then(|| (sender.clone(), local_plan));
    let mut local = None;
    let threshold = remote.first().and_then(|s| latency.threshold(&latency::key(s.provider, s.model(s.provider)), secs)).filter(|_| tx.is_some());
    let remote_labels: Vec<String> = remote.iter().map(|s| format!("{} ({})", s.provider.info().label, s.model(s.provider))).collect();
    let start_local = |trigger: Trigger, tx: &mut Option<(Sender<Msg>, Vec<Settings>)>, local: &mut Option<(Trigger, u64)>| {
        if let Some((tx, plan)) = tx.take() {
            *local = Some((trigger, started.elapsed().as_millis() as u64));
            if latency.mark_degraded() {
                notify(Notice::LocalStarted(trigger));
            }
            spawn_lane(plan, runner.clone(), cancel.clone(), tx, None);
        }
    };
    match remote.is_empty() {
        true => {
            if let Some((tx, plan)) = tx.take() {
                spawn_lane(plan, runner.clone(), cancel.clone(), tx, None);
            }
        }
        false => spawn_lane(remote, runner.clone(), cancel.clone(), sender, Some((latency.clone(), secs, notify.clone()))),
    }
    let mut deadline = None;
    let mut attempts: Vec<Attempt> = Vec::new();
    let winner = loop {
        let msg = match deadline.filter(|_| tx.is_some()) {
            Some(d) => match rx.recv_timeout(d - Instant::now().min(d)) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => {
                    start_local(Trigger::Slow, &mut tx, &mut local);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => None,
            },
            None => rx.recv().ok(),
        };
        match msg {
            None => break None,
            Some(Msg::Uploaded) => deadline = deadline.or(threshold.map(|t| Instant::now() + t)),
            Some(Msg::Done(a)) => {
                let (ok, remote) = (a.result.is_ok(), a.provider != Provider::Local);
                attempts.push(a);
                if ok {
                    break Some(attempts.len() - 1);
                }
                if remote {
                    start_local(Trigger::Failed, &mut tx, &mut local);
                }
            }
        }
    };
    cancel.store(true, Ordering::SeqCst);
    let remote_done = attempts.iter().filter(|a| a.provider != Provider::Local).count();
    let mut pending: Vec<String> = remote_labels.get(remote_done).cloned().into_iter().collect();
    if local.is_some() && !attempts.iter().any(|a| a.provider == Provider::Local) {
        pending.push("Local".into());
    }
    if winner.is_none() {
        pending.clear();
    }
    Race { attempts, winner, local_started: local, threshold, pending, late: rx }
}

#[cfg(test)]
pub fn transcribe(samples: &[f32], settings: &Settings, local: &LocalEngine) -> Result<String, String> {
    match settings.provider {
        Provider::Local => local.transcribe(samples, settings, Arc::new(AtomicBool::new(false))),
        p => transcribe_cloud(p, samples, settings, Box::new(|| {})).0,
    }
}

struct Timed<R> {
    inner: R,
    left: u64,
    on_done: Option<Box<dyn FnOnce() + Send>>,
}

impl<R: Read> Read for Timed<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.left = self.left.saturating_sub(n as u64);
        if (self.left == 0 || n == 0)
            && let Some(f) = self.on_done.take() {
                f();
            }
        Ok(n)
    }
}

fn multipart_body(fields: &[(&str, String)], wav: &[u8]) -> (String, Vec<u8>) {
    let boundary = format!("steezen-{}-{}", std::process::id(), chrono::Local::now().timestamp_nanos_opt().unwrap_or_default());
    let mut body = Vec::with_capacity(wav.len() + 1024);
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes());
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

fn transcribe_cloud(provider: Provider, samples: &[f32], settings: &Settings, uploaded: Box<dyn FnOnce() + Send>) -> (Result<String, String>, Option<Timing>) {
    let info = provider.info();
    let key = settings.api_key(provider);
    if key.is_empty() {
        return (Err(format!("No {} API key. Add one in Settings → Transcription or set {}", info.label, info.key_env)), None);
    }
    let mut fields = vec![("model", settings.model(provider).to_string()), ("response_format", "json".into()), ("temperature", settings.temperature.to_string())];
    for (field, value) in [("language", &settings.language), ("prompt", &settings.prompt)] {
        if !value.trim().is_empty() {
            fields.push((field, value.trim().to_string()));
        }
    }
    let (boundary, body) = multipart_body(&fields, &wav_bytes(samples, WHISPER_RATE));
    let client = match untimed_client() {
        Ok(c) => c,
        Err(e) => return (Err(e), None),
    };
    let started = Instant::now();
    let sent = Arc::new(Mutex::new(None));
    let len = body.len() as u64;
    let mark = sent.clone();
    let on_done = Box::new(move || {
        *mark.lock().unwrap() = Some(Instant::now());
        uploaded();
    });
    let reader = Timed { inner: std::io::Cursor::new(body), left: len, on_done: Some(on_done) };
    let result = client
        .post(format!("{}/audio/transcriptions", info.base_url))
        .bearer_auth(key)
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .body(reqwest::blocking::Body::sized(reader, len))
        .send()
        .map_err(|e| format!("{} request failed: {e}", info.label))
        .and_then(|resp| {
            let status = resp.status();
            let body: Value = resp.json().map_err(|e| format!("{} returned an unreadable response: {e}", info.label))?;
            if !status.is_success() {
                let msg = body["error"]["message"].as_str().unwrap_or("unknown error");
                return Err(format!("{} error ({status}): {msg}", info.label));
            }
            Ok(body["text"].as_str().unwrap_or_default().trim().to_string())
        });
    let done = Instant::now();
    let sent_at = *sent.lock().unwrap().get_or_insert(done);
    (result, Some(Timing { send_ms: (sent_at - started).as_millis() as u64, wait_ms: (done - sent_at).as_millis() as u64 }))
}

#[cfg(feature = "local")]
struct Loaded {
    name: String,
    gpu: bool,
    ctx: whisper_rs::WhisperContext,
}

#[derive(Default)]
pub struct LocalEngine {
    #[cfg(feature = "local")]
    loaded: Mutex<Option<Loaded>>,
    last_used: Mutex<Option<Instant>>,
}

impl LocalEngine {
    pub fn is_loaded(&self) -> bool {
        #[cfg(feature = "local")]
        return self.loaded.lock().unwrap().is_some();
        #[cfg(not(feature = "local"))]
        false
    }

    pub fn unload_if_idle(&self, idle: Duration) {
        let mut last = self.last_used.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() >= idle) {
            #[cfg(feature = "local")]
            self.loaded.lock().unwrap().take();
            *last = None;
        }
    }

    #[cfg(not(feature = "local"))]
    fn transcribe(&self, _: &[f32], _: &Settings, _: Arc<AtomicBool>) -> Result<String, String> {
        Err("This build has no local transcription. Rebuild with the `local` feature.".into())
    }

    #[cfg(feature = "local")]
    fn transcribe(&self, samples: &[f32], settings: &Settings, cancel: Arc<AtomicBool>) -> Result<String, String> {
        use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
        let name = &settings.local_model;
        let path = models_dir().join(name);
        if !path.exists() {
            return Err(format!("Local model {name} is not downloaded. Download it in Settings → Transcription."));
        }
        let mut guard = self.loaded.lock().unwrap();
        if cancel.load(Ordering::SeqCst) {
            return Err("Local transcription cancelled; another result already won".into());
        }
        if guard.as_ref().is_none_or(|l| l.name != *name || l.gpu != settings.local_use_gpu) {
            guard.take();
            let mut params = WhisperContextParameters::default();
            params.use_gpu(settings.local_use_gpu);
            let ctx = WhisperContext::new_with_params(&path, params).map_err(|e| format!("Failed to load {name}: {e}"))?;
            *guard = Some(Loaded { name: name.clone(), gpu: settings.local_use_gpu, ctx });
        }
        let loaded = guard.as_ref().unwrap();
        let mut state = loaded.ctx.create_state().map_err(|e| e.to_string())?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let threads = match settings.local_threads {
            0 => std::thread::available_parallelism().map_or(4, |n| n.get().min(8)),
            n => n as usize,
        };
        params.set_n_threads(threads as i32);
        let language = settings.language.trim();
        params.set_language(Some(if language.is_empty() { "auto" } else { language }));
        params.set_temperature(settings.temperature);
        if !settings.prompt.trim().is_empty() {
            params.set_initial_prompt(settings.prompt.trim());
        }
        params.set_no_context(true);
        let abort = cancel.clone();
        params.set_abort_callback_safe(move || abort.load(Ordering::SeqCst));
        for flag in [FullParams::set_print_special, FullParams::set_print_progress, FullParams::set_print_realtime, FullParams::set_print_timestamps] {
            flag(&mut params, false);
        }
        let mut padded = samples.to_vec();
        padded.resize(padded.len().max(WHISPER_RATE as usize + WHISPER_RATE as usize / 10), 0.0);
        let ran = state.full(params, &padded);
        if cancel.load(Ordering::SeqCst) {
            return Err("Local transcription cancelled; another result already won".into());
        }
        ran.map_err(|e| format!("Local transcription failed: {e}"))?;
        let text: String = state.as_iter().map(|s| s.to_string()).collect();
        *self.last_used.lock().unwrap() = Some(Instant::now());
        Ok(text.trim().to_string())
    }
}

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs WH_TEST_WAV and a downloaded model"]
    fn local_transcribes_wav() {
        let (samples, rate) = crate::audio::read_wav(&std::fs::read(std::env::var("WH_TEST_WAV").unwrap()).unwrap()).unwrap();
        let samples = crate::audio::resample(&samples, rate, WHISPER_RATE);
        let settings = Settings { provider: Provider::Local, local_model: std::env::var("WH_TEST_MODEL").unwrap(), ..Settings::default() };
        let text = transcribe(&samples, &settings, &LocalEngine::default()).unwrap();
        println!("TRANSCRIPT: {text}");
        assert!(text.to_lowercase().contains("cloud"));
    }

    #[test]
    #[ignore = "needs WH_TEST_WAV and a downloaded model"]
    fn falls_back_to_local_when_cloud_fails() {
        let (samples, rate) = crate::audio::read_wav(&std::fs::read(std::env::var("WH_TEST_WAV").unwrap()).unwrap()).unwrap();
        let samples: Arc<[f32]> = crate::audio::resample(&samples, rate, WHISPER_RATE).into();
        let model = std::env::var("WH_TEST_MODEL").unwrap();
        let settings = Settings { provider: Provider::OpenAI, openai_api_key: String::new(), local_fallback: vec!["ggml-missing.bin".into(), model.clone()], ..Settings::default() };
        let remote = vec![settings.clone()];
        let local = vec![Settings { provider: Provider::Local, local_model: "ggml-missing.bin".into(), ..settings.clone() }, Settings { provider: Provider::Local, local_model: model.clone(), ..settings.clone() }];
        let r = race(remote, local, 3.0, real_runner(samples, Arc::new(LocalEngine::default())), Arc::new(Latency::in_memory()), Arc::new(|_| {}));
        let summary: Vec<(Provider, &str, bool)> = r.attempts.iter().map(|a| (a.provider, a.model.as_str(), a.result.is_ok())).collect();
        assert_eq!(summary, vec![(Provider::OpenAI, "gpt-4o-transcribe", false), (Provider::Local, "ggml-missing.bin", false), (Provider::Local, model.as_str(), true)]);
        println!("TRANSCRIPT: {}", r.attempts[r.winner.unwrap()].result.as_ref().unwrap());
    }
}

#[cfg(test)]
mod info_tests {
    use super::local_model_info;

    #[test]
    fn parses_local_model_names() {
        let turbo = local_model_info("ggml-large-v3-turbo-q5_0.bin").unwrap();
        assert_eq!((turbo.quality, turbo.speed, turbo.notes), (5, 3, vec!["5-bit"]));
        let small = local_model_info("ggml-small.en-tdrz.bin").unwrap();
        assert_eq!((small.quality, small.notes), (3, vec!["English only", "marks speaker turns"]));
        assert_eq!(local_model_info("ggml-base.en.bin").unwrap().size_mb, 148);
        assert!(local_model_info("whisper-1").is_none());
    }
}


#[cfg(test)]
mod order_tests {
    use super::*;

    fn order(s: &Settings) -> Vec<String> {
        plan(s).0.iter().map(|a| format!("{:?}", a.provider)).collect()
    }

    #[test]
    fn cloud_providers_with_keys_come_first() {
        let s = Settings { provider: Provider::OpenAI, openai_api_key: "sk-x".into(), groq_api_key: "gsk-x".into(), ..Settings::default() };
        assert_eq!(order(&s), ["OpenAI", "Groq"]);
        let g = Settings { provider: Provider::Groq, ..s.clone() };
        assert_eq!(order(&g), ["Groq", "OpenAI"]);
    }

    #[test]
    fn a_cloud_provider_without_a_key_is_skipped() {
        let s = Settings { provider: Provider::OpenAI, openai_api_key: "sk-x".into(), groq_api_key: String::new(), ..Settings::default() };
        if std::env::var("GROQ_API_KEY").is_err() {
            assert_eq!(order(&s), ["OpenAI"]);
        }
    }

    #[test]
    fn local_only_plans_have_no_remote_lane() {
        let s = Settings { provider: Provider::Local, local_model: "ggml-nope.bin".into(), ..Settings::default() };
        let (remote, local) = plan(&s);
        assert!(remote.is_empty());
        assert!(!local.is_empty());
    }

    #[test]
    fn multipart_body_carries_fields_and_audio() {
        let (boundary, body) = multipart_body(&[("model", "m1".into())], b"RIFFDATA");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with(&format!("--{boundary}\r\n")) && text.ends_with(&format!("--{boundary}--\r\n")));
        assert!(text.contains("name=\"model\"\r\n\r\nm1\r\n") && text.contains("filename=\"audio.wav\"") && text.contains("RIFFDATA"));
    }
}

#[cfg(test)]
mod race_tests {
    use super::*;
    use std::collections::HashMap;

    type Spec = HashMap<Provider, (u64, Result<&'static str, &'static str>)>;

    struct Fake {
        runner: Runner,
        calls: Arc<Mutex<Vec<Provider>>>,
        notices: Arc<Mutex<Vec<Notice>>>,
        latency: Arc<Latency>,
    }

    fn fake(spec: &[(Provider, u64, Result<&'static str, &'static str>)]) -> Fake {
        let spec: Spec = spec.iter().map(|&(p, ms, r)| (p, (ms, r))).collect();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let log = calls.clone();
        let runner: Runner = Arc::new(move |s: &Settings, signal: &Signal| {
            log.lock().unwrap().push(s.provider);
            let (ms, result) = spec[&s.provider];
            let started = Instant::now();
            let remote = s.provider != Provider::Local;
            if remote {
                std::thread::sleep(Duration::from_millis(20));
                signal.on_uploaded()();
            }
            while started.elapsed() < Duration::from_millis(ms) {
                if !remote && signal.cancelled() {
                    return Attempt { provider: s.provider, model: s.model(s.provider).into(), ms: 0, timing: None, result: Err("cancelled".into()) };
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let total = started.elapsed().as_millis() as u64;
            let timing = remote.then_some(Timing { send_ms: 20, wait_ms: total - 20 });
            Attempt { provider: s.provider, model: s.model(s.provider).into(), ms: total, timing, result: result.map(String::from).map_err(String::from) }
        });
        Fake { runner, calls, notices: Arc::new(Mutex::new(Vec::new())), latency: Arc::new(Latency::in_memory()) }
    }

    fn trained(f: &Fake, samples: u32) {
        for i in 0..samples {
            f.latency.record(&latency::key(Provider::OpenAI, "gpt-4o-transcribe"), 5.0, 20, 300 + (i as u64 % 3) * 10);
        }
    }

    fn lanes(local: bool) -> (Vec<Settings>, Vec<Settings>) {
        let s = Settings::default();
        let local = if local { vec![Settings { provider: Provider::Local, local_model: "ggml-base.en.bin".into(), ..s.clone() }] } else { vec![] };
        (vec![s], local)
    }

    fn run(f: &Fake, local: bool, secs: f64) -> Race {
        let (remote, local) = lanes(local);
        let notices = f.notices.clone();
        race(remote, local, secs, f.runner.clone(), f.latency.clone(), Arc::new(move |n| notices.lock().unwrap().push(n)))
    }

    fn won(r: &Race) -> Option<(Provider, &str)> {
        r.winner.map(|i| (r.attempts[i].provider, r.attempts[i].result.as_deref().unwrap()))
    }

    const SLOW: u64 = 2600;

    #[test]
    fn fast_remote_never_starts_local() {
        let f = fake(&[(Provider::OpenAI, 100, Ok("remote")), (Provider::Local, 50, Ok("local"))]);
        trained(&f, 10);
        let r = run(&f, true, 5.0);
        assert_eq!(won(&r), Some((Provider::OpenAI, "remote")));
        assert!(r.local_started.is_none());
        assert_eq!(*f.calls.lock().unwrap(), [Provider::OpenAI]);
        assert!(f.notices.lock().unwrap().is_empty());
    }

    #[test]
    fn slow_remote_starts_local_which_wins() {
        let f = fake(&[(Provider::OpenAI, SLOW, Ok("remote")), (Provider::Local, 100, Ok("local"))]);
        trained(&f, 10);
        let r = run(&f, true, 5.0);
        assert_eq!(won(&r), Some((Provider::Local, "local")));
        let (trigger, at) = r.local_started.unwrap();
        assert_eq!(trigger, Trigger::Slow);
        assert!((1500..2400).contains(&at), "{at}");
        assert_eq!(*f.notices.lock().unwrap(), [Notice::LocalStarted(Trigger::Slow)]);
        assert_eq!(r.pending, ["OpenAI (gpt-4o-transcribe)"]);
        match r.late.recv_timeout(Duration::from_secs(3)) {
            Ok(Msg::Done(a)) => assert_eq!((a.provider, a.result.unwrap().as_str()), (Provider::OpenAI, "remote")),
            _ => panic!("late remote result should still arrive for diagnostics"),
        }
        assert_eq!(f.latency.baseline(&latency::key(Provider::OpenAI, "gpt-4o-transcribe")).samples, 11);
    }

    #[test]
    fn remote_wins_after_local_starts() {
        let f = fake(&[(Provider::OpenAI, 1900, Ok("remote")), (Provider::Local, 3000, Ok("local"))]);
        trained(&f, 10);
        let r = run(&f, true, 5.0);
        assert_eq!(won(&r), Some((Provider::OpenAI, "remote")));
        assert_eq!(r.local_started.map(|l| l.0), Some(Trigger::Slow));
        assert_eq!(r.pending, ["Local"]);
        let late: Vec<_> = r.late.iter().filter_map(|m| match m { Msg::Done(a) => Some(a), _ => None }).collect();
        assert!(late.iter().all(|a| a.result.is_err()), "a losing local run is cancelled, never a second result");
    }

    #[test]
    fn simultaneous_finish_yields_exactly_one_winner() {
        for _ in 0..5 {
            let f = fake(&[(Provider::OpenAI, 1700, Ok("remote")), (Provider::Local, 200, Ok("local"))]);
            trained(&f, 10);
            let r = run(&f, true, 5.0);
            let successes = r.attempts.iter().filter(|a| a.result.is_ok()).count();
            assert_eq!(successes, 1, "only the winner is returned for delivery");
            assert!(r.winner.is_some());
            let late_ok = r.late.iter().filter(|m| matches!(m, Msg::Done(a) if a.result.is_ok())).count();
            assert!(late_ok <= 1);
        }
    }

    #[test]
    fn remote_failure_starts_local_immediately() {
        let f = fake(&[(Provider::OpenAI, 50, Err("boom")), (Provider::Local, 100, Ok("local"))]);
        let r = run(&f, true, 5.0);
        assert_eq!(won(&r), Some((Provider::Local, "local")));
        assert_eq!(r.local_started.map(|l| l.0), Some(Trigger::Failed));
        assert_eq!(*f.notices.lock().unwrap(), [Notice::LocalStarted(Trigger::Failed)]);
    }

    #[test]
    fn local_failure_keeps_waiting_for_remote() {
        let f = fake(&[(Provider::OpenAI, SLOW, Ok("remote")), (Provider::Local, 50, Err("no gpu"))]);
        trained(&f, 10);
        let r = run(&f, true, 5.0);
        assert_eq!(won(&r), Some((Provider::OpenAI, "remote")));
        assert_eq!(r.attempts.len(), 2);
    }

    #[test]
    fn both_failing_reports_both_errors() {
        let f = fake(&[(Provider::OpenAI, 50, Err("remote down")), (Provider::Local, 50, Err("model broken"))]);
        let r = run(&f, true, 5.0);
        assert!(r.winner.is_none());
        let errors: Vec<_> = r.attempts.iter().map(|a| a.result.clone().unwrap_err()).collect();
        assert_eq!(errors, ["remote down", "model broken"]);
        assert!(r.pending.is_empty());
    }

    #[test]
    fn without_a_local_model_the_remote_is_awaited() {
        let f = fake(&[(Provider::OpenAI, SLOW, Ok("remote"))]);
        trained(&f, 10);
        let r = run(&f, false, 5.0);
        assert_eq!(won(&r), Some((Provider::OpenAI, "remote")));
        assert!(r.threshold.is_none() && r.local_started.is_none());
        assert!(f.notices.lock().unwrap().is_empty());
    }

    #[test]
    fn longer_audio_moves_the_threshold_out() {
        let f = fake(&[(Provider::OpenAI, SLOW, Ok("remote")), (Provider::Local, 100, Ok("local"))]);
        let key = latency::key(Provider::OpenAI, "gpt-4o-transcribe");
        for i in 0..20u64 {
            let secs = [5.0, 20.0, 60.0][i as usize % 3];
            f.latency.record(&key, secs, 20, 300 + secs as u64 * 40 + (i % 2) * 20);
        }
        let short = f.latency.threshold(&key, 5.0).unwrap();
        let long = f.latency.threshold(&key, 60.0).unwrap();
        assert!(long > short + Duration::from_millis(1500), "{short:?} {long:?}");
        let r = run(&f, true, 60.0);
        assert_eq!(won(&r), Some((Provider::OpenAI, "remote")), "a 60 s clip gets a longer allowance, so a 2.6 s reply is normal");
        assert!(r.local_started.is_none());
    }

    #[test]
    fn insufficient_history_never_races() {
        let f = fake(&[(Provider::OpenAI, SLOW, Ok("remote")), (Provider::Local, 100, Ok("local"))]);
        trained(&f, latency::MIN_SAMPLES - 1);
        let r = run(&f, true, 5.0);
        assert_eq!(won(&r), Some((Provider::OpenAI, "remote")));
        assert!(r.threshold.is_none() && r.local_started.is_none());
    }

    #[test]
    fn slowness_is_announced_once_and_recovery_once() {
        let f = fake(&[(Provider::OpenAI, SLOW, Ok("remote")), (Provider::Local, 100, Ok("local"))]);
        trained(&f, 10);
        for _ in 0..2 {
            let r = run(&f, true, 5.0);
            let _ = r.late.recv_timeout(Duration::from_secs(3));
        }
        let fast = fake(&[(Provider::OpenAI, 100, Ok("remote"))]);
        let r = race(lanes(false).0, vec![], 5.0, fast.runner.clone(), f.latency.clone(), { let n = f.notices.clone(); Arc::new(move |x| n.lock().unwrap().push(x)) });
        assert_eq!(won(&r), Some((Provider::OpenAI, "remote")));
        assert_eq!(*f.notices.lock().unwrap(), [Notice::LocalStarted(Trigger::Slow), Notice::Recovered]);
    }
}
