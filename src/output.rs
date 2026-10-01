use crate::settings::{PasteMethod, Settings};
use enigo::{Direction, Enigo, Key, Keyboard};
use std::sync::Mutex;
use std::time::Duration;

pub struct Output {
    clipboard: Mutex<Option<arboard::Clipboard>>,
}

const MODIFIERS: [Key; 4] = [Key::Alt, Key::Shift, Key::Control, Key::Meta];

fn paste_keys(method: PasteMethod) -> (&'static [Key], Key) {
    match method {
        PasteMethod::CtrlV | PasteMethod::Type => (&[Key::Control], Key::Unicode('v')),
        PasteMethod::CtrlShiftV => (&[Key::Control, Key::Shift], Key::Unicode('v')),
        PasteMethod::ShiftInsert => (&[Key::Shift], Key::Insert),
    }
}

impl Output {
    pub fn new() -> Self {
        Self { clipboard: Mutex::new(arboard::Clipboard::new().ok()) }
    }

    pub fn copy(&self, text: &str) -> Result<(), String> {
        let mut guard = self.clipboard.lock().unwrap();
        if guard.is_none() {
            *guard = arboard::Clipboard::new().ok();
        }
        guard.as_mut().ok_or("Clipboard unavailable")?.set_text(text).map_err(|e| e.to_string())
    }

    fn read(&self) -> Option<String> {
        self.clipboard.lock().unwrap().as_mut()?.get_text().ok()
    }

    pub fn deliver(&self, text: &str, s: &Settings) -> Result<(), String> {
        if !s.output_cursor {
            return if s.output_clipboard { self.copy(text) } else { Ok(()) };
        }
        let mut enigo = Enigo::new(&enigo::Settings::default()).map_err(|e| format!("Keyboard control unavailable: {e}"))?;
        for m in MODIFIERS {
            let _ = enigo.key(m, Direction::Release);
        }
        if s.paste_method == PasteMethod::Type {
            enigo.text(text).map_err(|e| e.to_string())?;
            if s.output_clipboard {
                self.copy(text)?;
            }
        } else {
            let previous = (!s.output_clipboard).then(|| self.read()).flatten();
            self.copy(text)?;
            std::thread::sleep(Duration::from_millis(40));
            let (mods, key) = paste_keys(s.paste_method);
            let result = (|| {
                for &m in mods {
                    enigo.key(m, Direction::Press)?;
                }
                enigo.key(key, Direction::Click)?;
                for &m in mods.iter().rev() {
                    enigo.key(m, Direction::Release)?;
                }
                Ok::<_, enigo::InputError>(())
            })();
            if let Err(e) = result {
                return Err(format!("Copied to clipboard, but couldn't paste: {e}"));
            }
            if let Some(prev) = previous {
                std::thread::sleep(Duration::from_millis(250));
                let _ = self.copy(&prev);
            }
        }
        if s.output_enter {
            std::thread::sleep(Duration::from_millis(60));
            enigo.key(Key::Return, Direction::Click).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
