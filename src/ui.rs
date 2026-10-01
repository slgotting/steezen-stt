use crate::corrector;
use crate::engine::{Cmd, Entry, Phase, Shared};
use crate::hotkeys::{self, Hotkeys};
use crate::settings::{Alternative, DictRule, PasteMethod, Provider, Script, Settings, Stage, config_dir, data_dir};
use crate::transcribe;
use crate::tray::{self, Tray};
use crate::theme::{self, ACCENT, ACCENT_HI, AMBER, BAR, BORDER, GREEN, MUTED, RED, TEXT, bold, card, muted};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Ui, Vec2, ViewportCommand};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PADDING: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    History,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Transcription,
    Recording,
    Shortcuts,
    Output,
    Dictionary,
    Scripts,
    Feedback,
    General,
}

const PAGES: [(Page, &str); 3] = [(Page::Home, "Home"), (Page::History, "History"), (Page::Settings, "Settings")];

const SECTIONS: [(Section, &str, &str); 8] = [
    (Section::Transcription, "Transcription", "Where your audio goes and which model turns it into text."),
    (Section::Recording, "Recording", "Microphone and recording behaviour."),
    (Section::Shortcuts, "Shortcuts", "Global keys, e.g. Alt+Shift+D, Ctrl+Space or F9. Click Record and press the combination."),
    (Section::Output, "Output", "What happens to the final text, after the dictionary and every text-filter script have run."),
    (Section::Dictionary, "Dictionary", "Words speech-to-text often gets wrong. Jev classifies each occurrence in context; the app replaces it only above the threshold. No LLM rewrites your text."),
    (Section::Scripts, "Scripts", "Shell commands that run at each stage of the pipeline, in order."),
    (Section::Feedback, "Sounds & notifications", "Audible and desktop feedback. Everything is off by default."),
    (Section::General, "General", "Window, tray and history."),
];

type Toggle = (&'static str, &'static str, fn(&mut Settings) -> &mut bool);

const OUTPUT_TOGGLES: [Toggle; 3] = [
    ("Copy transcript to clipboard", "Leave the final text on the clipboard after delivery.", |s| &mut s.output_clipboard),
    ("Paste transcript at cursor", "Paste into the focused window when transcription finishes.", |s| &mut s.output_cursor),
    ("Press Enter after pasting", "Send an Enter keystroke after the text is pasted (e.g. to submit a chat message).", |s| &mut s.output_enter),
];

const FEEDBACK_TOGGLES: [Toggle; 6] = [
    ("Sound when recording starts", "", |s| &mut s.sound_start),
    ("Sound when recording stops", "", |s| &mut s.sound_stop),
    ("Sound when recording is cancelled", "", |s| &mut s.sound_cancel),
    ("Sound when transcription completes", "", |s| &mut s.sound_complete),
    ("Notification when transcription completes", "", |s| &mut s.notify_complete),
    ("Notification on errors", "", |s| &mut s.notify_error),
];

type Field = fn(&mut Settings) -> &mut String;

const SHORTCUTS: [(&str, &str, Field); 3] = [
    ("Push to talk", "Hold to record; release to transcribe.", |s| &mut s.push_to_talk),
    ("Toggle recording", "Press once to start, again to stop. Optional.", |s| &mut s.toggle_shortcut),
    ("Cancel recording", "Discard the current recording. Optional.", |s| &mut s.cancel_shortcut),
];

pub struct App {
    shared: Arc<Shared>,
    draft: Settings,
    dirty_since: Option<Instant>,
    hide_on_start: bool,
    page: Page,
    section: Section,
    hotkeys: Hotkeys,
    tray: Tray,
    devices: Vec<String>,
    capturing: Option<usize>,
    show_keys: bool,
    test_input: String,
    test_output: Arc<Mutex<Option<String>>>,
    installed: Vec<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, shared: Arc<Shared>) -> Self {
        let _ = shared.ctx.set(cc.egui_ctx.clone());
        theme::install(&cc.egui_ctx);
        let draft = shared.settings();
        let mut hotkeys = Hotkeys::new(shared.clone());
        hotkeys.sync(&draft);
        let tray = Tray::new(shared.clone());
        for p in Provider::ALL {
            shared.refresh_models(p);
        }
        Self {
            hide_on_start: draft.start_hidden,
            draft,
            dirty_since: None,
            page: Page::Home,
            section: Section::Transcription,
            hotkeys,
            tray,
            devices: crate::audio::input_devices(),
            capturing: None,
            show_keys: false,
            test_input: "I asked cloud to fix the sequel query.".into(),
            test_output: Arc::default(),
            installed: transcribe::installed_local_models(),
            shared,
        }
    }

    fn persist(&mut self, ctx: &egui::Context) {
        if *self.shared.settings.read().unwrap() != self.draft {
            *self.shared.settings.write().unwrap() = self.draft.clone();
            self.dirty_since = Some(Instant::now());
            self.hotkeys.sync(&self.draft);
        }
        if let Some(t) = self.dirty_since {
            if t.elapsed() >= Duration::from_millis(500) {
                self.dirty_since = None;
                if let Err(e) = self.draft.save() {
                    self.shared.set_notice(format!("Couldn't save settings: {e}"), true);
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
    }
}

fn phase_badge(phase: Phase) -> (Color32, &'static str) {
    match phase {
        Phase::Idle => (GREEN, "Ready"),
        Phase::Recording => (RED, "Recording"),
        Phase::Transcribing => (AMBER, "Transcribing"),
    }
}

fn pill(ui: &mut Ui, selected: bool, text: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(14.0), TEXT);
    let size = galley.size() + Vec2::new(24.0, 12.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = match (selected, resp.hovered()) {
        (true, _) => ACCENT,
        (false, true) => Color32::from_rgb(34, 39, 52),
        _ => Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(255), fill);
    let color = if selected || resp.hovered() { Color32::WHITE } else { MUTED };
    ui.painter().galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

fn status_chip(ui: &mut Ui, phase: Phase) {
    let (color, label) = phase_badge(phase);
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), color);
    let (rect, _) = ui.allocate_exact_size(galley.size() + Vec2::new(36.0, 10.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, egui::CornerRadius::same(255), color.gamma_multiply(0.15));
    p.circle_filled(rect.left_center() + Vec2::new(14.0, 0.0), 4.0, color);
    p.galley(rect.left_center() + Vec2::new(24.0, -galley.size().y / 2.0), galley, color);
}

fn record_button(ui: &mut Ui, phase: Phase) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(180.0), Sense::click());
    let p = ui.painter();
    let c = rect.center();
    let t = ui.input(|i| i.time) as f32;
    let (fill, glow) = match phase {
        Phase::Idle => (ACCENT, ACCENT_HI),
        Phase::Recording => (RED, RED),
        Phase::Transcribing => (AMBER, AMBER),
    };
    let base = if resp.hovered() { 66.0 } else { 64.0 };
    let pulse = if phase == Phase::Recording { (t * 3.0).sin() * 0.5 + 0.5 } else { 0.0 };
    for (extra, alpha) in [(26.0 + 8.0 * pulse, 0.05), (16.0 + 5.0 * pulse, 0.09), (8.0, 0.16)] {
        p.circle_filled(c, base + extra, glow.gamma_multiply(alpha));
    }
    p.circle_filled(c, base, fill);
    p.circle_stroke(c, base, Stroke::new(1.5, Color32::from_white_alpha(40)));
    let white = Stroke::new(3.5, Color32::WHITE);
    match phase {
        Phase::Idle => {
            p.rect_filled(egui::Rect::from_center_size(c + Vec2::new(0.0, -8.0), Vec2::new(18.0, 32.0)), egui::CornerRadius::same(9), Color32::WHITE);
            let arc: Vec<egui::Pos2> = (0..=16).map(|i| {
                let a = std::f32::consts::PI * i as f32 / 16.0;
                c + Vec2::new(-15.0 * a.cos(), -4.0 + 15.0 * a.sin())
            }).collect();
            p.add(egui::Shape::line(arc, white));
            p.line_segment([c + Vec2::new(0.0, 11.0), c + Vec2::new(0.0, 22.0)], white);
            p.line_segment([c + Vec2::new(-9.0, 22.0), c + Vec2::new(9.0, 22.0)], white);
        }
        Phase::Recording => {
            p.rect_filled(egui::Rect::from_center_size(c, Vec2::splat(30.0)), egui::CornerRadius::same(6), Color32::WHITE);
        }
        Phase::Transcribing => {
            let start = t * 4.0;
            let arc: Vec<egui::Pos2> = (0..=24).map(|i| {
                let a = start + 4.7 * i as f32 / 24.0;
                c + Vec2::new(22.0 * a.cos(), 22.0 * a.sin())
            }).collect();
            p.add(egui::Shape::line(arc, Stroke::new(4.0, Color32::WHITE)));
        }
    }
    if phase != Phase::Idle {
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
    resp
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tray.update(self.shared.phase());
        if let Some(wait) = self.hotkeys.retry(&self.draft) {
            ctx.request_repaint_after(wait);
        }
        if std::mem::take(&mut self.hide_on_start) {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.draft.close_to_tray && !tray::QUITTING.load(std::sync::atomic::Ordering::SeqCst) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let phase = self.shared.phase();
        let bar = egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(12, 8));
        egui::Panel::top("nav").frame(bar).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 10.0, ACCENT);
                ui.painter().circle_filled(r.center(), 4.0, Color32::WHITE);
                ui.label(bold("whisper-hook", 16.0).color(TEXT));
                ui.add_space(14.0);
                for (page, label) in PAGES {
                    if pill(ui, self.page == page, label).clicked() {
                        self.page = page;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| status_chip(ui, phase));
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::central_panel(ui.style()).inner_margin(PADDING)).show(ui, |ui| match self.page {
            Page::Home => self.home(ui),
            Page::History => self.history(ui),
            Page::Settings => self.settings(ui),
        });
        let ctx = ui.ctx().clone();
        self.persist(&ctx);
    }
}

fn notice(ui: &mut Ui, shared: &Shared) {
    let current = shared.notice.lock().unwrap().clone();
    if let Some((text, error)) = current {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&text).color(if error { RED } else { MUTED }));
            if ui.small_button("✖").clicked() {
                *shared.notice.lock().unwrap() = None;
            }
        });
    }
}

fn copy_button(ui: &mut Ui, shared: &Shared, label: &str, text: &str) {
    if ui.small_button(label).clicked()
        && let Err(e) = shared.output.copy(text) {
            shared.set_notice(format!("Copy failed: {e}"), true);
        }
}

fn entry_body(ui: &mut Ui, shared: &Shared, e: &Entry) {
    ui.horizontal_wrapped(|ui| {
        ui.label(bold(e.time.format("%b %d, %H:%M").to_string(), 13.0).color(TEXT));
        ui.label(muted(format!("{} · {} · {:.1}s", e.provider.info().label, e.model, e.duration_secs)).small());
    });
    if !e.text.is_empty() {
        ui.add(egui::Label::new(RichText::new(&e.text).size(15.0)).selectable(true).wrap());
    }
    let changed: Vec<String> = e.corrections.iter().filter_map(|c| c.to.as_ref().map(|to| format!("{} → {to} ({:.0}%)", c.from, c.probability * 100.0))).collect();
    if !changed.is_empty() {
        ui.label(RichText::new(format!("Dictionary: {}", changed.join(", "))).color(ACCENT_HI).small());
    }
    if let Some(err) = &e.error {
        ui.label(RichText::new(err).color(RED).small());
    }
    ui.horizontal(|ui| {
        copy_button(ui, shared, "Copy", &e.text);
        if e.raw != e.text && !e.raw.is_empty() {
            copy_button(ui, shared, "Copy original", &e.raw);
            ui.collapsing("Original transcript", |ui| {
                ui.add(egui::Label::new(&e.raw).selectable(true).wrap());
            });
        }
    });
}

impl App {
    fn home(&mut self, ui: &mut Ui) {
        let phase = self.shared.phase();
        ui.vertical_centered(|ui| {
            ui.add_space(8.0);
            if record_button(ui, phase).on_hover_text("Click to start or stop recording").clicked() {
                self.shared.send(Cmd::Toggle);
            }
            let since = *self.shared.recording_since.lock().unwrap();
            let status = match (phase, since) {
                (Phase::Recording, Some(t)) => {
                    let s = t.elapsed().as_secs();
                    format!("Recording {}:{:02}", s / 60, s % 60)
                }
                (Phase::Transcribing, _) => "Transcribing…".into(),
                _ if self.draft.push_to_talk.trim().is_empty() => "Click the button to record".into(),
                _ => format!("Hold {} to talk", self.draft.push_to_talk),
            };
            ui.label(bold(status, 17.0).color(TEXT));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let width = 2.0 * 220.0 + 16.0;
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                provider_combo(ui, &mut self.draft.provider);
                model_combo(ui, &self.shared, &mut self.draft, &self.installed);
            });
            for e in &self.hotkeys.errors {
                ui.label(RichText::new(e).color(RED));
            }
            notice(ui, &self.shared);
        });
        ui.add_space(12.0);
        let latest = self.shared.history.lock().unwrap().last().cloned();
        if let Some(e) = latest {
            ui.label(bold("Latest transcript", 15.0).color(MUTED));
            card(ui, |ui| entry_body(ui, &self.shared, &e));
        }
    }

    fn history(&mut self, ui: &mut Ui) {
        let entries = self.shared.history.lock().unwrap().clone();
        ui.horizontal(|ui| {
            ui.label(bold("History", 20.0).color(TEXT));
            ui.label(muted(format!("{} entries", entries.len())));
            if !entries.is_empty() && ui.button("Clear all").on_hover_text("Double-click to delete every entry").double_clicked() {
                self.shared.history.lock().unwrap().clear();
                self.shared.save_history();
            }
        });
        let mut delete = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for (i, e) in entries.iter().enumerate().rev() {
                card(ui, |ui| {
                    entry_body(ui, &self.shared, e);
                    if ui.small_button("Delete").clicked() {
                        delete = Some(i);
                    }
                });
            }
        });
        if let Some(i) = delete {
            self.shared.history.lock().unwrap().remove(i);
            self.shared.save_history();
        }
    }

    fn settings(&mut self, ui: &mut Ui) {
        egui::Panel::left("sections").resizable(false).exact_size(190.0).frame(egui::Frame::NONE.inner_margin(egui::Margin { right: 8, ..Default::default() })).show(ui, |ui| {
            ui.add_space(4.0);
            for (section, label, _) in SECTIONS {
                if nav_item(ui, self.section == section, label).clicked() {
                    self.section = section;
                }
            }
        });
        egui::Frame::NONE.inner_margin(egui::Margin { left: 8, ..Default::default() }).show(ui, |ui| egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.add_space(2.0);
            let (_, title, subtitle) = SECTIONS.iter().find(|(s, _, _)| *s == self.section).copied().unwrap_or(SECTIONS[0]);
            theme::section_title(ui, title, subtitle);
            card(ui, |ui| match self.section {
                Section::Transcription => self.transcription_settings(ui),
                Section::Recording => self.recording_settings(ui),
                Section::Shortcuts => self.shortcut_settings(ui),
                Section::Output => self.output_settings(ui),
                Section::Dictionary => self.dictionary_settings(ui),
                Section::Scripts => self.script_settings(ui),
                Section::Feedback => toggles(ui, &mut self.draft, &FEEDBACK_TOGGLES),
                Section::General => self.general_settings(ui),
            });
            ui.add_space(24.0);
        }));
    }

    fn transcription_settings(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Service");
            provider_combo(ui, &mut self.draft.provider);
        });
        let provider = self.draft.provider;
        if let Some(key) = self.draft.key_field(provider) {
            ui.horizontal(|ui| {
                ui.label(format!("{} API key", provider.info().label));
                ui.add(egui::TextEdit::singleline(key).password(!self.show_keys).desired_width(320.0).hint_text(format!("or set ${}", provider.info().key_env)));
                ui.checkbox(&mut self.show_keys, "Show");
            });
        }
        ui.horizontal(|ui| {
            ui.label("Model");
            model_combo(ui, &self.shared, &mut self.draft, &self.installed);
            if ui.button("⟳ Refresh list").clicked() {
                self.shared.refresh_models(provider);
                self.installed = transcribe::installed_local_models();
            }
        });
        if let Some(Err(e)) = self.shared.models.lock().unwrap().get(&provider) {
            ui.label(RichText::new(format!("Couldn't fetch models: {e}")).color(MUTED).small());
        }
        if provider == Provider::Local {
            self.local_settings(ui);
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Language");
            ui.add(egui::TextEdit::singleline(&mut self.draft.language).desired_width(80.0).hint_text("auto"));
            ui.label(RichText::new("ISO code such as en, de, es. Blank = auto-detect.").color(MUTED).small());
        });
        ui.label("Prompt (vocabulary and style hints passed to the model)");
        ui.add(egui::TextEdit::multiline(&mut self.draft.prompt).desired_rows(2).desired_width(f32::INFINITY));
        ui.add(egui::Slider::new(&mut self.draft.temperature, 0.0..=1.0).text("Temperature"));
    }

    fn local_settings(&mut self, ui: &mut Ui) {
        let model = self.draft.local_model.clone();
        let downloaded = self.installed.contains(&model);
        let download = self.shared.download.lock().unwrap().clone();
        ui.horizontal(|ui| match (&download, downloaded) {
            (Some((name, done, total)), _) => {
                let fraction = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).text(format!("Downloading {name}: {} / {} MB", done >> 20, total >> 20)).desired_width(360.0));
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
            (None, true) => {
                ui.label(RichText::new(format!("✔ {model} is downloaded")).color(MUTED));
                self.installed = transcribe::installed_local_models();
            }
            (None, false) => {
                ui.label(RichText::new(format!("{model} is not downloaded")).color(AMBER));
                if ui.button("⬇ Download").clicked() {
                    self.shared.download_model(model.clone());
                }
            }
        });
        if download.is_none() && !downloaded {
            self.installed = transcribe::installed_local_models();
        }
        let gpu_build = cfg!(any(feature = "cuda", feature = "vulkan"));
        ui.checkbox(&mut self.draft.local_use_gpu, "Use GPU when available");
        if !gpu_build {
            ui.label(RichText::new("This build is CPU-only. Build with `--features cuda` or `--features vulkan` for GPU support.").color(MUTED).small());
        }
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.draft.local_threads).range(0..=64));
            ui.label("CPU threads (0 = automatic)");
        });
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.draft.local_unload_minutes).range(0..=1440));
            ui.label("Unload model after this many idle minutes (0 = keep loaded)");
        });
        ui.label(RichText::new(format!("Models folder: {}", transcribe::models_dir().display())).color(MUTED).small());
        if self.shared.local.is_loaded() {
            ui.label(RichText::new("Model is loaded in memory").color(MUTED).small());
        }
    }

    fn recording_settings(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Microphone");
            let current = if self.draft.input_device.is_empty() { "System default".to_string() } else { self.draft.input_device.clone() };
            egui::ComboBox::from_id_salt("mic").selected_text(current).width(320.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.draft.input_device, String::new(), "System default");
                for d in &self.devices {
                    ui.selectable_value(&mut self.draft.input_device, d.clone(), d);
                }
            });
            if ui.button("⟳").on_hover_text("Rescan devices").clicked() {
                self.devices = crate::audio::input_devices();
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.draft.min_recording_ms).range(0..=5000).suffix(" ms"));
            ui.label("Ignore recordings shorter than this (accidental taps)");
        });
    }

    fn shortcut_settings(&mut self, ui: &mut Ui) {
        if let Some(i) = self.capturing
            && let Some(combo) = capture_combo(ui) {
                if !combo.is_empty() {
                    *(SHORTCUTS[i].2)(&mut self.draft) = combo;
                }
                self.capturing = None;
            }
        egui::Grid::new("shortcuts").num_columns(3).spacing([12.0, 10.0]).show(ui, |ui| {
            for (i, (label, help, field)) in SHORTCUTS.iter().enumerate() {
                ui.label(*label).on_hover_text(*help);
                let value = field(&mut self.draft);
                let valid = hotkeys::parse(value).is_ok();
                ui.add_sized([200.0, 26.0], egui::TextEdit::singleline(value).text_color_opt((!valid).then_some(RED)));
                ui.horizontal(|ui| {
                    let recording = self.capturing == Some(i);
                    if ui.selectable_label(recording, if recording { "Press keys… (Esc cancels)" } else { "⏺ Record" }).clicked() {
                        self.capturing = if recording { None } else { Some(i) };
                    }
                    if ui.small_button("Clear").clicked() {
                        field(&mut self.draft).clear();
                    }
                });
                ui.end_row();
            }
        });
        for e in &self.hotkeys.errors {
            ui.label(RichText::new(e).color(RED));
        }
        ui.label(RichText::new("Global shortcuts work on X11 and Windows. Wayland sessions do not allow apps to grab global keys.").color(MUTED).small());
    }

    fn output_settings(&mut self, ui: &mut Ui) {
        toggles(ui, &mut self.draft, &OUTPUT_TOGGLES);
        ui.horizontal(|ui| {
            ui.label("Paste using");
            egui::ComboBox::from_id_salt("paste").selected_text(self.draft.paste_method.label()).width(260.0).show_ui(ui, |ui| {
                for m in PasteMethod::ALL {
                    ui.selectable_value(&mut self.draft.paste_method, m, m.label());
                }
            });
        });
        ui.label(RichText::new("When pasting without \"Copy transcript to clipboard\", your previous clipboard is restored afterwards.").color(MUTED).small());
    }

    fn dictionary_settings(&mut self, ui: &mut Ui) {
        ui.checkbox(&mut self.draft.corrector_enabled, "Enable dictionary corrections");
        ui.horizontal(|ui| {
            ui.label("TypeSafe API key");
            ui.add(egui::TextEdit::singleline(&mut self.draft.typesafe_api_key).password(!self.show_keys).desired_width(320.0).hint_text("or set $TYPESAFE_API_KEY"));
            ui.checkbox(&mut self.show_keys, "Show");
        });
        ui.horizontal(|ui| {
            ui.label("Jev model");
            ui.add(egui::TextEdit::singleline(&mut self.draft.corrector_model).desired_width(160.0));
        });
        self.dictionary_test(ui);
        ui.separator();
        let mut remove = None;
        for (i, rule) in self.draft.dictionary.iter_mut().enumerate() {
            let id = ui.make_persistent_id(("rule", i));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                .show_header(ui, |ui| {
                    ui.checkbox(&mut rule.enabled, "");
                    let word = if rule.word.trim().is_empty() { "(new word)" } else { rule.word.as_str() };
                    ui.label(RichText::new(word).strong());
                    ui.label(RichText::new(format!("misheard as {}", rule.triggers)).color(MUTED));
                })
                .body(|ui| {
                    if rule_editor(ui, rule, i) {
                        remove = Some(i);
                    }
                });
        }
        if let Some(i) = remove {
            self.draft.dictionary.remove(i);
        }
        if ui.button("➕ Add word").clicked() {
            self.draft.dictionary.push(DictRule::default());
        }
    }

    fn dictionary_test(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.test_input).desired_width(420.0).hint_text("Try a sentence"));
            if ui.button("Test").clicked() {
                let (shared, settings, input, out) = (self.shared.clone(), self.draft.clone(), self.test_input.clone(), self.test_output.clone());
                *out.lock().unwrap() = Some("…".into());
                std::thread::spawn(move || {
                    let result = corrector::correct(&input, &settings.dictionary, &settings.typesafe_key(), &settings.corrector_model, &shared.cache, &shared.http);
                    *out.lock().unwrap() = Some(match result {
                        Ok((text, cands)) => {
                            let details: Vec<String> = cands.iter().map(|c| format!("{} → {} ({:.0}%){}", c.from, c.choice, c.probability * 100.0, if c.to.is_some() { "" } else { ", kept" })).collect();
                            format!("{text}\n{}", if details.is_empty() { "No trigger words found".into() } else { details.join("; ") })
                        }
                        Err(e) => format!("Error: {e}"),
                    });
                    shared.repaint();
                });
            }
        });
        if let Some(out) = self.test_output.lock().unwrap().as_ref() {
            ui.label(RichText::new(out).color(ACCENT));
        }
    }

    fn script_settings(&mut self, ui: &mut Ui) {
        for stage in Stage::ALL {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(stage.label()).small().color(TEXT));
                ui.label(muted(stage.help()).small());
            });
        }
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.draft.script_timeout_secs).range(1..=300).suffix(" s"));
            ui.label("Timeout per script");
        });
        ui.separator();
        let mut remove = None;
        let mut swap = None;
        let count = self.draft.scripts.len();
        for (i, s) in self.draft.scripts.iter_mut().enumerate() {
            egui::Frame::new().fill(theme::INPUT).stroke(Stroke::new(1.0, BORDER)).corner_radius(egui::CornerRadius::same(8)).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.checkbox(&mut s.enabled, "");
                    ui.add(egui::TextEdit::singleline(&mut s.name).desired_width(160.0).hint_text("Name"));
                    egui::ComboBox::from_id_salt(("stage", i)).selected_text(s.stage.label()).width(230.0).show_ui(ui, |ui| {
                        for st in Stage::ALL {
                            ui.selectable_value(&mut s.stage, st, st.label());
                        }
                    });
                    if i > 0 && ui.small_button("▲").clicked() {
                        swap = Some((i, i - 1));
                    }
                    if i + 1 < count && ui.small_button("▼").clicked() {
                        swap = Some((i, i + 1));
                    }
                    if ui.small_button("🗑").clicked() {
                        remove = Some(i);
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut s.command).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text("e.g. python3 ~/bin/fix.py"));
            });
        }
        if let Some((a, b)) = swap {
            self.draft.scripts.swap(a, b);
        }
        if let Some(i) = remove {
            self.draft.scripts.remove(i);
        }
        if ui.button("➕ Add script").clicked() {
            self.draft.scripts.push(Script { enabled: true, name: format!("Script {}", count + 1), command: String::new(), stage: Stage::AfterTranscription });
        }
    }

    fn general_settings(&mut self, ui: &mut Ui) {
        ui.checkbox(&mut self.draft.close_to_tray, "Closing the window hides it to the tray");
        ui.checkbox(&mut self.draft.start_hidden, "Start hidden in the tray");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.draft.history_limit).range(1..=100_000));
            ui.label("History entries to keep");
        });
        ui.separator();
        for (label, path) in [("Settings", config_dir()), ("Data (history, models)", data_dir())] {
            ui.label(RichText::new(format!("{label}: {}", path.display())).color(MUTED).small());
        }
        if ui.button("Quit whisper-hook").clicked() {
            tray::quit(&self.shared);
        }
    }
}

fn toggles(ui: &mut Ui, s: &mut Settings, list: &[Toggle]) {
    for (label, help, field) in list {
        let r = ui.checkbox(field(s), *label);
        if !help.is_empty() {
            r.on_hover_text(*help);
        }
    }
}

fn provider_combo(ui: &mut Ui, provider: &mut Provider) {
    egui::ComboBox::from_id_salt(("provider", ui.id())).selected_text(provider.info().label).width(220.0).show_ui(ui, |ui| {
        for p in Provider::ALL {
            ui.selectable_value(provider, p, p.info().label);
        }
    });
}

fn model_combo(ui: &mut Ui, shared: &Shared, s: &mut Settings, installed: &[String]) {
    let provider = s.provider;
    let fetched = shared.models.lock().unwrap().get(&provider).and_then(|r| r.as_ref().ok().cloned());
    let mut models = fetched.unwrap_or_else(|| provider.info().fallback_models.iter().map(|m| m.to_string()).collect());
    models.extend(installed.iter().filter(|_| provider == Provider::Local).cloned());
    models.push(s.model(provider).to_string());
    models.sort();
    models.dedup();
    let current = s.model_mut(provider);
    let mark = |m: &str| if provider == Provider::Local && installed.iter().any(|i| i == m) { format!("✔ {m}") } else { m.to_string() };
    egui::ComboBox::from_id_salt(("model", ui.id())).selected_text(mark(current)).width(220.0).height(400.0).show_ui(ui, |ui| {
        for m in &models {
            ui.selectable_value(current, m.clone(), mark(m));
        }
    });
}

fn rule_editor(ui: &mut Ui, rule: &mut DictRule, i: usize) -> bool {
    let mut remove = false;
    let field = (ui.available_width() - 110.0).max(200.0);
    egui::Grid::new(("rule-grid", i)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Correct word");
        ui.add(egui::TextEdit::singleline(&mut rule.word).desired_width(200.0).hint_text("e.g. Claude"));
        ui.end_row();
        ui.label("Misheard as");
        ui.add(egui::TextEdit::singleline(&mut rule.triggers).desired_width(360.0).hint_text("comma-separated: cloud, clawed"));
        ui.end_row();
        ui.label("Means");
        ui.add(egui::TextEdit::singleline(&mut rule.meaning).desired_width(field).hint_text("what the correct word refers to"));
        ui.end_row();
        ui.label("Background");
        ui.add(egui::TextEdit::multiline(&mut rule.context).desired_rows(2).desired_width(field).hint_text("context that helps tell the meanings apart"));
        ui.end_row();
        ui.label("Threshold");
        ui.add(egui::Slider::new(&mut rule.threshold, 0.0..=1.0));
        ui.end_row();
    });
    ui.label(RichText::new("Other legitimate meanings of the misheard word (kept when they fit better):").small());
    let mut drop_alt = None;
    for (j, alt) in rule.alternatives.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut alt.word).desired_width(120.0).hint_text("word"));
            ui.add(egui::TextEdit::singleline(&mut alt.meaning).desired_width(360.0).hint_text("meaning"));
            if ui.small_button("✖").clicked() {
                drop_alt = Some(j);
            }
        });
    }
    if let Some(j) = drop_alt {
        rule.alternatives.remove(j);
    }
    ui.horizontal(|ui| {
        if ui.small_button("➕ Add meaning").clicked() {
            rule.alternatives.push(Alternative::default());
        }
        if ui.small_button("🗑 Delete word").clicked() {
            remove = true;
        }
    });
    remove
}

fn capture_combo(ui: &mut Ui) -> Option<String> {
    ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => Some(String::new()),
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let mods = [(modifiers.ctrl, "Ctrl"), (modifiers.alt, "Alt"), (modifiers.shift, "Shift"), (modifiers.mac_cmd, "Super")];
                let mut parts: Vec<&str> = mods.iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
                parts.push(key.name());
                Some(parts.join("+"))
            }
            _ => None,
        })
    })
}


fn nav_item(ui: &mut Ui, selected: bool, text: &str) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click());
    let fill = match (selected, resp.hovered()) {
        (true, _) => ACCENT.gamma_multiply(0.22),
        (false, true) => Color32::from_rgb(28, 32, 44),
        _ => Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(7), fill);
    if selected {
        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 7.0), Vec2::new(3.0, 16.0)), egui::CornerRadius::same(2), ACCENT_HI);
    }
    let color = if selected { Color32::WHITE } else if resp.hovered() { TEXT } else { MUTED };
    ui.painter().text(rect.left_center() + Vec2::new(12.0, 0.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(14.0), color);
    resp
}
