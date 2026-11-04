pub mod bundle;
pub mod corrector;
pub mod formatter;
pub mod http;
pub mod latency;
pub mod settings;
pub mod transcribe;
pub mod wav;

#[cfg(feature = "local")]
pub fn install_logging_hooks() {
    whisper_rs::install_logging_hooks();
}
