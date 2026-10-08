use crate::audio::{WHISPER_RATE, read_wav, resample};
use crate::settings::Settings;
use crate::transcribe::{LocalEngine, transcribe_with_fallback};

const USAGE: &str = "usage: steezen-stt transcribe <file.wav> [--prompt <words>]\nTranscribes a 16-bit PCM WAV with the service chosen in Settings and prints the text.";

pub fn transcribe(args: &[String]) -> i32 {
    let (mut file, mut prompt) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--prompt" => prompt = it.next().cloned(),
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
