# steezen-stt

Push-to-talk dictation for Linux (X11) and Windows, modelled on [Whispering](https://github.com/EpicenterHQ/epicenter/tree/main/apps/whispering), with a post-transcription pipeline:

```
hotkey → record → [before scripts] → transcribe (OpenAI / Groq / local whisper.cpp)
       → dictionary (Jev) → [text-filter scripts] → clipboard / paste / Enter → [after scripts]
```

Native Rust (egui), one binary. Uses about 40 MB of private memory when idle and no measurable CPU. A local model adds its own size while loaded and is unloaded after a configurable idle time.

## Layout

A Cargo workspace: `core/` is the shared library (settings, cloud and local transcription, Jev dictionary, formatting, latency, settings export/import), the root crate is the desktop app, and `android/` holds the two phone apps (steezen-stt and Zen Assistant, see Android below) with their JNI bridge in `android/rust/`.

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
### Slow or failing service

Settings → Transcription has a main service and a fallback service (None, another online service, or Local, the default). Every recording goes to the main service first. The app learns how long that service and model usually take to reply for a given length of audio (upload time is measured separately and not counted), and keeps the averages in `latency.json` in the data folder so they survive restarts. Once it has seen 5 replies, a reply that takes longer than the expected time plus one standard deviation (never less than 1.5 s) starts the fallback alongside it, with a soft sound and a notification. The online request is never cancelled: whichever finishes first with a good result is used, the other is discarded, and only one result reaches the clipboard, the cursor and the history. If the main service fails, the fallback starts at once. With no fallback (no downloaded model for Local, or no key for an online one) the app simply waits. Each history entry shows the upload and reply times, when local started and why, and a late reply that lost. Turn the sound and notification off in Settings → Sounds & notifications. The Stats page shows, for each service and model, the average audio length, upload, reply and total times, the fixed overhead and the reply time per audio second above it, the spread, when local would start, and the last 20 replies. Its Baseline check records a short "hello" clip and sends it to the current service as many times as you choose, with a gap you choose between requests (10 every 2 s by default; Stop ends it early). The average of the good replies, with the spread, fastest and slowest, goes into an editable box, so you can lower it if a few slow replies pulled it up, and one click sets that value as the baseline (the reply time for almost no audio), and the rate per audio second and the spread are then fitted only on the time above it. Nothing changes until you click, and you can go back to the fitted baseline at any time.

### Transcribe a file

`steezen-stt transcribe <file.wav> [--prompt <words>]` transcribes a 16-bit PCM WAV with the service, model and keys chosen in Settings and prints the text (errors go to stderr, exit code 1). `--prompt` adds vocabulary hints to the Settings prompt. Without `--local` it starts with the main service chosen in Settings and races the chosen fallback when needed, as described under Slow or failing service below. It prints `transcribed by: <provider> (<model>)` to stderr, with the upload and reply times for an online service. `--local` skips the online services and uses the downloaded whisper.cpp models (your local model first, then your fallback list, then base, small and large-v3-turbo, then any other downloaded model, so one broken model never stops it); Zen's runner uses it when the online transcription fails. The dictionary and formatting are not applied. It runs without opening the window, alongside a running copy. Zen's runner uses it when Zen's transcription backend is set to steezen-stt.

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

### Export & import settings

Settings → Export & import writes the groups you tick (dictionary, services & models, output, formatting, sounds, phone, phone assistant, API keys, desktop) to one `.steezen.json` file, for example to email to your phone. API keys and the Zen token are only exported with a passphrase, which locks the whole file (Argon2id + XChaCha20-Poly1305). Import a file by dropping it on the window or typing its path, pick which groups to apply, and either merge the dictionary (new words added, matching words updated) or replace it. Both phone apps read and write the same file.

## Android

`android/` holds two separate phone apps built from one Gradle project:

| Module | App | Package | What it is |
|---|---|---|---|
| `android/app` | **steezen-stt** | `com.slgotting.steezenstt` | Dictation: volume-down push-to-talk, paste into any text box, Zen capture. |
| `android/assistant` | **Zen Assistant** | `com.slgotting.zenassistant` | A digital assistant that operates the phone for you by voice or text. |
| `android/common` | (library) | | Recording, the JNI bridge to the Rust core, the power-button/earbud assist gesture, and the settings screen both apps use. |

Both run the same Rust core (`core/` plus `android/rust`, one `libsteezen_mobile.so`), so audio goes straight from the phone to OpenAI or Groq with your own key.

### Why two apps in one repo, not a fork

Zen Assistant is a separate app: it has its own install, icon, settings, permissions and processes, and you can uninstall either one without touching the other. steezen-stt no longer asks for calls, SMS, contacts, calendar or alarms, and no longer contains phone control at all.

They stay in this repository and share code rather than being a fork, because about 80% of what the assistant needs is the speech pipeline: transcription with a fallback service, the Jev dictionary, Zen upload, the settings file and Export & import, the assist gesture, earbud handling and the settings UI. A fork would copy all of that, and every fix (such as the whisper error -6 fix) would have to be made twice and would drift. A Gradle library module gives each app only the shared pieces it uses, so the separation is real at install time and the code is still written once. (The one-repository-per-project rule points the same way.)

### steezen-stt

- **Hold volume-down** in any app: records while held, then pastes the transcript into the focused text box (and presses Enter if that is on). A short press still changes the volume. Needs the *steezen-stt push-to-talk* accessibility service.
- **Long-press power** (with steezen-stt as the default digital assistant): records until you pause or tap Send.
- **Earbud assist button** (long-press on most Bluetooth earbuds): Android sends it as a voice-command intent, which steezen-stt handles. Settings → Gestures shows whether this phone routes it to steezen-stt or to another app. For earbuds that only send play/pause, an optional fallback keeps a media session that turns a long press into the gesture and passes short presses on to your music app (needs notification access); it works until another app plays media.
- Each gesture can be set to **Dictate**, **Zen** (uploads the recording to your Zen inbox at `/api/capture` with your inbox token, and Zen transcribes and routes it), **Assistant** (hands off to Zen Assistant, which listens and acts) or **Off**.
- Settings has a search box, a hold-to-dictate test button, a dictionary editor, and the same Export & import as the desktop: Share… sends the file by email or Drive, and opening a `.steezen.json` attachment imports it.

### Zen Assistant

Zen Assistant takes a spoken or typed request and has Claude operate the phone. It calls the Anthropic API directly from the phone with your own key (Claude Opus 5.5 at low effort by default, with server-side refusal fallback on), so screen steps don't go through a server. Notes, ideas and long tasks go to Zen with `send_to_zen`.

- **Ways to ask:** long-press power (with Zen Assistant as the default digital assistant), the earbud assist button, the *Ask Zen* Quick Settings tile, the Ask box and hold-to-ask button at the top of its screen, or steezen-stt's volume-down gesture set to Assistant. Each gesture can be Assistant, Zen or Off; power and earbud start as Assistant.
- **Its own process.** The agent is an accessibility service, *Zen phone control*, in a separate `:agent` process, with the listening pill, Stop button and confirmation sheet drawn there too. The agent loop runs under a crash guard and pauses itself after 3 errors in 10 minutes.
- **What it can do:** read the screen (numbered elements), tap, long-press, type, scroll, swipe, Back/Home/Recents/notifications/Quick Settings; open apps and links, search the web, start directions, open the camera; look up contacts, call, text; set alarms, timers and reminders; read and add calendar events; read notifications; play/pause/skip whatever is playing, set any volume, switch the ringer to normal/vibrate/silent; the flashlight; copy to the clipboard; open any settings page (Wi-Fi, Bluetooth, display, Do Not Disturb and more) and change it on screen; take screenshots (off by default); send to Zen. Each group is a toggle under *Assistant permissions*, and an Android permission is only asked for when you turn on the toggle that needs it.
- **What Android doesn't allow:** apps can't switch Wi-Fi, Bluetooth, mobile data or brightness directly any more (Android 10+), so it opens the settings page or panel and taps the switch, and that tap goes through the confirmation sheet like any settings change. Silent mode needs Do Not Disturb access, which it opens for you the first time.
- **Confirmation sheet** before calls, texts, anything that sends or shares, pays or buys, deletes or removes, any tap in Settings or a payment or banking app, and before everything when *Confirm every action* is on. The phone decides this from what is being tapped, not the model. If you decline, the action is not retried another way.
- **Safety:** it never acts while the phone is locked and stops when the screen turns off. A Stop button stays at the top of the screen and in the notification while it works. It can't read or type password fields, can't act on Zen Assistant or steezen-stt themselves, and only opens https, geo, mailto and market links. Screen text, notifications and messages are passed to Claude as untrusted data, with instructions never to follow text found there. Card numbers are hidden before anything is sent.
- **Privacy:** only your request and what the current request reads leave the phone: the audio to your speech-to-text service, everything else only to Anthropic. Nothing from the screen is stored. The action log keeps request text, tool names and targets (message text only as a length). It can be turned off and cleared under *Action log*.
- Answers show in a card, and are read aloud when earbuds or headphones are connected. If it asks a question, tap Answer within 2 minutes to continue the same conversation.
- Setup lists what is still missing: Zen phone control, the Anthropic key, a speech-to-text key, the default-assistant role, notification access, the battery-optimisation exemption, and exact alarms for reminders. Each step explains Android 13+ "restricted setting" (App info → ⋮ → Allow restricted settings), which sideloaded apps hit for accessibility and notification access.
- Its settings are its own file. To reuse steezen-stt's keys, export them there (keys group, with a passphrase) and open the file in Zen Assistant. An empty key in an imported file never wipes a key already set.

Only one app can be the default digital assistant. Make Zen Assistant the default if the power button should run the assistant, or steezen-stt if it should dictate.

Build and install (needs the Android SDK and NDK 29, `cargo install cargo-ndk`, and the Android Rust targets):

```sh
scripts/android.sh                    # build both APKs
scripts/android.sh install            # also adb install both on the connected phone
scripts/android.sh install assistant  # build both, install only Zen Assistant (or: app)
```

The APKs are signed with the local debug key, for sideloading. steezen-stt's version follows the root `Cargo.toml`; Zen Assistant's is in `android/assistant/VERSION`.

## Data

- Settings: `~/.config/steezen-stt/settings.json`
- History and models: `~/.local/share/steezen-stt/`
- Jev classification cache: `~/.cache/steezen-stt/corrector-cache.json`

(`%APPDATA%` / `%LOCALAPPDATA%` on Windows.)

## Limits

- Global shortcuts and simulated keystrokes need X11 or Windows; Wayland does not allow either.
- Whispering's voice-activated recording mode, file import and AI recipes/polish are not included.
