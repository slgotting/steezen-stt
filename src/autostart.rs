use crate::settings::APP_NAME;

pub const HIDDEN_ARG: &str = "--hidden";

#[derive(Default)]
pub struct Status {
    pub text: String,
    pub warn: bool,
}

pub fn launched_hidden() -> bool {
    std::env::args().skip(1).any(|a| a == HIDDEN_ARG)
}

fn exe() -> std::io::Result<String> {
    if let Ok(appimage) = std::env::var("APPIMAGE") {
        return Ok(appimage);
    }
    Ok(std::env::current_exe()?.to_string_lossy().into_owned())
}

#[cfg(not(windows))]
pub use linux::*;

#[cfg(not(windows))]
mod linux {
    use super::*;
    use std::path::PathBuf;

    const MARKER: &str = "# steezen-stt: start at login (Settings → General)";
    const XDG_DESKTOPS: &[&str] = &[
        "gnome", "kde", "plasma", "xfce", "cinnamon", "mate", "lxqt", "lxde", "budgie", "unity", "pantheon", "deepin", "cosmic", "enlightenment", "ukui", "openbox",
    ];

    struct Wm {
        name: &'static str,
        configs: &'static [&'static str],
        line: &'static str,
    }

    const WMS: &[Wm] = &[
        Wm { name: "i3", configs: &[".config/i3/config", ".i3/config"], line: "exec --no-startup-id {cmd}" },
        Wm { name: "sway", configs: &[".config/sway/config"], line: "exec {cmd}" },
        Wm { name: "hyprland", configs: &[".config/hypr/hyprland.conf"], line: "exec-once = {cmd}" },
        Wm { name: "bspwm", configs: &[".config/bspwm/bspwmrc"], line: "{cmd} &" },
    ];

    fn desktop_file() -> PathBuf {
        dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("autostart").join(format!("{APP_NAME}.desktop"))
    }

    fn session() -> String {
        ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "DESKTOP_SESSION"]
            .iter()
            .filter_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
            .collect::<Vec<_>>()
            .join(":")
            .to_lowercase()
    }

    fn config_path(wm: &Wm) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        wm.configs.iter().map(|c| home.join(c)).find(|p| p.is_file())
    }

    fn runs_xdg_autostart(config: &str) -> bool {
        config.lines().any(|l| !l.trim_start().starts_with('#') && ["dex", "xdg-autostart", "--autostart"].iter().any(|k| l.contains(k)))
    }

    fn set_config_line(path: &PathBuf, line: Option<&str>) -> std::io::Result<()> {
        let text = std::fs::read_to_string(path)?;
        if line.is_none() && !text.contains(MARKER) {
            return Ok(());
        }
        let mut out = Vec::new();
        let mut lines = text.lines();
        while let Some(l) = lines.next() {
            if l == MARKER {
                lines.next();
            } else {
                out.push(l);
            }
        }
        out.extend(line.map(|l| [MARKER, l]).into_iter().flatten());
        let new = out.join("\n") + "\n";
        if new != text { std::fs::write(path, new) } else { Ok(()) }
    }

    pub fn disabled_by_os() -> bool {
        std::fs::read_to_string(desktop_file()).is_ok_and(|s| s.lines().any(|l| matches!(l.trim(), "Hidden=true" | "X-GNOME-Autostart-enabled=false")))
    }

    pub fn apply(enabled: bool) -> std::io::Result<Status> {
        let session = session();
        let current = WMS.iter().find(|wm| session.split(':').any(|s| s == wm.name));
        let cmd = format!("\"{}\" {HIDDEN_ARG}", exe()?.replace('\\', "\\\\").replace('"', "\\\""));
        let mut status = Status { text: "Starts from the autostart folder (~/.config/autostart).".into(), warn: false };
        for wm in WMS {
            let Some(path) = config_path(wm) else { continue };
            let own = enabled && current.is_some_and(|c| c.name == wm.name) && !runs_xdg_autostart(&std::fs::read_to_string(&path)?);
            let line = own.then(|| wm.line.replace("{cmd}", &cmd));
            set_config_line(&path, line.as_deref())?;
            if own {
                status.text = format!("Your session ({}) ignores the autostart folder, so it starts from {}.", wm.name, path.display());
            }
        }
        let file = desktop_file();
        if !enabled {
            return match std::fs::remove_file(&file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(Status::default()),
            };
        }
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName={APP_NAME}\nComment=Push-to-talk dictation\nExec={cmd}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        );
        if std::fs::read_to_string(&file).ok().as_deref() != Some(entry.as_str()) {
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(&file, entry)?;
        }
        let known = current.is_some() || XDG_DESKTOPS.iter().any(|d| session.contains(d));
        if !known {
            let name = if session.is_empty() { "unknown" } else { session.as_str() };
            status = Status {
                text: format!("Your session ({name}) may not run the autostart folder. If the app doesn't start at login, add this to your window manager's startup:\n{cmd}"),
                warn: true,
            };
        }
        Ok(status)
    }
}

#[cfg(windows)]
pub use windows::*;

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::process::CommandExt;

    const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    fn reg(args: &[&str]) -> std::io::Result<std::process::Output> {
        std::process::Command::new("reg").args(args).creation_flags(0x0800_0000).output()
    }

    pub fn disabled_by_os() -> bool {
        reg(&["query", APPROVED, "/v", APP_NAME]).is_ok_and(|o| {
            let out = String::from_utf8_lossy(&o.stdout);
            out.split_whitespace().skip_while(|t| *t != "REG_BINARY").nth(1).and_then(|d| u8::from_str_radix(d.get(..2)?, 16).ok()).is_some_and(|b| b & 1 == 1)
        })
    }

    pub fn apply(enabled: bool) -> std::io::Result<Status> {
        if !enabled {
            reg(&["delete", RUN, "/v", APP_NAME, "/f"])?;
            return Ok(Status::default());
        }
        reg(&["delete", APPROVED, "/v", APP_NAME, "/f"])?;
        let out = reg(&["add", RUN, "/v", APP_NAME, "/t", "REG_SZ", "/d", &format!("\"{}\" {HIDDEN_ARG}", exe()?), "/f"])?;
        if !out.status.success() {
            return Err(std::io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string()));
        }
        Ok(Status { text: "Starts from the Run registry key (listed in Task Manager → Startup apps).".into(), warn: false })
    }
}
