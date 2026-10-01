use crate::engine::{Cmd, Phase, Shared};
use std::sync::Arc;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

const SIZE: u32 = 32;

fn color(phase: Phase) -> [u8; 3] {
    match phase {
        Phase::Idle => [140, 140, 150],
        Phase::Recording => [230, 57, 70],
        Phase::Transcribing => [245, 166, 35],
    }
}

fn tooltip(phase: Phase) -> &'static str {
    match phase {
        Phase::Idle => "whisper-hook: idle",
        Phase::Recording => "whisper-hook: recording",
        Phase::Transcribing => "whisper-hook: transcribing",
    }
}

pub fn icon_rgba(phase: Phase) -> Vec<u8> {
    let [r, g, b] = color(phase);
    let c = (SIZE as f32 - 1.0) / 2.0;
    (0..SIZE * SIZE)
        .flat_map(|i| {
            let (x, y) = ((i % SIZE) as f32 - c, (i / SIZE) as f32 - c);
            let d = (x * x + y * y).sqrt();
            let mic = x.abs() < 4.0 && (-9.0..5.0).contains(&y) || (y - 5.0).abs() < 1.5 && x.abs() < 7.0 || x.abs() < 1.2 && (5.0..11.0).contains(&y);
            let alpha = ((c + 0.5 - d).clamp(0.0, 1.0) * 255.0) as u8;
            if mic && alpha > 0 { [255, 255, 255, alpha] } else { [r, g, b, alpha] }
        })
        .collect()
}

fn icon(phase: Phase) -> Option<Icon> {
    Icon::from_rgba(icon_rgba(phase), SIZE, SIZE).ok()
}

const ITEMS: [(&str, &str); 3] = [("show", "Show window"), ("toggle", "Start / stop recording"), ("quit", "Quit")];

fn build() -> Option<TrayIcon> {
    let menu = Menu::new();
    for (id, label) in ITEMS {
        let _ = menu.append(&MenuItem::with_id(id, label, true, None));
    }
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon(Phase::Idle)?)
        .with_tooltip(tooltip(Phase::Idle))
        .build()
        .map_err(|e| eprintln!("tray unavailable: {e}"))
        .ok()
}

fn apply(tray: &TrayIcon, phase: Phase) {
    let _ = tray.set_icon(icon(phase));
    let _ = tray.set_tooltip(Some(tooltip(phase)));
}

pub fn show_window(shared: &Shared) {
    if let Some(ctx) = shared.ctx.get() {
        use eframe::egui::ViewportCommand;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        ctx.request_repaint();
    }
}

pub fn quit(shared: &Shared) {
    QUITTING.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Some(ctx) = shared.ctx.get() {
        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
        ctx.request_repaint();
    }
}

pub static QUITTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn install_handlers(shared: Arc<Shared>) {
    let menu_shared = shared.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| match e.id.as_ref() {
        "show" => show_window(&menu_shared),
        "toggle" => menu_shared.send(Cmd::Toggle),
        "quit" => quit(&menu_shared),
        _ => {}
    }));
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick { .. } = e {
            show_window(&shared);
        }
    }));
}

pub struct Tray {
    #[cfg(not(target_os = "linux"))]
    icon: Option<TrayIcon>,
    phase: std::cell::Cell<Phase>,
}

#[cfg(target_os = "linux")]
thread_local! {
    static LINUX_TRAY: std::cell::RefCell<Option<TrayIcon>> = const { std::cell::RefCell::new(None) };
}

impl Tray {
    pub fn new(shared: Arc<Shared>) -> Self {
        install_handlers(shared);
        #[cfg(target_os = "linux")]
        {
            std::thread::spawn(|| {
                if gtk::init().is_err() {
                    eprintln!("tray unavailable: gtk init failed");
                    return;
                }
                LINUX_TRAY.with(|t| *t.borrow_mut() = build());
                gtk::main();
            });
            Self { phase: std::cell::Cell::new(Phase::Idle) }
        }
        #[cfg(not(target_os = "linux"))]
        Self { icon: build(), phase: std::cell::Cell::new(Phase::Idle) }
    }

    pub fn update(&self, phase: Phase) {
        if self.phase.replace(phase) == phase {
            return;
        }
        #[cfg(target_os = "linux")]
        gtk::glib::idle_add_once(move || LINUX_TRAY.with(|t| {
            if let Some(t) = t.borrow().as_ref() {
                apply(t, phase);
            }
        }));
        #[cfg(not(target_os = "linux"))]
        if let Some(t) = &self.icon {
            apply(t, phase);
        }
    }
}
