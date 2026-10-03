use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const APP_NAME: &str = "steezen-stt";

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Provider {
    OpenAI,
    Groq,
    Local,
}

pub struct ProviderInfo {
    pub label: &'static str,
    pub base_url: &'static str,
    pub key_env: &'static str,
    pub model_filter: &'static [&'static str],
    pub fallback_models: &'static [&'static str],
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::OpenAI, Provider::Groq, Provider::Local];

    pub fn info(self) -> &'static ProviderInfo {
        match self {
            Provider::OpenAI => &ProviderInfo {
                label: "OpenAI",
                base_url: "https://api.openai.com/v1",
                key_env: "OPENAI_API_KEY",
                model_filter: &["whisper", "transcribe"],
                fallback_models: &["gpt-4o-transcribe", "gpt-4o-mini-transcribe", "whisper-1"],
            },
            Provider::Groq => &ProviderInfo {
                label: "Groq",
                base_url: "https://api.groq.com/openai/v1",
                key_env: "GROQ_API_KEY",
                model_filter: &["whisper"],
                fallback_models: &["whisper-large-v3-turbo", "whisper-large-v3"],
            },
            Provider::Local => &ProviderInfo {
                label: "Local (whisper.cpp)",
                base_url: "https://huggingface.co/api/models/ggerganov/whisper.cpp",
                key_env: "",
                model_filter: &["ggml-"],
                fallback_models: &["ggml-base.en.bin", "ggml-small.en.bin", "ggml-large-v3-turbo-q5_0.bin"],
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PasteMethod {
    CtrlV,
    CtrlShiftV,
    ShiftInsert,
    Type,
}

impl PasteMethod {
    pub const ALL: [PasteMethod; 4] = [PasteMethod::CtrlV, PasteMethod::CtrlShiftV, PasteMethod::ShiftInsert, PasteMethod::Type];

    pub fn label(self) -> &'static str {
        match self {
            PasteMethod::CtrlV => "Ctrl+V (most apps)",
            PasteMethod::CtrlShiftV => "Ctrl+Shift+V (terminals)",
            PasteMethod::ShiftInsert => "Shift+Insert",
            PasteMethod::Type => "Type the text (no clipboard)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Stage {
    RecordingStart,
    BeforeTranscription,
    AfterTranscription,
    AfterDelivery,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::RecordingStart, Stage::BeforeTranscription, Stage::AfterTranscription, Stage::AfterDelivery];

    pub fn label(self) -> &'static str {
        match self {
            Stage::RecordingStart => "When recording starts",
            Stage::BeforeTranscription => "Before transcription",
            Stage::AfterTranscription => "After transcription (text filter)",
            Stage::AfterDelivery => "After delivery",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Stage::RecordingStart => "Runs in the background as recording begins (e.g. `playerctl pause`). Output is ignored.",
            Stage::BeforeTranscription => "Runs after recording stops, before the audio is sent. $WH_AUDIO holds a 16 kHz WAV path. Output is ignored.",
            Stage::AfterTranscription => "Receives the transcript on stdin; its stdout replaces the transcript. Filters run in order, after the dictionary. On failure the text passes through unchanged.",
            Stage::AfterDelivery => "Receives the delivered text on stdin and in $WH_TEXT after it has been pasted/copied. Output is ignored.",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Script {
    pub enabled: bool,
    pub name: String,
    pub command: String,
    pub stage: Stage,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Alternative {
    pub word: String,
    pub meaning: String,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DictRule {
    pub enabled: bool,
    pub word: String,
    pub triggers: String,
    pub meaning: String,
    pub alternatives: Vec<Alternative>,
    pub threshold: f32,
    pub context: String,
}

impl Default for DictRule {
    fn default() -> Self {
        Self {
            enabled: true,
            word: String::new(),
            triggers: String::new(),
            meaning: String::new(),
            alternatives: Vec::new(),
            threshold: 0.6,
            context: String::new(),
        }
    }
}

impl DictRule {
    pub fn trigger_list(&self) -> Vec<String> {
        self.triggers.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
    }
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct JevReminder {
    pub never: bool,
    pub snoozes: u32,
    pub until: Option<chrono::DateTime<chrono::Local>>,
}

impl JevReminder {
    const BASE_HOURS: i64 = 24;
    const MAX_HOURS: i64 = 24 * 30;

    pub fn due(&self) -> bool {
        !self.never && self.until.is_none_or(|t| chrono::Local::now() >= t)
    }

    pub fn snooze(&mut self) -> chrono::Duration {
        let hours = (Self::BASE_HOURS << self.snoozes.min(10)).min(Self::MAX_HOURS);
        self.snoozes += 1;
        let wait = chrono::Duration::hours(hours);
        self.until = Some(chrono::Local::now() + wait);
        wait
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub provider: Provider,
    pub openai_api_key: String,
    pub groq_api_key: String,
    pub typesafe_api_key: String,
    pub openai_model: String,
    pub groq_model: String,
    pub local_model: String,
    pub local_use_gpu: bool,
    pub local_threads: u32,
    pub local_unload_minutes: u32,
    pub language: String,
    pub prompt: String,
    pub temperature: f32,

    pub input_device: String,
    pub min_recording_ms: u32,

    pub push_to_talk: String,
    pub toggle_shortcut: String,
    pub cancel_shortcut: String,
    pub format_push_to_talk: String,

    pub output_clipboard: bool,
    pub output_cursor: bool,
    pub output_enter: bool,
    pub paste_method: PasteMethod,

    pub sound_start: bool,
    pub sound_stop: bool,
    pub sound_cancel: bool,
    pub sound_complete: bool,
    pub notify_complete: bool,
    pub notify_error: bool,

    pub corrector_enabled: bool,
    pub corrector_model: String,
    pub dictionary: Vec<DictRule>,
    pub jev_reminder: JevReminder,

    pub format_voice: bool,
    pub format_always: bool,
    pub format_model: String,
    pub format_prompt: String,

    pub scripts: Vec<Script>,
    pub script_timeout_secs: u32,

    pub history_limit: usize,
    pub close_to_tray: bool,
    pub start_hidden: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::OpenAI,
            openai_api_key: String::new(),
            groq_api_key: String::new(),
            typesafe_api_key: String::new(),
            openai_model: "gpt-4o-transcribe".into(),
            groq_model: "whisper-large-v3-turbo".into(),
            local_model: "ggml-base.en.bin".into(),
            local_use_gpu: true,
            local_threads: 0,
            local_unload_minutes: 5,
            language: String::new(),
            prompt: String::new(),
            temperature: 0.0,
            input_device: String::new(),
            min_recording_ms: 300,
            push_to_talk: "Alt+Shift+D".into(),
            toggle_shortcut: String::new(),
            cancel_shortcut: String::new(),
            format_push_to_talk: "Alt+Shift+F".into(),
            output_clipboard: true,
            output_cursor: true,
            output_enter: false,
            paste_method: PasteMethod::CtrlV,
            sound_start: false,
            sound_stop: false,
            sound_cancel: false,
            sound_complete: false,
            notify_complete: false,
            notify_error: false,
            corrector_enabled: true,
            corrector_model: "jev-latest".into(),
            jev_reminder: JevReminder::default(),
            format_voice: true,
            format_always: false,
            format_model: "gpt-4.1-mini".into(),
            format_prompt: crate::formatter::DEFAULT_PROMPT.into(),
            dictionary: serde_json::from_str(include_str!("../assets/default_dictionary.json")).unwrap_or_default(),
            scripts: Vec::new(),
            script_timeout_secs: 10,
            history_limit: 200,
            close_to_tray: true,
            start_hidden: false,
        }
    }
}

impl Settings {
    pub fn model(&self, provider: Provider) -> &str {
        match provider {
            Provider::OpenAI => &self.openai_model,
            Provider::Groq => &self.groq_model,
            Provider::Local => &self.local_model,
        }
    }

    pub fn model_mut(&mut self, provider: Provider) -> &mut String {
        match provider {
            Provider::OpenAI => &mut self.openai_model,
            Provider::Groq => &mut self.groq_model,
            Provider::Local => &mut self.local_model,
        }
    }

    pub fn key_field(&mut self, provider: Provider) -> Option<&mut String> {
        match provider {
            Provider::OpenAI => Some(&mut self.openai_api_key),
            Provider::Groq => Some(&mut self.groq_api_key),
            Provider::Local => None,
        }
    }

    pub fn api_key(&self, provider: Provider) -> String {
        let stored = match provider {
            Provider::OpenAI => &self.openai_api_key,
            Provider::Groq => &self.groq_api_key,
            Provider::Local => return String::new(),
        };
        non_empty_or_env(stored, provider.info().key_env)
    }

    pub fn typesafe_key(&self) -> String {
        non_empty_or_env(&self.typesafe_api_key, "TYPESAFE_API_KEY")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        write_private(&config_path(), serde_json::to_string_pretty(self)?.as_bytes())
    }
}

fn non_empty_or_env(stored: &str, env: &str) -> String {
    let stored = stored.trim();
    if stored.is_empty() { std::env::var(env).unwrap_or_default().trim().to_string() } else { stored.to_string() }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join(APP_NAME)
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join(APP_NAME)
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join(APP_NAME)
}

fn config_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    std::io::Write::write_all(&mut opts.open(&tmp)?, bytes)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::JevReminder;

    #[test]
    fn reminder_backs_off_exponentially_and_caps() {
        let mut r = JevReminder::default();
        assert!(r.due());
        let waits: Vec<i64> = (0..7).map(|_| r.snooze().num_hours()).collect();
        assert_eq!(waits, [24, 48, 96, 192, 384, 720, 720]);
        assert!(!r.due());
        r.until = None;
        r.never = true;
        assert!(!r.due());
    }
}
