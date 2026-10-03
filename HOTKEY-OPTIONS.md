# Hotkey options for push-to-talk and push-to-talk + format

Written 2026-10-03 while adding the "Push to talk + format" hotkey. The format hotkey
defaults to **Alt+Shift+F** so it works now; change it in Settings → Shortcuts.

## Probed on this machine (i3 on X11)

A scratch program registered each combination with the same `global-hotkey` crate the
app uses. "Free" means nothing else has grabbed it at the X server level; it says nothing
about shortcuts inside individual apps (the grab wins there, so the app loses that shortcut).

| Combination | X11 grab | Notes |
|---|---|---|
| Alt+Shift+D | taken | steezen-stt itself (current push-to-talk) |
| Super+D | taken | i3 `dmenu_run` ($mod is Super/Mod4) |
| Alt+Shift+F | free | **default for format.** Next to D, same modifiers. VS Code on Linux formats with Ctrl+Shift+I, so no clash here; on Windows VS Code's Format Document (Shift+Alt+F) would be swallowed. |
| Alt+Shift+S | free | Next to D on the other side. Good alternative if you want both on the home row. |
| Alt+Shift+T / G / M / X / R / E / Q | free | All fine for X; pick by mnemonic. T = "text", M = "markdown". |
| Alt+Shift+Space | free | Easy to hit, but Alt+Space opens the window menu on some desktops (not i3). |
| Alt+Shift+Semicolon / Backquote | free | Unlikely to clash anywhere; harder to remember. |
| Ctrl+Alt+Shift+D / F / T | free | Grab works, but see the Ctrl warning below. |
| Super+Alt+D, Super+Shift+D, Super+Alt+F | free | See the Super warning below. |
| F9, F10, Pause, Scroll Lock | free | Single keys, no modifier leaks at all. Pause/Scroll Lock are ideal if your keyboard has them; F9/F10 clash with debuggers (VS Code F9 = breakpoint) and terminal menus. |

## Why Ctrl (and Super) are a bad idea for *push-to-talk*

You were right that Ctrl causes "saving and other unwanted activities". Pressing the letter
a moment before the last modifier sends the partial combination to the focused window, and
releasing is worse: while you hold the letter, X auto-repeats it as release/press pairs
(that is why the app has a 60 ms auto-repeat grace). Each release ends the grab, and the
grab only matches the *exact* modifier set, so if you let go of one modifier before the
letter, the next repeat can go to the focused window with the remaining modifiers:

- Ctrl+Alt+Shift+**D**, release Alt first → Ctrl+Shift+D reaches the terminal. Most
  terminals send that as Ctrl+D (EOF) → shells and REPLs exit.
- Ctrl+Alt+Shift+**S** → Ctrl+S (save, or XOFF freezing the terminal).
- Ctrl+Alt+Shift+**T** → Ctrl+Shift+T (reopen closed tab / new terminal tab).
- Super+Shift+D or Super+Alt+D, release the other modifier first → Super+D → dmenu pops up.

With Alt+Shift+letter the same leak only types a capital letter (e.g. "F"), which is what
already happens with Alt+Shift+D, and the app's 60 ms auto-repeat grace absorbs most of it.

Toggle-style shortcuts (press once to start, again to stop) do not have this problem,
because the keys are released immediately; Ctrl is fine there.

## Recommendation

1. Keep **Alt+Shift+D** for plain push-to-talk (muscle memory, safe).
2. Use **Alt+Shift+F** (default) or **Alt+Shift+S** for push-to-talk + format. Both sit
   next to D, so it is a one-finger shift.
3. If you want a single key with zero leak risk and the keyboard has one: **Pause** or
   **Scroll Lock**.
4. Avoid Ctrl and Super in any push-to-talk combination.

To recheck on another machine (e.g. Windows), set the shortcut in Settings → Shortcuts:
the app shows "Couldn't register … another program is using it" when a grab fails.
