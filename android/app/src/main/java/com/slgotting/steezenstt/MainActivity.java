package com.slgotting.steezenstt;

import android.annotation.SuppressLint;
import android.content.Intent;
import android.provider.Settings;
import android.view.MotionEvent;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;
import com.slgotting.steezen.common.Dictation;
import com.slgotting.steezen.common.R;
import com.slgotting.steezen.common.SettingsActivity;

public class MainActivity extends SettingsActivity {
    private static final String ACTIONS = "Off,Dictate,Zen,Assistant";
    private static final String HAND_OFF = " Assistant opens the separate Zen Assistant app, which operates the phone for you.";

    private static final String[][] FIELDS = {
            {"Gestures", "volume_action", "choice", "Hold volume-down", "Hold to record, release to finish. A short press still changes the volume. Dictate pastes where you are typing; Zen sends the recording to your Zen inbox." + HAND_OFF, ACTIONS},
            {"Gestures", "assistant_action", "choice", "Long-press power (assistant)", "Starts recording and stops after a pause, or tap Send. Needs steezen-stt set as the default digital assistant; to have the power button run Zen Assistant, make Zen Assistant the default instead.", ACTIONS},
            {"Gestures", "earbud_action", "choice", "Earbud assist button", "Long-press on most Bluetooth earbuds. Stops after a pause, like the power gesture.", ACTIONS},
            {"Gestures", "earbud_check", "earbud", "Does the earbud button reach steezen-stt?", "", ""},
            {"Gestures", "earbud_media_session", "bool", "Earbud fallback: catch a long press of play/pause", "For earbuds that only send play/pause. Works until another app plays media; short presses still go to your music app (needs notification access).", ""},
            {"Services", "provider", "choice", "Main service", "Your phone sends the audio straight to this service with your own key.", "OpenAI,Groq"},
            {"Services", "fallback", "choice", "Fallback service", "Tried when the main service fails.", "None,OpenAI,Groq"},
            {"Services", "openai_api_key", "secret", "OpenAI API key", "", ""},
            {"Services", "groq_api_key", "secret", "Groq API key", "", ""},
            {"Services", "openai_model", "text", "OpenAI model", "e.g. gpt-4o-transcribe, gpt-4o-mini-transcribe, whisper-1", ""},
            {"Services", "groq_model", "text", "Groq model", "e.g. whisper-large-v3-turbo", ""},
            {"Services", "language", "text", "Language", "Blank detects it; or a code such as en.", ""},
            {"Services", "prompt", "text", "Prompt", "Words and names to bias the transcription toward.", ""},
            {"Output", "output_clipboard", "bool", "Copy transcript to clipboard", "", ""},
            {"Output", "output_cursor", "bool", "Paste into the focused text box", "Uses the push-to-talk accessibility service.", ""},
            {"Output", "output_enter", "bool", "Press Enter after pasting", "Sends the message in most chat apps.", ""},
            {"Dictionary", "corrector_enabled", "bool", "Fix dictionary words with Jev", "Same dictionary as the desktop app; import it from there.", ""},
            {"Dictionary", "typesafe_api_key", "secret", "Jev (TypeSafe) API key", "", ""},
            {"Formatting", "format_voice", "bool", "Format when I ask for it out loud", "Say it in your first or last two sentences, e.g. \"…format this as Markdown.\"", ""},
            {"Formatting", "format_always", "bool", "Format every transcript", "", ""},
            {"Zen", "zen_url", "text", "Zen address", "", ""},
            {"Zen", "zen_token", "secret", "Zen inbox token", "Zen → Tools → inbox token.", ""},
            {"Sounds", "sound_start", "bool", "Sound when recording starts", "It always vibrates briefly.", ""},
            {"Sounds", "sound_stop", "bool", "Sound when recording stops", "", ""},
            {"Sounds", "sound_complete", "bool", "Sound when the transcript is ready", "", ""},
            {"Sounds", "notify_complete", "bool", "Show the transcript after pasting", "Errors and copy-only results always show.", ""},
            {"General", "min_recording_ms", "int", "Shortest recording (ms)", "Anything shorter is ignored.", ""},
    };

    private static final String[] TIPS = {
            "Hold volume-down in any app to dictate; a quick press still changes the volume.",
            "Long-press power to talk to Zen: it stops by itself after a pause.",
            "Export your dictionary on the desktop (Settings → Export & import), email it to yourself, and open the attachment here.",
            "Turn on \"Press Enter after pasting\" to send chat messages hands-free.",
            "Search finds any setting and any dictionary word.",
            "Want your phone operated by voice? Install Zen Assistant and set a gesture here to Assistant.",
    };

    private TextView testResult;

    @Override
    protected String[][] fields() {
        return FIELDS;
    }

    @Override
    protected String[] tips() {
        return TIPS;
    }

    private boolean usesAssistant() {
        for (String k : new String[]{"volume_action", "assistant_action", "earbud_action"}) if (Dictation.ASSISTANT.equals(settings.optString(k))) return true;
        return false;
    }

    @Override
    protected Object[][] setupSteps() {
        return new Object[][]{
                {"Volume-down push-to-talk and pasting (accessibility service)", KeyService.enabled(), "Open settings", (Runnable) () -> restricted("Turn on the accessibility service", "steezen-stt push-to-talk", new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))},
                {"Long-press power: default digital assistant", assistantSet(), "Open settings", (Runnable) () -> startActivity(new Intent(Settings.ACTION_VOICE_INPUT_SETTINGS))},
                {"A gesture is set to Assistant: install the Zen Assistant app", !usesAssistant() || App.assistantInstalled(this), "Show", (Runnable) () -> search.setText("Assistant")},
        };
    }

    @Override
    protected void saved() {
        KeyService.syncEarbuds();
    }

    @SuppressLint("ClickableViewAccessibility")
    @Override
    protected void top() {
        LinearLayout c = section("Try it", "Try it", false);
        Button hold = button("Hold to dictate here", true, null);
        hold.setOnTouchListener((v, e) -> {
            if (e.getAction() == MotionEvent.ACTION_DOWN) Dictation.start(this, Dictation.DICTATE, false);
            if (e.getAction() == MotionEvent.ACTION_UP || e.getAction() == MotionEvent.ACTION_CANCEL) Dictation.stop(this);
            return false;
        });
        c.addView(hold);
        testResult = text("The transcript shows here and is copied, without pasting anywhere.", 14, R.color.muted);
        testResult.setTextIsSelectable(true);
        testResult.setPadding(0, px(8), 0, 0);
        c.addView(testResult);
    }

    @Override
    public void state(String phase, String detail) {
        if (testResult == null) return;
        testResult.setText(detail);
        testResult.setTextColor(getColor(phase.equals("error") ? R.color.red : phase.equals("done") ? R.color.text : R.color.muted));
    }
}
