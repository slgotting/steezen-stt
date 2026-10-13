use crate::audio::{WHISPER_RATE, read_wav, resample};
use crate::settings::{Provider, Settings};
use crate::transcribe::{LocalEngine, installed_local_models, transcribe_with_fallback};

const USAGE: &str = "usage: steezen-stt transcribe <file.wav> [--prompt <words>] [--local]\nTranscribes a 16-bit PCM WAV with the service chosen in Settings and prints the text.\n--local skips the online service and uses the downloaded whisper.cpp models.";

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
        match local_only(&settings) {
            Ok(s) => settings = s,
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        }
    }
    if let Some(p) = prompt.filter(|p| !p.trim().is_empty()) {
        settings.prompt = [settings.prompt.trim(), p.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    }
    let result = std::fs::read(&file)
        .map_err(|e| format!("cannot read {file}: {e}"))
        .and_then(|bytes| read_wav(&bytes))
        .and_then(|(samples, rate)| {
            let mut attempts = transcribe_with_fallback(&resample(&samples, rate, WHISPER_RATE), &settings, &LocalEngine::default());
            let last = attempts.pop().map(|a| a.result).unwrap_or_else(|| Err("nothing to try".into()));
            for a in &attempts {
                eprintln!("{} ({}) failed: {}", a.provider.info().label, a.model, a.result.as_ref().err().map_or("", |e| e));
            }
            last
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

fn local_only(settings: &Settings) -> Result<Settings, String> {
    let installed = installed_local_models();
    let preferred: Vec<String> = std::iter::once(settings.local_model.clone())
        .chain(settings.local_fallback.iter().cloned())
        .chain(Provider::Local.info().fallback_models.iter().map(|m| m.to_string()))
        .chain(installed.iter().cloned())
        .filter(|m| installed.contains(m))
        .fold(Vec::new(), |mut out, m| {
            if !out.contains(&m) {
                out.push(m);
            }
            out
        });
    let (first, rest) = preferred.split_first().ok_or("No local model is downloaded. Download one in Settings → Transcription.")?;
    Ok(Settings { provider: Provider::Local, local_model: first.clone(), local_fallback: rest.to_vec(), ..settings.clone() })
}
