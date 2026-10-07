#![cfg_attr(windows, windows_subsystem = "windows")]

mod audio;
mod autostart;
mod cli;
mod corrector;
mod engine;
mod formatter;
mod hooks;
mod hotkeys;
mod instance;
mod output;
mod settings;
mod theme;
mod transcribe;
mod tray;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "transcribe") {
        std::process::exit(cli::transcribe(&args[1..]));
    }
    if instance::signal_existing(!autostart::launched_hidden()) {
        return Ok(());
    }
    #[cfg(feature = "local")]
    whisper_rs::install_logging_hooks();
    let settings = settings::Settings::load();
    let shared = engine::Shared::new(settings);
    engine::spawn(shared.clone());
    instance::listen(shared.clone());
    let icon = egui::IconData { rgba: tray::icon_rgba(engine::Phase::Idle), width: 32, height: 32 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("steezen-stt")
            .with_app_id(settings::APP_NAME)
            .with_inner_size([920.0, 680.0])
            .with_min_inner_size([640.0, 480.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native("steezen-stt", options, Box::new(move |cc| Ok(Box::new(ui::App::new(cc, shared)))))
}
