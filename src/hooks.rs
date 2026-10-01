use crate::settings::{Script, Stage};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn shell(command: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.args(["/C", command]).creation_flags(0x0800_0000);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    }
}

fn spawn(script: &Script, env: &[(&str, &str)]) -> std::io::Result<std::process::Child> {
    let mut cmd = shell(&script.command);
    cmd.env("WH_STAGE", format!("{:?}", script.stage)).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.spawn()
}

fn run(script: &Script, input: &str, env: &[(&str, &str)], timeout: Duration) -> Result<String, String> {
    let mut child = spawn(script, env).map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let (mut out, mut err) = (String::new(), String::new());
        let _ = stdout.read_to_string(&mut out);
        let _ = stderr.read_to_string(&mut err);
        (out, err)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("timed out after {}s", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = writer.join();
    let (out, err) = reader.join().unwrap_or_default();
    if status.success() {
        Ok(out)
    } else {
        Err(format!("exited with {status}: {}", err.trim()))
    }
}

fn active(scripts: &[Script], stage: Stage) -> impl Iterator<Item = &Script> {
    scripts.iter().filter(move |s| s.enabled && s.stage == stage && !s.command.trim().is_empty())
}

pub fn has(scripts: &[Script], stage: Stage) -> bool {
    active(scripts, stage).next().is_some()
}

pub fn fire(scripts: &[Script], stage: Stage, env: &[(&str, &str)]) {
    for s in active(scripts, stage) {
        if let Ok(mut child) = spawn(s, env) {
            drop(child.stdin.take());
            std::thread::spawn(move || child.wait());
        }
    }
}

pub fn notify(scripts: &[Script], stage: Stage, input: &str, env: &[(&str, &str)], timeout: Duration) -> Vec<String> {
    active(scripts, stage).filter_map(|s| run(s, input, env, timeout).err().map(|e| format!("Script '{}' {e}", s.name))).collect()
}

pub fn filter(scripts: &[Script], text: String, timeout: Duration) -> (String, Vec<String>) {
    let mut errors = Vec::new();
    let text = active(scripts, Stage::AfterTranscription).fold(text, |text, s| match run(s, &text, &[("WH_TEXT", &text)], timeout) {
        Ok(out) if !out.trim().is_empty() => out.trim_end_matches(['\n', '\r']).to_string(),
        Ok(_) => {
            errors.push(format!("Script '{}' returned no text; kept previous text", s.name));
            text
        }
        Err(e) => {
            errors.push(format!("Script '{}' {e}; kept previous text", s.name));
            text
        }
    });
    (text, errors)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn script(cmd: &str) -> Script {
        Script { enabled: true, name: cmd.into(), command: cmd.into(), stage: Stage::AfterTranscription }
    }

    #[test]
    fn filters_chain_and_failures_pass_through() {
        let scripts = [script("tr a-z A-Z"), script("exit 3"), script("sed 's/HELLO/bye/'")];
        let (out, errors) = filter(&scripts, "hello there".into(), Duration::from_secs(5));
        assert_eq!(out, "bye THERE");
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn timeout_kills() {
        let (out, errors) = filter(&[script("sleep 5")], "x".into(), Duration::from_millis(200));
        assert_eq!(out, "x");
        assert!(errors[0].contains("timed out"));
    }
}
