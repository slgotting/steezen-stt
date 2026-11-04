use crate::settings::{DictRule, Settings};
use argon2::Argon2;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde_json::{Map, Value, json};

pub const MARK: &str = "steezen_stt_settings";
pub const EXTENSION: &str = "steezen.json";

pub struct Group {
    pub id: &'static str,
    pub label: &'static str,
    pub fields: &'static [&'static str],
    pub secret: bool,
    pub desktop_only: bool,
}

pub const GROUPS: &[Group] = &[
    Group { id: "dictionary", label: "Dictionary", fields: &["corrector_enabled", "corrector_model", "dictionary"], secret: false, desktop_only: false },
    Group {
        id: "transcription",
        label: "Services & models",
        fields: &["provider", "fallback", "openai_model", "groq_model", "local_model", "local_use_gpu", "local_threads", "local_unload_minutes", "local_fallback", "language", "prompt", "temperature", "min_recording_ms"],
        secret: false,
        desktop_only: false,
    },
    Group { id: "output", label: "Output (clipboard, paste, Enter)", fields: &["output_clipboard", "output_cursor", "output_enter", "paste_method"], secret: false, desktop_only: false },
    Group { id: "formatting", label: "Formatting", fields: &["format_voice", "format_always", "format_model", "format_prompt"], secret: false, desktop_only: false },
    Group {
        id: "sounds",
        label: "Sounds & notifications",
        fields: &["sound_start", "sound_stop", "sound_cancel", "sound_complete", "notify_complete", "notify_error", "notify_fallback"],
        secret: false,
        desktop_only: false,
    },
    Group { id: "phone", label: "Phone (Zen address, gestures)", fields: &["zen_url", "volume_action", "assistant_action"], secret: false, desktop_only: false },
    Group { id: "keys", label: "API keys and Zen token", fields: &["openai_api_key", "groq_api_key", "typesafe_api_key", "zen_token"], secret: true, desktop_only: false },
    Group {
        id: "desktop",
        label: "Desktop (hotkeys, scripts, startup)",
        fields: &["push_to_talk", "toggle_shortcut", "cancel_shortcut", "format_push_to_talk", "scripts", "script_timeout_secs", "input_device", "close_to_tray", "start_hidden", "start_at_login", "history_limit", "keep_recordings", "probe_runs", "probe_gap_secs"],
        secret: false,
        desktop_only: true,
    },
];

pub fn group(id: &str) -> Option<&'static Group> {
    GROUPS.iter().find(|g| g.id == id)
}

fn key(pass: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    Argon2::default().hash_password_into(pass.as_bytes(), salt, &mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

fn lock(plain: &[u8], pass: &str) -> Result<Value, String> {
    let (salt, nonce) = (random::<16>()?, random::<24>()?);
    let cipher = XChaCha20Poly1305::new(&key(pass, &salt)?.into());
    let data = cipher.encrypt(&XNonce::from(nonce), plain).map_err(|_| "encryption failed".to_string())?;
    Ok(json!({"kdf": "argon2id", "cipher": "xchacha20poly1305", "salt": B64.encode(salt), "nonce": B64.encode(nonce), "data": B64.encode(data)}))
}

fn unlock(locked: &Value, pass: &str) -> Result<Vec<u8>, String> {
    let field = |name: &str| locked[name].as_str().ok_or(format!("the file is missing {name}")).and_then(|s| B64.decode(s).map_err(|e| e.to_string()));
    let (salt, nonce, data) = (field("salt")?, field("nonce")?, field("data")?);
    let nonce = XNonce::try_from(nonce.as_slice()).map_err(|_| "the file is damaged".to_string())?;
    let cipher = XChaCha20Poly1305::new(&key(pass, &salt)?.into());
    cipher.decrypt(&nonce, data.as_slice()).map_err(|_| "wrong passphrase, or the file is damaged".into())
}

pub fn export(settings: &Settings, groups: &[&str], pass: Option<&str>) -> Result<String, String> {
    let all = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    let pass = pass.map(str::trim).filter(|p| !p.is_empty());
    let mut chosen = Map::new();
    for id in groups {
        let g = group(id).ok_or(format!("unknown group {id}"))?;
        if g.secret && pass.is_none() {
            return Err(format!("{} can only be exported with a passphrase", g.label));
        }
        chosen.insert(g.id.into(), Value::Object(g.fields.iter().filter_map(|f| all.get(*f).map(|v| (f.to_string(), v.clone()))).collect()));
    }
    let groups = Value::Object(chosen);
    let body = match pass {
        Some(p) => json!({MARK: 1, "locked": lock(&serde_json::to_vec(&groups).map_err(|e| e.to_string())?, p)?}),
        None => json!({MARK: 1, "groups": groups}),
    };
    serde_json::to_string_pretty(&body).map_err(|e| e.to_string())
}

pub fn is_locked(text: &str) -> bool {
    serde_json::from_str::<Value>(text).is_ok_and(|v| v.get("locked").is_some())
}

pub fn open(text: &str, pass: Option<&str>) -> Result<Map<String, Value>, String> {
    let v: Value = serde_json::from_str(text).map_err(|_| "this is not a steezen-stt settings file".to_string())?;
    if v.get(MARK).is_none() {
        return Err("this is not a steezen-stt settings file".into());
    }
    let groups = match (v.get("locked"), pass.map(str::trim).filter(|p| !p.is_empty())) {
        (Some(_), None) => return Err("this file is locked; enter its passphrase".into()),
        (Some(locked), Some(p)) => serde_json::from_slice(&unlock(locked, p)?).map_err(|e| e.to_string())?,
        (None, _) => v["groups"].clone(),
    };
    match groups {
        Value::Object(m) => Ok(m.into_iter().filter(|(k, _)| group(k).is_some()).collect()),
        _ => Err("the file has no settings in it".into()),
    }
}

fn merge_dictionary(current: &[DictRule], incoming: Vec<DictRule>) -> Vec<DictRule> {
    let mut out = current.to_vec();
    for rule in incoming {
        match out.iter_mut().find(|r| r.word.eq_ignore_ascii_case(&rule.word)) {
            Some(existing) => *existing = rule,
            None => out.push(rule),
        }
    }
    out
}

pub fn apply(settings: &Settings, opened: &Map<String, Value>, choices: &[(&str, bool)]) -> Result<Settings, String> {
    let mut all = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    for (id, merge) in choices {
        let Some(Value::Object(fields)) = opened.get(*id) else { continue };
        for (name, value) in fields {
            let value = match (name.as_str(), merge) {
                ("dictionary", true) => serde_json::to_value(merge_dictionary(&settings.dictionary, serde_json::from_value(value.clone()).map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?,
                _ => value.clone(),
            };
            all[name] = value;
        }
    }
    serde_json::from_value(all).map_err(|e| format!("the file has a setting this version cannot read: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Settings {
        let mut s = Settings::default();
        s.openai_api_key = "sk-test".into();
        s.output_enter = true;
        s.dictionary = vec![DictRule { word: "Zen".into(), ..Default::default() }];
        s
    }

    #[test]
    fn plain_round_trip_applies_only_chosen_groups() {
        let text = export(&sample(), &["output", "dictionary"], None).unwrap();
        assert!(!text.contains("sk-test"));
        let opened = open(&text, None).unwrap();
        let out = apply(&Settings::default(), &opened, &[("output", false)]).unwrap();
        assert!(out.output_enter);
        assert_eq!(out.dictionary, Settings::default().dictionary);
    }

    #[test]
    fn keys_need_a_passphrase_and_the_right_one() {
        assert!(export(&sample(), &["keys"], None).is_err());
        let text = export(&sample(), &["keys"], Some("hunter2")).unwrap();
        assert!(!text.contains("sk-test") && is_locked(&text));
        assert!(open(&text, None).is_err());
        assert!(open(&text, Some("wrong")).is_err());
        let out = apply(&Settings::default(), &open(&text, Some("hunter2")).unwrap(), &[("keys", false)]).unwrap();
        assert_eq!(out.openai_api_key, "sk-test");
    }

    #[test]
    fn merge_keeps_existing_rules_and_replaces_same_word() {
        let mut mine = Settings::default();
        mine.dictionary = vec![DictRule { word: "mine".into(), ..Default::default() }, DictRule { word: "zen".into(), meaning: "old".into(), ..Default::default() }];
        let text = export(&sample(), &["dictionary"], None).unwrap();
        let opened = open(&text, None).unwrap();
        let merged = apply(&mine, &opened, &[("dictionary", true)]).unwrap();
        assert_eq!(merged.dictionary.iter().map(|r| r.word.as_str()).collect::<Vec<_>>(), ["mine", "Zen"]);
        let replaced = apply(&mine, &opened, &[("dictionary", false)]).unwrap();
        assert_eq!(replaced.dictionary.len(), 1);
    }

    #[test]
    fn every_group_field_exists_in_settings() {
        let all = serde_json::to_value(Settings::default()).unwrap();
        for g in GROUPS {
            for f in g.fields {
                assert!(all.get(*f).is_some(), "{f}");
            }
        }
    }
}
