use crate::corrector;
use crate::formatter::{self, Description};
use crate::engine::{Cmd, Entry, KeyCheck, Phase, Shared, Step, StepState, fmt_ms, hello_path, recordings_dir};
use crate::latency::{self, Latency};
use crate::hotkeys::{self, Hotkeys};
use crate::settings::{Alternative, DictRule, PasteMethod, Provider, Script, Settings, Stage, config_dir, data_dir};
use crate::transcribe;
use crate::tray::{self, Tray};
use crate::theme::{self, ACCENT, ACCENT_HI, AMBER, BAR, BORDER, GREEN, MUTED, RED, TEXT, bold, card, muted};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Ui, Vec2, ViewportCommand};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PADDING: f32 = 8.0;
const GENERATING: &str = "Generating…";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    History,
    Stats,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Transcription,
    Recording,
    Shortcuts,
    Output,
    Dictionary,
    Formatting,
    Scripts,
    Feedback,
    General,
}

const PAGES: [(Page, &str); 4] = [(Page::Home, "Home"), (Page::History, "History"), (Page::Stats, "Stats"), (Page::Settings, "Settings")];

const SECTIONS: [(Section, &str, &str); 9] = [
    (Section::Transcription, "Transcription", "Where your audio goes and which model turns it into text."),
    (Section::Recording, "Recording", "Microphone and recording behaviour."),
    (Section::Shortcuts, "Shortcuts", "Global keys, e.g. Alt+Shift+D, Ctrl+Space or F9. Click Record and press the combination."),
    (Section::Output, "Output", "What happens to the final text, after the dictionary and every text-filter script have run."),
    (Section::Dictionary, "Dictionary", "Built-in corrector for words speech-to-text often gets wrong. Jev classifies each occurrence in context and the app replaces it only above the threshold. It runs on every transcript, before your scripts. No LLM rewrites your text."),
    (Section::Formatting, "AI formatting", "Turns a rambling dictation into tidy Markdown with a cheap OpenAI model. It runs after the dictionary and before your scripts, only when you ask for it."),
    (Section::Scripts, "Post-processing", "Your own scripts, as many as you like. Order: transcribe → built-in Jev dictionary → AI formatting (when asked) → your text filters, top to bottom → output → after-delivery scripts."),
    (Section::Feedback, "Sounds & notifications", "Audible and desktop feedback. Everything except the local takeover warning is off by default."),
    (Section::General, "General", "Window, tray and history."),
];

type Toggle = (&'static str, &'static str, fn(&mut Settings) -> &mut bool);

const OUTPUT_TOGGLES: [Toggle; 3] = [
    ("Copy transcript to clipboard", "Leave the final text on the clipboard after delivery.", |s| &mut s.output_clipboard),
    ("Paste transcript at cursor", "Paste into the focused window when transcription finishes.", |s| &mut s.output_cursor),
    ("Press Enter after pasting", "Send an Enter keystroke after the text is pasted (e.g. to submit a chat message).", |s| &mut s.output_enter),
];

const FORMAT_TOGGLES: [Toggle; 2] = [
    ("Format when I ask for it out loud", "Say it in your first or last two sentences, e.g. \"…format this as Markdown.\" A request in the middle of a dictation is not noticed.", |s| &mut s.format_voice),
    ("Format every transcript", "Clean up every transcript, with no hotkey or spoken request needed.", |s| &mut s.format_always),
];

const FEEDBACK_TOGGLES: [Toggle; 7] = [
    ("Sound when recording starts", "", |s| &mut s.sound_start),
    ("Sound when recording stops", "", |s| &mut s.sound_stop),
    ("Sound when recording is cancelled", "", |s| &mut s.sound_cancel),
    ("Sound when transcription completes", "", |s| &mut s.sound_complete),
    ("Notification when transcription completes", "", |s| &mut s.notify_complete),
    ("Notification on errors", "", |s| &mut s.notify_error),
    ("Sound and notification when local transcription takes over", "A soft warning when the online service is unusually slow or failing and a local model starts, and a notification once it is back to normal.", |s| &mut s.notify_fallback),
];

type Field = fn(&mut Settings) -> &mut String;

const SHORTCUTS: [(&str, &str, Field); 4] = [
    ("Push to talk", "Hold to record; release to transcribe.", |s| &mut s.push_to_talk),
    ("Push to talk + format", "Hold to record; release to transcribe and have AI format the text as Markdown.", |s| &mut s.format_push_to_talk),
    ("Toggle recording", "Press once to start, again to stop. Optional.", |s| &mut s.toggle_shortcut),
    ("Cancel recording", "Discard the current recording. Optional.", |s| &mut s.cancel_shortcut),
];

pub struct App {
    shared: Arc<Shared>,
    draft: Settings,
    dirty_since: Option<Instant>,
    hide_on_start: bool,
    autostart: crate::autostart::Status,
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
    key_input: String,
    script_tests: Arc<Mutex<std::collections::HashMap<usize, String>>>,
    generating: Arc<Mutex<std::collections::HashMap<usize, (String, Option<Result<Description, String>>)>>>,
    hello_rec: Option<crate::audio::Recording>,
    baseline_edit: Option<(u64, f64)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, shared: Arc<Shared>) -> Self {
        let _ = shared.ctx.set(cc.egui_ctx.clone());
        theme::install(&cc.egui_ctx);
        let mut draft = shared.settings();
        if draft.start_at_login && crate::autostart::disabled_by_os() {
            draft.start_at_login = false;
        }
        let mut hotkeys = Hotkeys::new(shared.clone());
        hotkeys.sync(&draft);
        let tray = Tray::new(shared.clone());
        for p in Provider::ALL {
            shared.refresh_models(p);
        }
        if draft.corrector_enabled && !draft.typesafe_key().is_empty() {
            shared.check_key(draft.typesafe_key(), draft.corrector_model.clone(), false);
        }
        let mut app = Self {
            hide_on_start: draft.start_hidden || crate::autostart::launched_hidden(),
            autostart: Default::default(),
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
            hello_rec: None,
            baseline_edit: None,
            key_input: String::new(),
            script_tests: Arc::default(),
            generating: Arc::default(),
            shared,
        };
        app.apply_autostart();
        app
    }

    fn apply_autostart(&mut self) {
        match crate::autostart::apply(self.draft.start_at_login) {
            Ok(status) => self.autostart = status,
            Err(e) => self.shared.set_notice(format!("Couldn't update start at login: {e}"), true),
        }
    }

    fn persist(&mut self, ctx: &egui::Context) {
        if *self.shared.settings.read().unwrap() != self.draft {
            let login_changed = self.shared.settings.read().unwrap().start_at_login != self.draft.start_at_login;
            *self.shared.settings.write().unwrap() = self.draft.clone();
            if login_changed {
                self.apply_autostart();
            }
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
                ui.label(bold("steezen-stt", 16.0).color(TEXT));
                ui.add_space(14.0);
                for (page, label) in PAGES {
                    if pill(ui, self.page == page, label).clicked() {
                        self.page = page;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_chip(ui, phase);
                    if jev_chip(ui, &self.draft, &self.shared).clicked() {
                        self.page = Page::Settings;
                        self.section = Section::Dictionary;
                    }
                });
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::central_panel(ui.style()).inner_margin(PADDING)).show(ui, |ui| match self.page {
            Page::Home => self.home(ui),
            Page::History => self.history(ui),
            Page::Stats => {
                self.baseline_card(ui);
                stats_page(ui, &self.shared.latency);
            }
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

fn entry_body(ui: &mut Ui, shared: &Arc<Shared>, e: &Entry) {
    ui.horizontal_wrapped(|ui| {
        ui.label(bold(e.time.format("%b %d, %H:%M").to_string(), 13.0).color(TEXT));
        ui.label(muted(format!("{} · {} · {:.1}s", e.provider.info().label, e.model, e.duration_secs)).small());
    });
    if !e.text.is_empty() {
        transcript_text(ui, e);
    }
    if !e.steps.is_empty() {
        step_strip(ui, &e.steps);
    }
    let changed: Vec<String> = e.corrections.iter().filter_map(|c| c.to.as_ref().map(|to| format!("{} → {to} ({:.0}%)", c.from, c.probability * 100.0))).collect();
    if !changed.is_empty() && e.steps.is_empty() {
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
        if let Some(name) = e.audio.as_ref().filter(|n| recordings_dir().join(n).exists()) {
            if ui.small_button("↻ Transcribe again").on_hover_text("Run this recording through the current settings; the result goes to the clipboard").clicked() {
                shared.retry(name);
            }
            if ui.small_button("Show file").on_hover_text(recordings_dir().join(name).display().to_string()).clicked() {
                reveal(&recordings_dir());
            }
        }
    });
}

fn reveal(path: &std::path::Path) {
    let opener = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(path).spawn();
}

impl App {
    fn home(&mut self, ui: &mut Ui) {
        let phase = self.shared.phase();
        let key_failed = matches!(*self.shared.key_check.lock().unwrap(), KeyCheck::Invalid(_));
        if self.draft.corrector_enabled && (self.draft.typesafe_key().is_empty() || key_failed) && self.draft.jev_reminder.due() {
            self.jev_banner(ui);
        }
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

    fn jev_banner(&mut self, ui: &mut Ui) {
        let missed = {
            let m = self.shared.missed.lock().unwrap();
            (m.transcripts, m.words.join("\", \""))
        };
        egui::Frame::new()
            .fill(ACCENT.gamma_multiply(0.12))
            .stroke(Stroke::new(1.0, ACCENT.gamma_multiply(0.6)))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(bold("Turn on smart corrections with Jev", 16.0).color(TEXT));
                ui.label("Speech-to-text often mishears names and jargon: \"Claude\" comes back as \"cloud\", \"SQL\" as \"sequel\". The built-in dictionary uses Jev, TypeSafe's classification model, to read the surrounding sentence and fix only the words that are actually wrong, in a fraction of a second. It never rewrites the rest of your text, and nothing is sent to Jev unless a dictionary word appears.");
                if missed.0 > 0 {
                    ui.label(RichText::new(format!("{} recent transcript{} had words it could have checked (\"{}\").", missed.0, if missed.0 == 1 { "" } else { "s" }, missed.1)).color(ACCENT_HI));
                }
                key_status(ui, &self.shared);
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.key_input).password(true).desired_width(280.0).hint_text("Paste your Jev (TypeSafe) API key"));
                    if ui.add_enabled(!self.key_input.trim().is_empty(), egui::Button::new("Save & turn on")).clicked() {
                        self.draft.typesafe_api_key = self.key_input.trim().to_string();
                        self.key_input.clear();
                        self.shared.check_key(self.draft.typesafe_api_key.clone(), self.draft.corrector_model.clone(), true);
                    }
                    ui.hyperlink_to("Get a key", corrector::KEY_URL);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Don't show again").on_hover_text("You can still add a key in Settings → Dictionary").clicked() {
                            self.draft.jev_reminder.never = true;
                        }
                        let next = 24i64 << self.draft.jev_reminder.snoozes.min(10);
                        if ui.button("Later").on_hover_text(format!("Remind me in {}", describe_hours(next.min(24 * 30)))).clicked() {
                            let wait = self.draft.jev_reminder.snooze();
                            self.shared.set_notice(format!("OK, we'll remind you in {}", describe_hours(wait.num_hours())), false);
                        }
                    });
                });
            });
        ui.add_space(4.0);
    }

    fn baseline_card(&mut self, ui: &mut Ui) {
        let s = self.shared.settings();
        let key = latency::key(s.provider, s.model(s.provider));
        let name = key.replacen(':', " · ", 1);
        let clip = std::fs::read(hello_path()).ok().and_then(|b| crate::audio::read_wav(&b).ok()).map(|(samples, rate)| samples.len() as f32 / rate as f32);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(bold("Baseline check", 15.0).color(TEXT));
            ui.label(muted(format!("Record a short clip saying \"hello\", then send it to {name} several times while the service seems healthy. The average reply time goes into the box below, where you can adjust it before using it; nothing changes until you do. The baseline is the reply time for a clip of almost no audio; the rate per second and the spread are then measured only on the time above it.")).small());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let recording = self.hello_rec.is_some();
                if ui.button(if recording { "⏹ Stop" } else if clip.is_some() { "⏺ Record again" } else { "⏺ Record hello clip" }).clicked() {
                    match self.hello_rec.take() {
                        Some(rec) => {
                            let (samples, _) = rec.finish();
                            if let Err(e) = crate::settings::write_private(&hello_path(), &crate::audio::wav_bytes(&samples, crate::audio::WHISPER_RATE)) {
                                self.shared.set_notice(format!("Couldn't save the hello clip: {e}"), true);
                            }
                        }
                        None => match crate::audio::start(&s.input_device) {
                            Ok(rec) => self.hello_rec = Some(rec),
                            Err(e) => self.shared.set_notice(format!("Couldn't start recording: {e}"), true),
                        },
                    }
                }
                ui.label(muted(match (recording, clip) {
                    (true, _) => "Recording… say \"hello\" and press Stop.".to_string(),
                    (false, Some(secs)) => format!("Clip saved ({secs:.1} s)."),
                    (false, None) => "No clip yet.".to_string(),
                }));
            });
            let mut probe = self.shared.probe.lock().unwrap();
            let online = s.provider != Provider::Local;
            let can_test = online && clip.is_some() && !probe.running && self.hello_rec.is_none();
            let mut start = false;
            ui.horizontal(|ui| {
                ui.add_enabled(!probe.running, egui::DragValue::new(&mut self.draft.probe_runs).range(1..=100).suffix(" requests"));
                ui.label(muted("every"));
                ui.add_enabled(!probe.running, egui::DragValue::new(&mut self.draft.probe_gap_secs).range(0.0..=120.0).speed(0.1).fixed_decimals(1).suffix(" s"));
                if probe.running {
                    ui.label(muted(format!("Testing… {} of {}", probe.runs.len(), probe.total)));
                    if ui.button("Stop").clicked() {
                        probe.stop = true;
                    }
                } else {
                    let test = ui.add_enabled(can_test, egui::Button::new(format!("Test against {name}")));
                    let test = if online { test } else { test.on_disabled_hover_text("Choose an online service in Settings → Transcription first.") };
                    start = test.clicked();
                }
            });
            if !probe.runs.is_empty() {
                egui::ScrollArea::vertical().id_salt("probe-runs").max_height(220.0).show(ui, |ui| egui::Grid::new("probe").num_columns(4).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                    for h in ["Run", "Upload", "Reply", "Heard"] {
                        ui.label(muted(h).small());
                    }
                    ui.end_row();
                    for (i, (result, timing)) in probe.runs.iter().enumerate() {
                        ui.label(format!("{}", i + 1));
                        ui.label(timing.map_or("–".into(), |t| fmt_ms(t.send_ms)));
                        ui.label(timing.map_or("–".into(), |t| fmt_ms(t.wait_ms)));
                        match result {
                            Ok(text) => ui.label(RichText::new(format!("\"{text}\"")).color(TEXT)),
                            Err(e) => ui.label(RichText::new(e).color(RED)),
                        };
                        ui.end_row();
                    }
                }));
            }
            let mut set = None;
            if let (false, Some(sum)) = (probe.running, probe.summary()) {
                ui.label(muted(format!("{} good replies: average {}, spread ± {}, fastest {}, slowest {}.", sum.count, fmt_ms(sum.mean as u64), fmt_ms(sum.sd as u64), fmt_ms(sum.min), fmt_ms(sum.max))));
                if self.baseline_edit.is_none_or(|(id, _)| id != probe.id) {
                    self.baseline_edit = Some((probe.id, sum.mean.round()));
                }
                let target = probe.key.replacen(':', " · ", 1);
                ui.horizontal(|ui| {
                    if let Some((_, ms)) = &mut self.baseline_edit {
                        ui.add(egui::DragValue::new(ms).range(0.0..=60_000.0).speed(5.0).suffix(" ms")).on_hover_text("Starts at the average. Lower it if a few slow replies pulled it up.");
                        if ui.button(format!("Use as the baseline for {target}")).clicked() {
                            set = Some((probe.key.clone(), Some(*ms)));
                        }
                    }
                });
            }
            drop(probe);
            let current = self.shared.latency.baseline(&key);
            ui.horizontal_wrapped(|ui| {
                match current.manual {
                    Some(m) => {
                        ui.label(muted(format!("{name} uses your baseline of {}, set {}.", fmt_ms(m.ms as u64), m.time.format("%b %d, %H:%M"))));
                        if ui.small_button("Go back to the fitted baseline").clicked() {
                            set = Some((key.clone(), None));
                        }
                    }
                    None => {
                        ui.label(muted(format!("{name} uses the baseline fitted from your dictations{}.", current.fit().map_or(String::new(), |(b, _)| format!(" ({})", fmt_ms(b.max(0.0) as u64))))));
                    }
                }
            });
            if let Some((k, ms)) = set {
                self.shared.latency.set_manual(&k, ms);
                if ms.is_some() {
                    *self.shared.probe.lock().unwrap() = Default::default();
                }
            }
            if start {
                self.shared.test_baseline(self.draft.probe_runs, Duration::from_secs_f32(self.draft.probe_gap_secs));
            }
        });
        if self.hello_rec.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        ui.add_space(8.0);
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
                Section::Formatting => self.format_settings(ui),
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
        if let Some(details) = local_model_details(self.draft.model(provider)).filter(|_| provider == Provider::Local) {
            ui.label(muted(details).small());
            ui.label(muted("Quality and speed are relative to the other Whisper models; speed is for CPU, a GPU narrows the gap. English-only (.en) models are a little more accurate in English. Quantized (5-bit, 8-bit) models are smaller and faster with almost no loss. large-v3-turbo is the best balance; base.en suits slow machines.").small());
        }
        if let Some(Err(e)) = self.shared.models.lock().unwrap().get(&provider) {
            ui.label(RichText::new(format!("Couldn't fetch models: {e}")).color(MUTED).small());
        }
        if provider == Provider::Local {
            self.local_settings(ui);
        }
        self.fallback_settings(ui);
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

    fn fallback_settings(&mut self, ui: &mut Ui) {
        ui.separator();
        ui.label(bold("Local fallback", 14.0).color(TEXT));
        ui.label(muted("If the online service fails, these downloaded models are tried from top to bottom at once. If it is unusually slow for the length of the clip (more than one standard deviation over its usual reply time), a local model starts alongside it and whichever finishes first is used; the online request is never cut off. With an empty list the best downloaded model is used.").small());
        latency_summary(ui, &self.shared.latency);
        let list = &mut self.draft.local_fallback;
        let (mut swap, mut remove) = (None, None);
        let count = list.len();
        for (i, m) in list.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{}.", i + 1));
                let missing = !self.installed.contains(m);
                ui.label(if missing { RichText::new(format!("{m} (not downloaded)")).color(AMBER) } else { RichText::new(m).color(TEXT) });
                if let Some(details) = local_model_details(m) {
                    ui.label(muted(details).small());
                }
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
        }
        if let Some((a, b)) = swap {
            list.swap(a, b);
        }
        if let Some(i) = remove {
            list.remove(i);
        }
        let mut available: Vec<&String> = self.installed.iter().filter(|m| !list.contains(m)).collect();
        available.sort_by_key(|m| transcribe::local_model_info(m).map_or(u32::MAX, |i| i.size_mb));
        if available.is_empty() {
            if self.installed.is_empty() {
                ui.label(muted("No local models downloaded yet. Pick Local above, choose a model and download it.").small());
            }
            return;
        }
        let mut added = None;
        egui::ComboBox::from_id_salt("add-fallback").selected_text("➕ Add a fallback model").width(220.0).show_ui(ui, |ui| {
            ui.set_min_width(520.0);
            for m in available {
                if ui.selectable_label(false, model_row(m, m, ui.style())).clicked() {
                    added = Some(m.clone());
                }
            }
        });
        if let Some(m) = added {
            list.push(m);
        }
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
        notice(ui, &self.shared);
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
            ui.label("Jev API key");
            let edit = ui.add(egui::TextEdit::singleline(&mut self.draft.typesafe_api_key).password(!self.show_keys).desired_width(300.0).hint_text("or set $TYPESAFE_API_KEY"));
            if edit.changed() {
                *self.shared.key_check.lock().unwrap() = KeyCheck::Unchecked;
            }
            ui.checkbox(&mut self.show_keys, "Show");
            if ui.button("Test key").clicked() {
                self.shared.check_key(self.draft.typesafe_key(), self.draft.corrector_model.clone(), true);
            }
            ui.hyperlink_to("Get a key", corrector::KEY_URL);
        });
        key_status(ui, &self.shared);
        ui.horizontal(|ui| {
            ui.label("Jev model");
            ui.add(egui::TextEdit::singleline(&mut self.draft.corrector_model).desired_width(160.0));
        });
        self.dictionary_test(ui);
        ui.separator();
        let mut remove = None;
        let key = self.draft.api_key(Provider::OpenAI);
        let mut generating = self.generating.lock().unwrap();
        for (i, rule) in self.draft.dictionary.iter_mut().enumerate() {
            let stale = generating.get(&i).is_some_and(|(word, _)| *word != rule.word);
            if stale || matches!(generating.get(&i), Some((_, Some(Ok(_))))) {
                if let Some((_, Some(Ok(d)))) = generating.remove(&i).filter(|_| !stale) {
                    d.apply(rule);
                }
            }
            let state = generating.get(&i).map(|(_, r)| r.as_ref().map(|r| r.as_ref().err().cloned().unwrap_or_default()));
            let blocked = match &state {
                Some(None) => Some(GENERATING),
                _ if key.is_empty() => Some("Needs an OpenAI API key (Settings → Transcription, or $OPENAI_API_KEY)"),
                _ if rule.word.trim().is_empty() => Some("Type the correct word first"),
                _ => None,
            };
            let id = ui.make_persistent_id(("rule", i));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                .show_header(ui, |ui| {
                    ui.checkbox(&mut rule.enabled, "");
                    let word = if rule.word.trim().is_empty() { "(new word)" } else { rule.word.as_str() };
                    ui.label(RichText::new(word).strong());
                    ui.label(RichText::new(format!("misheard as {}", rule.triggers)).color(MUTED));
                })
                .body(|ui| {
                    let (delete, generate) = rule_editor(ui, rule, i, blocked);
                    if let Some(Some(e)) = &state {
                        ui.label(RichText::new(e).color(RED).small());
                    }
                    if delete {
                        remove = Some(i);
                    }
                    if generate {
                        let (rule, key, model, out, shared) = (rule.clone(), key.clone(), self.draft.format_model.clone(), self.generating.clone(), self.shared.clone());
                        generating.insert(i, (rule.word.clone(), None));
                        std::thread::spawn(move || {
                            let result = formatter::describe(&rule, &key, &model, &shared.http);
                            out.lock().unwrap().insert(i, (rule.word, Some(result)));
                            shared.repaint();
                        });
                    }
                });
        }
        if let Some(i) = remove {
            self.draft.dictionary.remove(i);
            generating.clear();
        }
        drop(generating);
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

    fn format_settings(&mut self, ui: &mut Ui) {
        toggles(ui, &mut self.draft, &FORMAT_TOGGLES);
        ui.label(muted("To ask out loud, say it in your first or last two sentences, e.g. \"…and format this as Markdown.\" Only those sentences are checked, by Jev when you have a Jev key, otherwise by the model below. A request in the middle is not noticed; use the format hotkey instead.").small());
        let format_key = self.draft.format_push_to_talk.trim();
        ui.label(muted(if format_key.is_empty() { "No format hotkey set (Settings → Shortcuts).".into() } else { format!("Or hold {format_key} instead of your usual push-to-talk key.") }).small());
        if self.draft.api_key(Provider::OpenAI).is_empty() {
            ui.label(RichText::new("Formatting needs an OpenAI API key: add one in Settings → Transcription (choose OpenAI to see the field) or set $OPENAI_API_KEY.").color(AMBER));
        }
        ui.horizontal(|ui| {
            ui.label("OpenAI model");
            ui.add(egui::TextEdit::singleline(&mut self.draft.format_model).desired_width(200.0));
        });
        ui.horizontal(|ui| {
            ui.label("Instructions (system prompt)");
            if ui.small_button("Reset").clicked() {
                self.draft.format_prompt = crate::formatter::DEFAULT_PROMPT.into();
            }
        });
        ui.add(egui::TextEdit::multiline(&mut self.draft.format_prompt).desired_width(f32::INFINITY).desired_rows(6));
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
        ui.horizontal(|ui| {
            ui.label("Sample text for Test");
            ui.add(egui::TextEdit::singleline(&mut self.test_input).desired_width(f32::INFINITY));
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
                    if s.stage == Stage::AfterTranscription && ui.small_button("Test").on_hover_text("Run this script on the sample text").clicked() {
                        let (script, input, timeout, out, shared) = (s.clone(), self.test_input.clone(), self.draft.script_timeout_secs.max(1), self.script_tests.clone(), self.shared.clone());
                        out.lock().unwrap().insert(i, "Running…".into());
                        std::thread::spawn(move || {
                            let script = crate::settings::Script { enabled: true, ..script };
                            let (text, runs) = crate::hooks::filter(&[script], input, Duration::from_secs(timeout as u64));
                            out.lock().unwrap().insert(i, runs.into_iter().find_map(|r| r.error).unwrap_or(text));
                            shared.repaint();
                        });
                    }
                    if ui.small_button("🗑").clicked() {
                        remove = Some(i);
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut s.command).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text("e.g. python3 ~/bin/fix.py"));
                if let Some(result) = self.script_tests.lock().unwrap().get(&i) {
                    ui.label(RichText::new(format!("→ {result}")).color(ACCENT_HI).small());
                }
            });
        }
        if let Some((a, b)) = swap {
            self.draft.scripts.swap(a, b);
        }
        if let Some(i) = remove {
            self.draft.scripts.remove(i);
        }
        if swap.is_some() || remove.is_some() {
            self.script_tests.lock().unwrap().clear();
        }
        if ui.button("➕ Add script").clicked() {
            self.draft.scripts.push(Script { enabled: true, name: format!("Script {}", count + 1), command: String::new(), stage: Stage::AfterTranscription });
        }
    }

    fn general_settings(&mut self, ui: &mut Ui) {
        ui.checkbox(&mut self.draft.close_to_tray, "Closing the window hides it to the tray");
        ui.checkbox(&mut self.draft.start_at_login, "Start at login (hidden in the tray)");
        if !self.autostart.text.is_empty() {
            let text = RichText::new(&self.autostart.text).small().color(if self.autostart.warn { AMBER } else { MUTED });
            ui.add(egui::Label::new(text).selectable(true));
        }
        ui.checkbox(&mut self.draft.start_hidden, "Start hidden in the tray");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.draft.history_limit).range(1..=100_000));
            ui.label("History entries to keep");
        });
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.draft.keep_recordings).range(0..=1000));
            ui.label("Recordings to keep, whether or not they transcribed (0 = don't save audio)");
            if ui.small_button("Open folder").clicked() {
                let _ = std::fs::create_dir_all(recordings_dir());
                reveal(&recordings_dir());
            }
        });
        ui.separator();
        for (label, path) in [("Settings", config_dir()), ("Data (history, models, recordings)", data_dir())] {
            ui.label(RichText::new(format!("{label}: {}", path.display())).color(MUTED).small());
        }
        if ui.button("Quit steezen-stt").clicked() {
            tray::quit(&self.shared);
        }
    }
}

const STATS_INTRO: &str = "Only online replies that succeeded are counted. Reply time is the wait after the upload finishes. It is modelled as a fixed overhead (what even a one-word clip costs) plus a rate for each second of audio, fitted to every dictation with newer ones counting more. The spread is measured around that line, so the overhead is not part of it.";

fn stat_rows(b: &latency::Baseline) -> Vec<(&'static str, String, &'static str)> {
    let t = b.totals;
    let avg = |sum: f64| if t.count == 0 { "–".to_string() } else { fmt_ms((sum / t.count as f64) as u64) };
    let fit = b.fit();
    let per = |ms: f64| format!("{ms:.0} ms per audio second");
    vec![
        ("Replies measured", format!("{} (model learned from {})", t.count, b.samples), "Totals start when tracking began; the model also counts replies from before the totals existed."),
        ("Average audio length", if t.count == 0 { "–".into() } else { format!("{:.1} s", t.secs / t.count as f64) }, ""),
        ("Average upload time", avg(t.send_ms), "From starting the request until the last byte of audio is handed to the network."),
        ("Average reply time", avg(t.wait_ms), "From the end of the upload until the transcript arrives."),
        ("Average total time", avg(t.send_ms + t.wait_ms), ""),
        ("Reply time per audio second, overall", if t.secs > 0.0 { per(t.wait_ms / t.secs) } else { "–".into() }, "Total reply time divided by total audio. Short clips inflate this because the fixed overhead is included."),
        ("Fixed overhead (baseline)", fit.map_or("–".into(), |(base, _)| format!("{}{}", fmt_ms(base.max(0.0) as u64), if b.manual.is_some() { " (set by you)" } else { " (fitted)" })), "The reply time for a clip of zero length: your hello-clip baseline if you set one, otherwise where the fitted line starts."),
        ("Reply time per audio second, above the baseline", fit.map_or("–".into(), |(_, rate)| per(rate)), "The slope of the line: what each extra second of audio adds."),
        ("Spread above expected (1 SD)", if b.samples > 1 { fmt_ms(b.sd_ms() as u64) } else { "–".into() }, "How far replies usually land from the line; larger slowdowns are capped so one bad reply can't skew it."),
        ("Local starts after (10 s / 60 s clip)", match (b.threshold_ms(10.0), b.threshold_ms(60.0)) {
            (Some(a), Some(c)) => format!("{} / {}", fmt_ms(a as u64), fmt_ms(c as u64)),
            _ => format!("not yet: {} of {} replies seen", b.samples, latency::MIN_SAMPLES),
        }, "Expected reply time plus one standard deviation, never under 1.5 s."),
    ]
}

fn stats_page(ui: &mut Ui, latency: &Latency) {
    ui.label(bold("Stats", 20.0).color(TEXT));
    ui.label(muted(STATS_INTRO).small());
    ui.add_space(6.0);
    let all = latency.all();
    if all.is_empty() {
        ui.label(muted("Nothing measured yet. Stats appear after your first online transcription."));
        return;
    }
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (key, b) in &all {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(bold(key.replacen(':', " · ", 1), 15.0).color(TEXT));
                egui::Grid::new(("stats", key)).num_columns(2).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
                    for (label, value, help) in stat_rows(b) {
                        let r = ui.label(muted(label));
                        if !help.is_empty() {
                            r.on_hover_text(help);
                        }
                        ui.label(RichText::new(value).color(TEXT));
                        ui.end_row();
                    }
                });
                if b.recent.is_empty() {
                    return;
                }
                ui.add_space(6.0);
                ui.label(bold("Recent replies", 13.0).color(TEXT));
                egui::Grid::new(("recent", key)).num_columns(6).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                    for h in ["When", "Audio", "Upload", "Reply", "Expected", ""] {
                        ui.label(muted(h).small());
                    }
                    ui.end_row();
                    for s in b.recent.iter().rev() {
                        ui.label(s.time.format("%b %d, %H:%M").to_string());
                        ui.label(format!("{:.1} s", s.secs));
                        ui.label(fmt_ms(s.send_ms));
                        ui.label(fmt_ms(s.wait_ms));
                        ui.label(s.expected_ms.map_or("–".into(), |e| fmt_ms(e as u64)));
                        ui.label(if s.slow { RichText::new("slow").color(AMBER) } else { RichText::new("") });
                        ui.end_row();
                    }
                });
            });
            ui.add_space(8.0);
        }
    });
}

fn latency_summary(ui: &mut Ui, latency: &Latency) {
    for (key, b) in latency.all() {
        let name = key.replacen(':', " ", 1);
        let line = match (b.expected_ms(10.0), b.threshold_ms(10.0)) {
            (Some(e), Some(t)) => format!("{name}: replies in about {} for 10 s of audio (± {}); local starts after {}. Learned from {} replies.", fmt_ms(e as u64), fmt_ms(b.sd_ms() as u64), fmt_ms(t as u64), b.samples),
            _ => format!("{name}: still learning its usual speed ({} of {} replies); until then local starts only if it fails.", b.samples, latency::MIN_SAMPLES),
        };
        ui.label(muted(line).small());
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
    let local = provider == Provider::Local;
    let fetched = shared.models.lock().unwrap().get(&provider).and_then(|r| r.as_ref().ok().cloned());
    let mut models = fetched.unwrap_or_else(|| provider.info().fallback_models.iter().map(|m| m.to_string()).collect());
    models.extend(installed.iter().filter(|_| local).cloned());
    models.push(s.model(provider).to_string());
    let is_installed = |m: &str| local && installed.iter().any(|i| i == m);
    let size = |m: &str| transcribe::local_model_info(m).map_or(u32::MAX, |i| i.size_mb);
    models.sort_by_key(|m| (!is_installed(m), size(m), m.clone()));
    models.dedup();
    let current = s.model_mut(provider);
    let mark = |m: &str| if is_installed(m) { format!("✔ {m}") } else { m.to_string() };
    let width = if local { 520.0 } else { 220.0 };
    egui::ComboBox::from_id_salt(("model", ui.id())).selected_text(mark(current)).width(220.0).height(480.0).show_ui(ui, |ui| {
        ui.set_min_width(width);
        let mut header = None;
        for m in &models {
            if local {
                let group = if is_installed(m) { "Downloaded" } else { "Available to download" };
                if header != Some(group) {
                    if header.is_some() {
                        ui.separator();
                    }
                    ui.label(muted(group).small());
                    header = Some(group);
                }
            }
            ui.selectable_value(current, m.clone(), model_row(&mark(m), m, ui.style()));
        }
    });
}

fn model_row(label: &str, name: &str, style: &egui::Style) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let body = egui::TextStyle::Body.resolve(style);
    job.append(label, 0.0, egui::TextFormat::simple(body, TEXT));
    if let Some(details) = local_model_details(name) {
        job.append(&details, 12.0, egui::TextFormat::simple(egui::TextStyle::Small.resolve(style), MUTED));
    }
    job
}

fn local_model_details(name: &str) -> Option<String> {
    let info = transcribe::local_model_info(name)?;
    let dots = |n: u8| (1..=5).map(|i| if i <= n { '●' } else { '○' }).collect::<String>();
    let size = if info.size_mb >= 1000 { format!("{:.1} GB", info.size_mb as f32 / 1000.0) } else { format!("{} MB", info.size_mb) };
    let notes = info.notes.iter().map(|n| format!(" · {n}")).collect::<String>();
    Some(format!("quality {} speed {} · ~{size}{notes}", dots(info.quality), dots(info.speed)))
}

fn rule_editor(ui: &mut Ui, rule: &mut DictRule, i: usize, blocked: Option<&str>) -> (bool, bool) {
    let mut remove = false;
    let mut generate = false;
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
        let button = ui.add_enabled(blocked.is_none(), egui::Button::new(if blocked == Some(GENERATING) { GENERATING } else { "✨ Generate" }).small());
        let button = button.on_hover_text("Fill in the meaning, background and other meanings with OpenAI from the word (and what you typed in Means), and add more words it may be misheard as");
        generate = button.on_disabled_hover_text(blocked.unwrap_or_default()).clicked();
        if ui.small_button("➕ Add meaning").clicked() {
            rule.alternatives.push(Alternative::default());
        }
        if ui.small_button("🗑 Delete word").clicked() {
            remove = true;
        }
    });
    (remove, generate)
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

fn describe_hours(hours: i64) -> String {
    match hours {
        h if h < 48 => format!("{h} hours"),
        h => format!("{} days", h / 24),
    }
}

fn key_status(ui: &mut Ui, shared: &Shared) {
    let (text, color) = match &*shared.key_check.lock().unwrap() {
        KeyCheck::Unchecked => return,
        KeyCheck::Checking => ("Checking key…".to_string(), MUTED),
        KeyCheck::Valid => ("Key works. Smart corrections are on.".to_string(), GREEN),
        KeyCheck::Invalid(e) => (e.clone(), RED),
    };
    ui.label(RichText::new(text).color(color).small());
}

fn transcript_text(ui: &mut Ui, e: &Entry) {
    let font = egui::FontId::proportional(15.0);
    let mut fixes: Vec<(usize, usize, &crate::corrector::Correction)> = e
        .corrections
        .iter()
        .filter_map(|c| Some((c.at?, c.at? + c.to.as_ref()?.len(), c)))
        .filter(|(_, end, _)| *end <= e.text.len())
        .collect();
    if e.dict_text.is_empty() || e.text != e.dict_text || fixes.is_empty() {
        ui.add(egui::Label::new(RichText::new(&e.text).font(font)).selectable(true).wrap());
        return;
    }
    fixes.sort_by_key(|f| f.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut pos = 0;
        for (start, end, c) in fixes {
            if start < pos || !e.text.is_char_boundary(start) || !e.text.is_char_boundary(end) {
                continue;
            }
            ui.add(egui::Label::new(RichText::new(&e.text[pos..start]).font(font.clone())).selectable(true).wrap());
            ui.add(egui::Label::new(RichText::new(&e.text[start..end]).font(font.clone()).color(ACCENT_HI).underline()).sense(Sense::hover()))
                .on_hover_text(format!("Jev fixed \"{}\" → \"{}\" ({:.0}% sure{})", c.from, &e.text[start..end], c.probability * 100.0, if c.cached { ", cached answer" } else { "" }));
            pos = end;
        }
        ui.add(egui::Label::new(RichText::new(&e.text[pos..]).font(font)).selectable(true).wrap());
    });
}

fn step_strip(ui: &mut Ui, steps: &[Step]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for (i, st) in steps.iter().enumerate() {
            if i > 0 {
                ui.label(muted("→").small());
            }
            let color = match st.state {
                StepState::Ok => GREEN,
                StepState::Skipped => MUTED,
                StepState::Failed => RED,
            };
            let jev = st.label.starts_with("Jev");
            let fill = if jev && st.state == StepState::Ok { ACCENT.gamma_multiply(0.25) } else { color.gamma_multiply(0.10) };
            egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius::same(255)).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
                let text = RichText::new(&st.label).small().color(if jev && st.state == StepState::Ok { ACCENT_HI } else { color });
                ui.add(egui::Label::new(text).sense(Sense::hover()))
            }).inner.on_hover_text(&st.detail);
        }
    });
}

fn jev_chip(ui: &mut Ui, s: &Settings, shared: &Shared) -> egui::Response {
    let check = shared.key_check.lock().unwrap().clone();
    let (label, color, tip) = match (s.corrector_enabled, s.typesafe_key().is_empty(), &check) {
        (false, _, _) => ("Jev off", MUTED, "Dictionary corrections are turned off.".to_string()),
        (true, true, _) => ("Jev off", MUTED, "No Jev API key, so misheard words are not corrected. Click to add one.".to_string()),
        (true, false, KeyCheck::Invalid(e)) => ("Jev key rejected", RED, e.clone()),
        (true, false, KeyCheck::Checking | KeyCheck::Unchecked) => ("Jev", MUTED, "Checking the Jev key…".to_string()),
        (true, false, KeyCheck::Valid) => {
            let stats = shared.jev.lock().unwrap();
            let last = stats.last.map_or("not used yet this session".to_string(), |t| format!("last used {} ago", ago(t.elapsed())));
            ("Jev on", ACCENT_HI, format!("Smart corrections are on. This session: {} Jev request{}, {} fix{}; {last}.", stats.requests, if stats.requests == 1 { "" } else { "s" }, stats.fixes, if stats.fixes == 1 { "" } else { "es" }))
        }
    };
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), color);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + Vec2::new(24.0, 10.0), Sense::click());
    let p = ui.painter();
    p.rect(rect, egui::CornerRadius::same(255), color.gamma_multiply(if resp.hovered() { 0.22 } else { 0.12 }), Stroke::new(1.0, color.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    p.galley(rect.center() - galley.size() / 2.0, galley, color);
    resp.on_hover_text(tip)
}

fn ago(d: Duration) -> String {
    match d.as_secs() {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min", s / 60),
        s => format!("{} h", s / 3600),
    }
}
