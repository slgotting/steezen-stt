use crate::audio::{WHISPER_RATE, wav_bytes};
use crate::settings::{Provider, Settings, data_dir};
use reqwest::blocking::{Client, multipart};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
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

pub fn download_model(name: &str, http: &Client, progress: &Mutex<Option<(String, u64, u64)>>) -> Result<(), String> {
    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{name}");
    let mut resp = http.get(url).send().and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
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

pub fn transcribe(samples: &[f32], settings: &Settings, http: &Client, local: &LocalEngine) -> Result<String, String> {
    match settings.provider {
        Provider::Local => local.transcribe(samples, settings),
        p => transcribe_cloud(p, samples, settings, http),
    }
}

fn transcribe_cloud(provider: Provider, samples: &[f32], settings: &Settings, http: &Client) -> Result<String, String> {
    let info = provider.info();
    let key = settings.api_key(provider);
    if key.is_empty() {
        return Err(format!("No {} API key. Add one in Settings → Transcription or set {}", info.label, info.key_env));
    }
    let file = multipart::Part::bytes(wav_bytes(samples, WHISPER_RATE)).file_name("audio.wav").mime_str("audio/wav").map_err(|e| e.to_string())?;
    let mut form = multipart::Form::new()
        .part("file", file)
        .text("model", settings.model(provider).to_string())
        .text("response_format", "json")
        .text("temperature", settings.temperature.to_string());
    for (field, value) in [("language", &settings.language), ("prompt", &settings.prompt)] {
        if !value.trim().is_empty() {
            form = form.text(field, value.trim().to_string());
        }
    }
    let resp = http
        .post(format!("{}/audio/transcriptions", info.base_url))
        .bearer_auth(key)
        .multipart(form)
        .timeout(Duration::from_secs(120))
        .send()
        .map_err(|e| format!("{} request failed: {e}", info.label))?;
    let status = resp.status();
    let body: Value = resp.json().map_err(|e| format!("{} returned an unreadable response: {e}", info.label))?;
    if !status.is_success() {
        let msg = body["error"]["message"].as_str().unwrap_or("unknown error");
        return Err(format!("{} error ({status}): {msg}", info.label));
    }
    Ok(body["text"].as_str().unwrap_or_default().trim().to_string())
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
    fn transcribe(&self, _: &[f32], _: &Settings) -> Result<String, String> {
        Err("This build has no local transcription. Rebuild with the `local` feature.".into())
    }

    #[cfg(feature = "local")]
    fn transcribe(&self, samples: &[f32], settings: &Settings) -> Result<String, String> {
        use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
        let name = &settings.local_model;
        let path = models_dir().join(name);
        if !path.exists() {
            return Err(format!("Local model {name} is not downloaded. Download it in Settings → Transcription."));
        }
        let mut guard = self.loaded.lock().unwrap();
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
        for flag in [FullParams::set_print_special, FullParams::set_print_progress, FullParams::set_print_realtime, FullParams::set_print_timestamps] {
            flag(&mut params, false);
        }
        let mut padded = samples.to_vec();
        padded.resize(padded.len().max(WHISPER_RATE as usize + WHISPER_RATE as usize / 10), 0.0);
        state.full(params, &padded).map_err(|e| format!("Local transcription failed: {e}"))?;
        let text: String = state.as_iter().map(|s| s.to_string()).collect();
        *self.last_used.lock().unwrap() = Some(Instant::now());
        Ok(text.trim().to_string())
    }
}

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;

    fn read_wav(bytes: &[u8]) -> (Vec<f32>, u32) {
        let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        let channels = u16::from_le_bytes(bytes[22..24].try_into().unwrap()) as usize;
        let data = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
        let samples: Vec<f32> = bytes[data..].as_chunks::<2>().0.iter().map(|&c| i16::from_le_bytes(c) as f32 / i16::MAX as f32).collect();
        (samples.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect(), rate)
    }

    #[test]
    #[ignore = "needs WH_TEST_WAV and a downloaded model"]
    fn local_transcribes_wav() {
        let (samples, rate) = read_wav(&std::fs::read(std::env::var("WH_TEST_WAV").unwrap()).unwrap());
        let samples = crate::audio::resample(&samples, rate, WHISPER_RATE);
        let settings = Settings { provider: Provider::Local, local_model: std::env::var("WH_TEST_MODEL").unwrap(), ..Settings::default() };
        let text = transcribe(&samples, &settings, &Client::new(), &LocalEngine::default()).unwrap();
        println!("TRANSCRIPT: {text}");
        assert!(text.to_lowercase().contains("cloud"));
    }
}
