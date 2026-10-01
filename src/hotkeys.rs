use crate::engine::{Cmd, Shared};
use crate::settings::Settings;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    PushToTalk,
    Toggle,
    Cancel,
}

type Getter = fn(&Settings) -> &str;

const ACTIONS: [(Action, Getter); 3] = [
    (Action::PushToTalk, |s| &s.push_to_talk),
    (Action::Toggle, |s| &s.toggle_shortcut),
    (Action::Cancel, |s| &s.cancel_shortcut),
];

const RETRY: Duration = Duration::from_secs(2);

pub struct Hotkeys {
    manager: Option<GlobalHotKeyManager>,
    active: Arc<Mutex<HashMap<u32, (Action, HotKey)>>>,
    current: Vec<String>,
    retry_at: Option<Instant>,
    pub errors: Vec<String>,
}

pub fn parse(text: &str) -> Result<Option<HotKey>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<HotKey>().map(Some).map_err(|e| e.to_string())
}

impl Hotkeys {
    pub fn new(shared: Arc<Shared>) -> Self {
        let active: Arc<Mutex<HashMap<u32, (Action, HotKey)>>> = Arc::default();
        let lookup = active.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            let Some((action, _)) = lookup.lock().unwrap().get(&e.id).copied() else { return };
            let cmd = match (action, e.state) {
                (Action::PushToTalk, HotKeyState::Pressed) => Cmd::PttDown,
                (Action::PushToTalk, HotKeyState::Released) => Cmd::PttUp,
                (Action::Toggle, HotKeyState::Pressed) => Cmd::Toggle,
                (Action::Cancel, HotKeyState::Pressed) => Cmd::Cancel,
                _ => return,
            };
            shared.send(cmd);
        }));
        let manager = GlobalHotKeyManager::new().map_err(|e| eprintln!("global hotkeys unavailable: {e}")).ok();
        Self { manager, active, current: Vec::new(), retry_at: None, errors: Vec::new() }
    }

    pub fn sync(&mut self, settings: &Settings) {
        let wanted: Vec<String> = ACTIONS.iter().map(|(_, get)| get(settings).trim().to_string()).collect();
        if wanted == self.current {
            return;
        }
        self.current = wanted;
        self.retry_at = None;
        let Some(manager) = &self.manager else {
            self.errors = vec!["Global hotkeys are unavailable on this system".into()];
            return;
        };
        let old: Vec<HotKey> = self.active.lock().unwrap().drain().map(|(_, (_, hk))| hk).collect();
        for hk in old {
            let _ = manager.unregister(hk);
        }
        let mut fresh = HashMap::new();
        self.errors.clear();
        for (action, get) in ACTIONS {
            let text = get(settings);
            match parse(text) {
                Ok(Some(hk)) => match manager.register(hk) {
                    Ok(()) => {
                        fresh.insert(hk.id(), (action, hk));
                    }
                    Err(e) => {
                        let reason = match e {
                            global_hotkey::Error::AlreadyRegistered(_) => "another program (or another copy of steezen-stt) is using it".to_string(),
                            other => other.to_string(),
                        };
                        self.errors.push(format!("Couldn't register {text}: {reason}. Retrying…"));
                        self.retry_at = Some(Instant::now() + RETRY);
                    }
                },
                Ok(None) => {}
                Err(e) => self.errors.push(format!("Invalid shortcut '{text}': {e}")),
            }
        }
        *self.active.lock().unwrap() = fresh;
    }

    pub fn retry(&mut self, settings: &Settings) -> Option<Duration> {
        let at = self.retry_at?;
        if Instant::now() >= at {
            self.current.clear();
            self.sync(settings);
        }
        self.retry_at.map(|t| t.saturating_duration_since(Instant::now()))
    }
}
