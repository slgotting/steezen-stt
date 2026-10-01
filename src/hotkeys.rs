use crate::engine::{Cmd, Shared};
use crate::settings::Settings;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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

pub struct Hotkeys {
    manager: Option<GlobalHotKeyManager>,
    active: Arc<Mutex<HashMap<u32, (Action, HotKey)>>>,
    current: Vec<String>,
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
        Self { manager, active, current: Vec::new() }
    }

    pub fn sync(&mut self, settings: &Settings) -> Vec<String> {
        let wanted: Vec<String> = ACTIONS.iter().map(|(_, get)| get(settings).trim().to_string()).collect();
        if wanted == self.current {
            return Vec::new();
        }
        self.current = wanted;
        let Some(manager) = &self.manager else { return vec!["Global hotkeys are unavailable on this system".into()] };
        let mut active = self.active.lock().unwrap();
        for (_, (_, hk)) in active.drain() {
            let _ = manager.unregister(hk);
        }
        let mut errors = Vec::new();
        for (action, get) in ACTIONS {
            match parse(get(settings)) {
                Ok(Some(hk)) => match manager.register(hk) {
                    Ok(()) => {
                        active.insert(hk.id(), (action, hk));
                    }
                    Err(e) => errors.push(format!("Couldn't register {}: {e}", get(settings))),
                },
                Ok(None) => {}
                Err(e) => errors.push(format!("Invalid shortcut '{}': {e}", get(settings))),
            }
        }
        errors
    }
}
