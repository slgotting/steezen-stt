use crate::audio::{WHISPER_RATE, read_wav, resample};
use crate::settings::{Provider, Settings};
use crate::latency::Latency;
use crate::transcribe::{LocalEngine, Notice, Notify, local_models, plan, race, real_runner};
use std::sync::Arc;

const USAGE: &str = "usage: steezen-stt transcribe <file.wav> [--prompt <words>] [--local]\nTranscribes a 16-bit PCM WAV and prints the text: the service chosen in Settings first, then the other online service you have a key for. A local whisper.cpp model (your fallback list, or the best downloaded model) starts at once if a service fails, or alongside it when the service is unusually slow for the clip length; the first good result wins. The service used is printed to stderr as \"transcribed by: …\".\n--local skips the online services and uses only the downloaded whisper.cpp models.";

pub fn transcribe(args: &[String]) -> i32 {
    let (mut file, mut prompt, mut local) = (None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--prompt" => prompt = it.next().cloned(),
            "--local" => local = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return 0;
            }
            _ if file.is_none() => file = Some(a.clone()),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let Some(file) = file else {
        eprintln!("{USAGE}");
        return 2;
    };
    let mut settings = Settings::load();
    if local {
        settings.provider = Provider::Local;
        if local_models(&settings).is_empty() {
            eprintln!("No local model is downloaded. Download one in Settings → Transcription.");
            return 1;
        }
    }
    if let Some(p) = prompt.filter(|p| !p.trim().is_empty()) {
        settings.prompt = [settings.prompt.trim(), p.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    }
    let result = std::fs::read(&file)
        .map_err(|e| format!("cannot read {file}: {e}"))
        .and_then(|bytes| read_wav(&bytes))
        .and_then(|(samples, rate)| {
            let samples: Arc<[f32]> = resample(&samples, rate, WHISPER_RATE).into();
            let secs = samples.len() as f64 / WHISPER_RATE as f64;
            let (remote, local) = plan(&settings);
            let notify: Notify = Arc::new(|n: Notice| eprintln!("{}", n.text().1));
            let r = race(remote, local, secs, real_runner(samples, Arc::new(LocalEngine::default())), Arc::new(Latency::load()), notify);
            for a in r.attempts.iter().filter(|a| a.result.is_err()) {
                eprintln!("{} ({}) failed: {}", a.provider.info().label, a.model, a.result.as_ref().err().map_or("", |e| e));
            }
            let winner = r.winner.map(|i| &r.attempts[i]).ok_or_else(|| if r.attempts.is_empty() { "nothing to try".to_string() } else { "every transcription attempt failed".to_string() })?;
            let timing = winner.timing.map_or(String::new(), |t| format!(", sent in {} ms, reply after {} ms", t.send_ms, t.wait_ms));
            eprintln!("transcribed by: {} ({}){timing}", winner.provider.info().label, winner.model);
            winner.result.clone()
        });
    match result {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
