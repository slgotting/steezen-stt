package com.slgotting.zenassistant;

import android.Manifest;
import android.annotation.SuppressLint;
import android.app.AlertDialog;
import android.content.Intent;
import android.net.Uri;
import android.provider.Settings;
import android.view.MotionEvent;
import android.view.inputmethod.EditorInfo;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;
import com.slgotting.steezen.common.Dictation;
import com.slgotting.steezen.common.R;
import com.slgotting.steezen.common.SettingsActivity;
import java.util.List;
import java.util.Map;
import java.util.Set;
import org.json.JSONObject;

public class MainActivity extends SettingsActivity {
    private static final String ACTIONS = "Off,Assistant,Zen";
    private static final Map<String, String[]> PERMS = Map.of(
            "agent_allow_calls", new String[]{Manifest.permission.CALL_PHONE},
            "agent_allow_sms", new String[]{Manifest.permission.SEND_SMS},
            "agent_allow_contacts", new String[]{Manifest.permission.READ_CONTACTS},
            "agent_allow_calendar", new String[]{Manifest.permission.READ_CALENDAR, Manifest.permission.WRITE_CALENDAR});

    private static final String[][] FIELDS = {
            {"Gestures", "assistant_action", "choice", "Long-press power (assistant)", "Listens and stops after a pause, or tap Send. Needs Zen Assistant set as the default digital assistant.", ACTIONS},
            {"Gestures", "earbud_action", "choice", "Earbud assist button", "Long-press on most Bluetooth earbuds. Stops after a pause, like the power gesture.", ACTIONS},
            {"Gestures", "earbud_check", "earbud", "Does the earbud button reach Zen Assistant?", "", ""},
            {"Gestures", "earbud_media_session", "bool", "Earbud fallback: catch a long press of play/pause", "For earbuds that only send play/pause. Works until another app plays media; short presses still go to your music app (needs notification access).", ""},
            {"Gestures", "gestures_note", "note", "", "Also: the Ask Zen tile in Quick Settings (edit your tiles to add it), the Ask box at the top of this screen, and steezen-stt's volume-down gesture when it is set to Assistant.", ""},
            {"Assistant", "assistant_note", "note", "", "Zen Assistant answers a spoken or typed request by operating the phone with Claude. What leaves the phone: your request and, only while it runs, what the assistant reads (screen elements, notifications, contacts), sent straight to Anthropic with your key. Nothing from the screen is stored. It asks before sending, paying, deleting or changing settings, shows a Stop button the whole time, and never acts on the lock screen.", ""},
            {"Assistant", "anthropic_api_key", "secret", "Anthropic API key", "console.anthropic.com → API keys.", ""},
            {"Assistant", "agent_model", "choice", "Model", "Opus is the most careful; Haiku is the fastest and cheapest.", "@models"},
            {"Assistant", "agent_effort", "choice", "Thinking effort", "Low keeps screen steps quick.", "@efforts"},
            {"Assistant", "agent_max_steps", "int", "Most steps per request", "It stops and tells you when it runs out.", ""},
            {"Assistant", "agent_confirm_all", "bool", "Confirm every action", "Otherwise it asks only before sending, paying, deleting or changing settings.", ""},
            {"Assistant", "agent_speak", "bool", "Read answers aloud when earbuds or headphones are connected", "", ""},
            {"Assistant", "agent_log", "bool", "Keep an action log", "Records what it did (tool names and targets), never screen content or message text.", ""},
            {"Assistant permissions", "agent_allow_screen", "bool", "Read the screen", "Visible text and buttons, read only during a request. Password fields are never read.", ""},
            {"Assistant permissions", "agent_allow_gestures", "bool", "Tap, type, scroll and press Back/Home", "", ""},
            {"Assistant permissions", "agent_allow_screenshots", "bool", "Send screenshots", "Only when the element list isn't enough. Off sends less data.", ""},
            {"Assistant permissions", "agent_allow_apps", "bool", "Open apps, links, web searches, maps and the camera", "", ""},
            {"Assistant permissions", "agent_allow_media", "bool", "Control music, volume and the ringer", "Play, pause, skip, volume levels, vibrate and silent.", ""},
            {"Assistant permissions", "agent_allow_device", "bool", "Flashlight, clipboard and settings pages", "Opens Wi-Fi, Bluetooth, display and other settings pages; changing a setting there still asks you first.", ""},
            {"Assistant permissions", "agent_allow_contacts", "bool", "Look up contacts", "Asks Android for contacts access.", ""},
            {"Assistant permissions", "agent_allow_calls", "bool", "Place calls", "Always asks you first. Asks Android for phone access.", ""},
            {"Assistant permissions", "agent_allow_sms", "bool", "Send text messages", "Always asks you first. Asks Android for SMS access.", ""},
            {"Assistant permissions", "agent_allow_alarms", "bool", "Alarms, timers and reminders", "", ""},
            {"Assistant permissions", "agent_allow_calendar", "bool", "Read and add calendar events", "Asks Android for calendar access.", ""},
            {"Assistant permissions", "agent_allow_notifications", "bool", "Read notifications", "Needs notification access (Setup).", ""},
            {"Assistant permissions", "agent_allow_zen", "bool", "Send notes and long tasks to Zen", "", ""},
            {"Hearing you", "provider", "choice", "Speech-to-text service", "Your phone sends the audio straight to this service with your own key.", "OpenAI,Groq"},
            {"Hearing you", "fallback", "choice", "Fallback service", "Tried when the main service fails.", "None,OpenAI,Groq"},
            {"Hearing you", "openai_api_key", "secret", "OpenAI API key", "", ""},
            {"Hearing you", "groq_api_key", "secret", "Groq API key", "", ""},
            {"Hearing you", "openai_model", "text", "OpenAI model", "e.g. gpt-4o-transcribe, gpt-4o-mini-transcribe, whisper-1", ""},
            {"Hearing you", "groq_model", "text", "Groq model", "e.g. whisper-large-v3-turbo", ""},
            {"Hearing you", "language", "text", "Language", "Blank detects it; or a code such as en.", ""},
            {"Hearing you", "prompt", "text", "Prompt", "Names and words to bias the transcription toward (contacts, apps).", ""},
            {"Hearing you", "corrector_enabled", "bool", "Fix dictionary words with Jev", "Same dictionary as steezen-stt; import it from there.", ""},
            {"Hearing you", "typesafe_api_key", "secret", "Jev (TypeSafe) API key", "", ""},
            {"Zen", "zen_url", "text", "Zen address", "", ""},
            {"Zen", "zen_token", "secret", "Zen inbox token", "Zen → Tools → inbox token.", ""},
            {"Sounds", "sound_start", "bool", "Sound when listening starts", "It always vibrates briefly.", ""},
            {"Sounds", "sound_stop", "bool", "Sound when listening stops", "", ""},
            {"Sounds", "sound_complete", "bool", "Sound when your request is understood", "", ""},
            {"General", "min_recording_ms", "int", "Shortest recording (ms)", "Anything shorter is ignored.", ""},
    };

    private static final String[] TIPS = {
            "Say \"text Sam I'm running late\": it always asks before sending anything.",
            "The Stop button stays at the top of the screen while the assistant works; the notification has one too.",
            "Action log shows everything the assistant did, and Clear log wipes it.",
            "Add the Ask Zen tile to Quick Settings for a one-tap request from anywhere.",
            "Try \"turn on the flashlight\", \"skip this song\" or \"directions home\".",
            "Already set up steezen-stt? Export its settings (with the keys group and a passphrase) and open the file here.",
            "When it asks you a question, tap Answer to reply out loud.",
    };

    private TextView askResult;

    @Override
    protected String[][] fields() {
        return FIELDS;
    }

    @Override
    protected String[] tips() {
        return TIPS;
    }

    @Override
    protected Map<String, String[]> permissions() {
        return PERMS;
    }

    @Override
    protected Object[][] setupSteps() {
        return new Object[][]{
                {"Turn on Zen phone control (accessibility service)", AgentBridge.enabled(this, AgentService.class), "Open settings", (Runnable) () -> restricted("Turn on Zen phone control", "Zen phone control", new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))},
                {"Add your Anthropic API key (Assistant section)", !settings.optString("anthropic_api_key").trim().isEmpty(), "Show", (Runnable) () -> search.setText("Anthropic")},
                {"Add a speech-to-text key (Hearing you section)", !settings.optString("openai_api_key").trim().isEmpty() || !settings.optString("groq_api_key").trim().isEmpty(), "Show", (Runnable) () -> search.setText("API key")},
                {"Long-press power: make Zen Assistant the default digital assistant", assistantSet(), "Open settings", (Runnable) () -> startActivity(new Intent(Settings.ACTION_VOICE_INPUT_SETTINGS))},
                {"Exact reminder times (optional)", !settings.optBoolean("agent_allow_alarms") || PhoneTools.exactAlarms(this), "Allow", (Runnable) () -> startActivity(new Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.parse("package:" + getPackageName())))},
        };
    }

    @Override
    protected void saved() {
        AgentBridge.sync(this);
    }

    private void ask(String text) {
        String problem = AgentBridge.problem(this);
        if (problem != null) {
            askResult.setText(problem);
            askResult.setTextColor(getColor(R.color.red));
            return;
        }
        AgentBridge.send(this, text);
        moveTaskToBack(true);
    }

    @SuppressLint("ClickableViewAccessibility")
    @Override
    protected void top() {
        LinearLayout c = section("Ask", "Ask", true);
        EditText box = new EditText(this);
        box.setHint("Type a request, e.g. set a timer for 10 minutes");
        box.setSingleLine();
        box.setImeOptions(EditorInfo.IME_ACTION_SEND);
        style(box);
        box.setOnEditorActionListener((v, id, e) -> {
            String t = box.getText().toString().trim();
            if (t.isEmpty()) return false;
            box.setText("");
            ask(t);
            return true;
        });
        c.addView(box, matchWrap(px(4)));
        Button hold = button("Hold to ask out loud", true, null);
        hold.setOnTouchListener((v, e) -> {
            if (e.getAction() == MotionEvent.ACTION_DOWN) Dictation.start(this, Dictation.ASSISTANT, false);
            if (e.getAction() == MotionEvent.ACTION_UP || e.getAction() == MotionEvent.ACTION_CANCEL) Dictation.stop(this);
            return false;
        });
        c.addView(hold, matchWrap(px(8)));
        askResult = text("The app steps aside while the assistant works, so it acts on the screen you were using.", 13, R.color.muted);
        askResult.setPadding(0, px(8), 0, 0);
        c.addView(askResult);
    }

    @Override
    public void state(String phase, String detail) {
        if (askResult == null) return;
        askResult.setText(detail);
        askResult.setTextColor(getColor(phase.equals("error") ? R.color.red : phase.equals("done") ? R.color.text : R.color.muted));
        if (phase.equals("done")) moveTaskToBack(true);
    }

    @Override
    protected void extras() {
        List<String> lines = AgentLog.read(this);
        if (!matches("assistant action log history clear privacy")) return;
        LinearLayout c = section("Action log", "Action log (" + lines.size() + ")", false);
        if (lines.isEmpty()) c.addView(text("Nothing yet. Every request and action the assistant takes shows here.", 13, R.color.muted));
        java.text.DateFormat when = java.text.DateFormat.getDateTimeInstance(java.text.DateFormat.SHORT, java.text.DateFormat.SHORT);
        for (int i = lines.size() - 1; i >= Math.max(0, lines.size() - 60); i--) {
            try {
                JSONObject e = new JSONObject(lines.get(i));
                String outcome = e.optString("outcome");
                int color = Set.of("blocked", "declined", "failed", "crashed").contains(outcome) ? R.color.red : e.optString("kind").equals("request") ? R.color.accent_hi : R.color.text;
                TextView t = text(when.format(new java.util.Date(e.optLong("time"))) + "  " + e.optString("kind") + " · " + outcome + "\n" + e.optString("text"), 13, color);
                t.setPadding(0, px(4), 0, px(4));
                c.addView(t);
            } catch (Exception ignored) {
            }
        }
        if (!lines.isEmpty()) c.addView(button("Clear log", false, v -> new AlertDialog.Builder(this, R.style.Dialog).setTitle("Clear the action log?").setMessage("This deletes every entry on this phone.")
                .setPositiveButton("Clear", (d, w) -> {
                    AgentLog.clear(this);
                    rebuild();
                }).setNegativeButton("Cancel", null).show()), matchWrap(px(8)));
    }
}
