use serde_json::{Value, json};
use std::sync::LazyLock;
use steezen_core::settings::{Provider, Settings, cache_dir, set_root};
use steezen_core::{bundle, corrector, formatter, http, transcribe, wav};

type Handler = fn(&Value, &[u8]) -> Result<Value, String>;

static HTTP: LazyLock<reqwest::blocking::Client> = LazyLock::new(http::client);
static CACHE: LazyLock<corrector::Cache> = LazyLock::new(|| corrector::Cache::load(cache_dir().join("corrector-cache.json")));

const HANDLERS: &[(&str, Handler)] = &[
    ("init", init),
    ("load", |_, _| settings_value(&Settings::load())),
    ("save", save),
    ("defaults", |_, _| settings_value(&Settings::default())),
    ("groups", |_, _| Ok(Value::Array(bundle::GROUPS.iter().map(|g| json!({"id": g.id, "label": g.label, "secret": g.secret, "desktop_only": g.desktop_only})).collect()))),
    ("providers", |_, _| Ok(Value::Array(Provider::ALL.iter().filter(|p| **p != Provider::Local).map(|p| json!({"id": p, "label": p.info().label, "models": p.info().fallback_models})).collect()))),
    ("dictate", dictate),
    ("zen", zen),
    ("export", export),
    ("open", open),
    ("apply", apply),
];

fn settings_value(s: &Settings) -> Result<Value, String> {
    serde_json::to_value(s).map_err(|e| e.to_string())
}

fn str_arg<'a>(args: &'a Value, name: &str) -> &'a str {
    args[name].as_str().unwrap_or_default()
}

fn init(args: &Value, _: &[u8]) -> Result<Value, String> {
    set_root(str_arg(args, "dir").into());
    settings_value(&Settings::load())
}

fn save(args: &Value, _: &[u8]) -> Result<Value, String> {
    let s: Settings = serde_json::from_value(args["settings"].clone()).map_err(|e| e.to_string())?;
    s.save().map_err(|e| e.to_string())?;
    settings_value(&s)
}

fn samples(pcm: &[u8]) -> Vec<f32> {
    pcm.as_chunks::<2>().0.iter().map(|&c| i16::from_le_bytes(c) as f32 / i16::MAX as f32).collect()
}

fn transcribe_with(s: &Settings, audio: &[f32]) -> (Result<String, String>, Provider) {
    let lanes = std::iter::once(s.provider).chain(s.fallback).filter(|p| *p != Provider::Local);
    let mut last = (Err("No online service is set. Pick OpenAI or Groq and add its key.".to_string()), s.provider);
    for p in lanes {
        let lane = Settings { provider: p, ..s.clone() };
        match transcribe::timed_cloud(audio, &lane).0 {
            Ok(text) => return (Ok(text), p),
            Err(e) => last = (Err(e), p),
        }
    }
    last
}

fn dictate(_: &Value, pcm: &[u8]) -> Result<Value, String> {
    let s = Settings::load();
    let audio = samples(pcm);
    if audio.len() < (wav::WHISPER_RATE as usize * s.min_recording_ms as usize / 1000) {
        return Err("Recording too short".into());
    }
    let (heard, provider) = transcribe_with(&s, &audio);
    let said = heard?;
    let mut text = said.clone();
    let mut notes = Vec::new();
    let mut fixed = Vec::new();
    let key = s.typesafe_key();
    if s.corrector_enabled && !key.is_empty() && !s.dictionary.is_empty() {
        match corrector::correct(&text, &s.dictionary, &key, &s.corrector_model, &CACHE, &HTTP) {
            Ok((corrected, corrections)) => {
                fixed = corrections.iter().filter_map(|c| c.to.as_ref().map(|to| format!("{} → {to}", c.from))).collect();
                text = corrected;
            }
            Err(e) => notes.push(format!("Dictionary skipped: {e}")),
        }
    }
    match formatter::wanted(&text, false, &s, &HTTP) {
        Ok((true, _)) => match formatter::format(&text, &s, &HTTP) {
            Ok(formatted) => text = formatted,
            Err(e) => notes.push(format!("Formatting skipped: {e}")),
        },
        Ok(_) => {}
        Err(e) => notes.push(format!("Format check skipped: {e}")),
    }
    Ok(json!({"text": text, "said": said, "provider": provider.info().label, "corrections": fixed, "notes": notes}))
}

fn zen(_: &Value, pcm: &[u8]) -> Result<Value, String> {
    let s = Settings::load();
    let url = s.zen_url.trim().trim_end_matches('/');
    if url.is_empty() {
        return Err("Set the Zen address first".into());
    }
    let wav = wav::wav_bytes(&samples(pcm), wav::WHISPER_RATE);
    let part = reqwest::blocking::multipart::Part::bytes(wav).file_name("thought.wav").mime_str("audio/wav").map_err(|e| e.to_string())?;
    let mut req = HTTP.post(format!("{url}/api/capture")).multipart(reqwest::blocking::multipart::Form::new().part("audio", part)).timeout(std::time::Duration::from_secs(60));
    if !s.zen_token.trim().is_empty() {
        req = req.header("x-zen-token", s.zen_token.trim());
    }
    let resp = req.send().map_err(|e| format!("Couldn't reach Zen: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().unwrap_or_default();
    match status.as_u16() {
        200..=299 => Ok(json!({"text": "Sent to Zen"})),
        401 => Err("Zen rejected the token. Copy it from Zen → Tools → Inbox token.".into()),
        code => Err(format!("Zen error {code}: {}", body["error"].as_str().unwrap_or_default())),
    }
}

fn export(args: &Value, _: &[u8]) -> Result<Value, String> {
    let groups: Vec<&str> = args["groups"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let pass = Some(str_arg(args, "pass")).filter(|p| !p.trim().is_empty());
    Ok(json!({"text": bundle::export(&Settings::load(), &groups, pass)?}))
}

fn open(args: &Value, _: &[u8]) -> Result<Value, String> {
    let text = str_arg(args, "text");
    if bundle::is_locked(text) && str_arg(args, "pass").trim().is_empty() {
        return Ok(json!({"locked": true}));
    }
    let opened = bundle::open(text, Some(str_arg(args, "pass")))?;
    Ok(json!({"locked": false, "groups": bundle::GROUPS.iter().filter(|g| opened.contains_key(g.id)).map(|g| json!({"id": g.id, "label": g.label})).collect::<Vec<_>>()}))
}

fn apply(args: &Value, _: &[u8]) -> Result<Value, String> {
    let opened = bundle::open(str_arg(args, "text"), Some(str_arg(args, "pass")))?;
    let choices: Vec<(&str, bool)> = args["choices"].as_array().map(|a| a.iter().filter_map(|c| Some((c[0].as_str()?, c[1].as_bool().unwrap_or(false)))).collect()).unwrap_or_default();
    let next = bundle::apply(&Settings::load(), &opened, &choices)?;
    next.save().map_err(|e| e.to_string())?;
    settings_value(&next)
}

pub fn call(method: &str, args: &str, audio: &[u8]) -> String {
    let args: Value = serde_json::from_str(args).unwrap_or(Value::Null);
    let result = HANDLERS.iter().find(|(name, _)| *name == method).ok_or(format!("unknown call {method}")).and_then(|(_, f)| f(&args, audio));
    match result {
        Ok(v) => json!({"ok": true, "value": v}),
        Err(e) => json!({"ok": false, "error": e}),
    }
    .to_string()
}

#[cfg(target_os = "android")]
mod jni_bridge {
    use jni::EnvUnowned;
    use jni::objects::{JByteArray, JClass, JString};

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_slgotting_steezenstt_Core_call<'caller>(mut unowned: EnvUnowned<'caller>, _class: JClass<'caller>, method: JString<'caller>, args: JString<'caller>, audio: JByteArray<'caller>) -> JString<'caller> {
        unowned
            .with_env(|env| -> Result<_, jni::errors::Error> {
                let method = method.try_to_string(env)?;
                let args = args.try_to_string(env)?;
                let audio = if audio.is_null() { Vec::new() } else { env.convert_byte_array(&audio)? };
                JString::from_str(env, super::call(&method, &args, &audio))
            })
            .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_reports_unknown_calls_and_lists_groups() {
        assert!(call("nope", "{}", &[]).contains("unknown call"));
        let v: Value = serde_json::from_str(&call("groups", "{}", &[])).unwrap();
        assert!(v["value"].as_array().unwrap().iter().any(|g| g["id"] == "keys" && g["secret"] == true));
    }

    #[test]
    fn export_open_apply_round_trip_through_calls() {
        let dir = std::env::temp_dir().join(format!("steezen-mobile-test-{}", std::process::id()));
        call("init", &json!({"dir": dir}).to_string(), &[]);
        let mut s = Settings::default();
        s.groq_api_key = "gsk".into();
        s.save().unwrap();
        let out: Value = serde_json::from_str(&call("export", &json!({"groups": ["keys", "output"], "pass": "pw"}).to_string(), &[])).unwrap();
        let text = out["value"]["text"].as_str().unwrap();
        let locked: Value = serde_json::from_str(&call("open", &json!({"text": text}).to_string(), &[])).unwrap();
        assert_eq!(locked["value"]["locked"], true);
        Settings::default().save().unwrap();
        let applied: Value = serde_json::from_str(&call("apply", &json!({"text": text, "pass": "pw", "choices": [["keys", false]]}).to_string(), &[])).unwrap();
        assert_eq!(applied["value"]["groq_api_key"], "gsk");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore]
    fn dictate_with_real_settings() {
        let (pcm, rate) = wav::read_wav(&std::fs::read(std::env::var("WH_TEST_WAV").unwrap()).unwrap()).unwrap();
        let pcm: Vec<u8> = wav::resample(&pcm, rate, wav::WHISPER_RATE).iter().flat_map(|s| ((s * i16::MAX as f32) as i16).to_le_bytes()).collect();
        println!("{}", call("dictate", "{}", &pcm));
    }
}
