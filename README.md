# whisper-hook

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
Windows: MSVC toolchain, CMake and LLVM (for libclang).

The toolchain is pinned in `rust-toolchain.toml`. Dependency versions are pinned to releases at least 30 days old; check with `cargo audit`.

## Use

Run `whisper-hook` (symlinked from `~/bin` to `target/release/whisper-hook`, so rebuilds take effect immediately). Running it again while it is open, even when hidden in the tray, brings the existing window to the front instead of starting a second copy.

Hold **Alt+Shift+D**, speak, release. The text is pasted into the focused window.
Everything is configured in **Settings**; changes save automatically to `~/.config/whisper-hook/settings.json` (mode 600).

| Section | What it does |
|---|---|
| Transcription | Service, API key (or `$OPENAI_API_KEY` / `$GROQ_API_KEY`), model (listed live from the provider), language, prompt, temperature. Local: download any whisper.cpp model, GPU toggle, threads, idle unload. |
| Recording | Microphone, minimum recording length. |
| Shortcuts | Push-to-talk, toggle, cancel. Click **Record** and press the combination. |
| Output | Copy to clipboard, paste at cursor, press Enter after pasting, paste keystroke (Ctrl+V, Ctrl+Shift+V for terminals, Shift+Insert, or typing). |
| Dictionary | Misheard words, their correct form, and alternative meanings. Jev classifies each occurrence in context; the app replaces it only above the threshold. Needs a TypeSafe key (or `$TYPESAFE_API_KEY`). Includes a test box. |
| Scripts | Shell commands per stage, run in order. |
| Sounds & notifications | All off by default. |
| General | Close to tray, start hidden, history size. |

### Scripts

| Stage | Input | Output |
|---|---|---|
| When recording starts | none | ignored, runs in background |
| Before transcription | `$WH_AUDIO` = 16 kHz WAV path | ignored |
| After transcription (text filter) | transcript on stdin and in `$WH_TEXT` | stdout replaces the transcript; on failure, timeout or empty output the text passes through unchanged |
| After delivery | final text on stdin and in `$WH_TEXT` | ignored |

`$WH_STAGE` names the stage. Commands run with `sh -c` (Linux) or `cmd /C` (Windows). The standalone Python corrector also works as a filter:
`python3 ~/projects/speech/transcript-corrector/transcript_corrector.py`

## Data

- Settings: `~/.config/whisper-hook/settings.json`
- History and models: `~/.local/share/whisper-hook/`
- Jev classification cache: `~/.cache/whisper-hook/corrector-cache.json`

(`%APPDATA%` / `%LOCALAPPDATA%` on Windows.)

## Limits

- Global shortcuts and simulated keystrokes need X11 or Windows; Wayland does not allow either.
- Whispering's voice-activated recording mode, file import and AI recipes/polish are not included.
