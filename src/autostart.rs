use crate::settings::APP_NAME;

pub const HIDDEN_ARG: &str = "--hidden";

pub fn launched_hidden() -> bool {
    std::env::args().skip(1).any(|a| a == HIDDEN_ARG)
}

#[cfg(not(windows))]
pub fn apply(enabled: bool) -> std::io::Result<()> {
    let path = dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("autostart").join(format!("{APP_NAME}.desktop"));
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe()?;
    let exe = exe.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={APP_NAME}\nComment=Push-to-talk dictation\nExec=\"{exe}\" {HIDDEN_ARG}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    );
    if std::fs::read_to_string(&path).is_ok_and(|s| s == entry) {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, entry)
}

#[cfg(windows)]
pub fn apply(enabled: bool) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let mut cmd = std::process::Command::new("reg");
    if enabled {
        let value = format!("\"{}\" {HIDDEN_ARG}", std::env::current_exe()?.display());
        cmd.args(["add", KEY, "/v", APP_NAME, "/t", "REG_SZ", "/d", &value, "/f"]);
    } else {
        cmd.args(["delete", KEY, "/v", APP_NAME, "/f"]);
    }
    let out = cmd.creation_flags(0x0800_0000).output()?;
    if enabled && !out.status.success() {
        return Err(std::io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(())
}
