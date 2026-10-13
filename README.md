# steezen-stt

Push-to-talk dictation for Linux (X11) and Windows, modelled on [Whispering](https://github.com/EpicenterHQ/epicenter/tree/main/apps/whispering), with a post-transcription pipeline:

```
hotkey → record → [before scripts] → transcribe (OpenAI / Groq / local whisper.cpp)
       → dictionary (Jev) → [text-filter scripts] → clipboard / paste / Enter → [after scripts]
```

Native Rust (egui), one binary. Uses about 40 MB of private memory when idle and no measurable CPU. A local model adds its own size while loaded and is unloaded after a configurable idle time.

## Build

```sh
cargo build --release                 # CPU (local whisper.cpp included)
cargo build --release --features cuda # NVIDIA GPU (needs the CUDA toolkit)
cargo build --release --features vulkan
cargo build --release --no-default-features  # cloud providers only, no C++ build
```

Linux build deps (Debian/Ubuntu): `cmake libclang-dev libasound2-dev libxdo-dev libgtk-3-dev libayatana-appindicator3-dev libssl-dev`.

### Windows

1. Install Rust from https://rustup.rs (accept the MSVC default).
2. Install the build tools:
   ```powershell
   winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   winget install Kitware.CMake
   winget install LLVM.LLVM
   ```
   Open a new terminal afterwards so the PATH changes apply.
3. Clone and build:
   ```powershell
   git clone git@github.com:slgotting/steezen-stt.git
   cd steezen-stt
   cargo build --release
   ```
   The app is `target\release\steezen-stt.exe`, a single file.

   For an NVIDIA GPU build, also install the CUDA Toolkit, after the Build Tools so it adds its Visual Studio integration:
   ```powershell
   winget install Nvidia.CUDA
   ```
   Open a new terminal (the installer sets `CUDA_PATH`, which the build needs), then `cargo build --release --features cuda`.
4. Make a Start menu shortcut to it. It starts at login by default; turn that off in *Settings → General → Start at login*.

On Windows, settings live in `%APPDATA%\steezen-stt` and scripts run through `cmd /C`, e.g. `python C:\tools\fix.py`.

The toolchain is pinned in `rust-toolchain.toml`. Dependency versions are pinned to releases at least 30 days old; check with `cargo audit`.

## Use

Run `steezen-stt` (symlinked from `~/bin` to `target/release/steezen-stt`, so rebuilds take effect immediately). Running it again while it is open, even when hidden in the tray, brings the existing window to the front instead of starting a second copy.

Hold **Alt+Shift+D**, speak, release. The text is pasted into the focused window.
### Transcribe a file

`steezen-stt transcribe <file.wav> [--prompt <words>]` transcribes a 16-bit PCM WAV with the service, model and keys chosen in Settings and prints the text (errors go to stderr, exit code 1). `--prompt` adds vocabulary hints to the Settings prompt. `--local` skips the online service and uses the downloaded whisper.cpp models (your local model first, then your fallback list, then base, small and large-v3-turbo, whichever are downloaded); Zen's runner uses it when the online transcription fails. The dictionary and formatting are not applied. It runs without opening the window, alongside a running copy. Zen's runner uses it when Zen's transcription backend is set to steezen-stt.

### Start at login

On by default; the app launches hidden in the tray. *Settings → General* shows how it was set up:

- **Windows:** the `HKCU\…\CurrentVersion\Run` registry key, which appears in Task Manager → Startup apps. Disabling it there turns the setting off at the next launch; re-enabling the setting clears Task Manager's disabled flag.
- **Linux desktops** (GNOME, KDE, XFCE, Cinnamon, MATE, LXQt, Budgie, …): `~/.config/autostart/steezen-stt.desktop`. Turning it off in the desktop's startup-apps tool turns the setting off.
- **i3, sway, Hyprland, bspwm**, which ignore that folder: a marked `exec` line is added to the window manager's config, unless the config already runs `dex --autostart`.
- **Other window managers:** the settings page shows the command to add to its startup.

The entry is refreshed on every launch, so it follows the binary if it moves (an AppImage uses its own path). A login launch that finds the app already running exits without showing the window.

Everything is configured in **Settings**; changes save automatically to `~/.config/steezen-stt/settings.json` (mode 600).

| Section | What it does |
|---|---|
| Transcription | Service, API key (or `$OPENAI_API_KEY` / `$GROQ_API_KEY`), model (listed live from the provider), language, prompt, temperature. Local: download any whisper.cpp model, GPU toggle, threads, idle unload. |
| Recording | Microphone, minimum recording length. |
| Shortcuts | Push-to-talk, push-to-talk + format (default **Alt+Shift+F**), toggle, cancel. Click **Record** and press the combination. See `HOTKEY-OPTIONS.md` for which combinations are safe. |
| Output | Copy to clipboard, paste at cursor, press Enter after pasting, paste keystroke (Ctrl+V, Ctrl+Shift+V for terminals, Shift+Insert, or typing). |
| Dictionary | Built-in corrector (the Rust port of `transcript-corrector`), on by default. Misheard words, their correct form, and alternative meanings. Jev classifies each occurrence in context; the app replaces it only above the threshold. Needs a Jev (TypeSafe) API key from https://console.typesafe.ai/keys (or `$TYPESAFE_API_KEY`), with a Test key button and a sentence test box. Without a key, Home shows a reminder you can paste the key into, snooze (1 day, doubling up to 30 days) or dismiss for good. |
| AI formatting | Turns a dictation into tidy Markdown with a cheap OpenAI model (`gpt-4.1-mini` by default, editable system prompt). Runs after the dictionary and before your scripts, only when you hold the format hotkey, turn on **Format every transcript**, or ask out loud in your **first or last two sentences** ("…clean this up and put it in markdown"). Only those sentences are checked, and only when they contain a formatting word (format, markdown, clean, tidy, bullet, organise, …); Jev decides yes/no when you have a Jev key, otherwise the OpenAI model does. A request in the middle of the dictation is not noticed. Needs an OpenAI API key. |
| Post-processing | Any number of your own scripts per stage, run in order, each with a Test button for text filters. |
| Sounds & notifications | All off by default. |
| General | Start at login (on by default, see below), close to tray, start hidden, history size. |

### Post-processing scripts

| Stage | Input | Output |
|---|---|---|
| When recording starts | none | ignored, runs in background |
| Before transcription | `$WH_AUDIO` = 16 kHz WAV path | ignored |
| After transcription (text filter) | transcript on stdin and in `$WH_TEXT` | stdout replaces the transcript; on failure, timeout or empty output the text passes through unchanged |
| After delivery | final text on stdin and in `$WH_TEXT` | ignored |

`$WH_STAGE` names the stage. Commands run with `sh -c` (Linux) or `cmd /C` (Windows). The standalone Python corrector also works as a filter:
`python3 ~/projects/speech/transcript-corrector/transcript_corrector.py`

## Data

- Settings: `~/.config/steezen-stt/settings.json`
- History and models: `~/.local/share/steezen-stt/`
- Jev classification cache: `~/.cache/steezen-stt/corrector-cache.json`

(`%APPDATA%` / `%LOCALAPPDATA%` on Windows.)

## Limits

- Global shortcuts and simulated keystrokes need X11 or Windows; Wayland does not allow either.
- Whispering's voice-activated recording mode, file import and AI recipes/polish are not included.
