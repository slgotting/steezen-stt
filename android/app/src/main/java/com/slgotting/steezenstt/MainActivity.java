package com.slgotting.steezenstt;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.app.role.RoleManager;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.app.AlarmManager;
import android.net.Uri;
import android.os.PowerManager;
import android.os.Bundle;
import android.provider.Settings;
import android.text.Editable;
import android.text.InputType;
import android.text.TextWatcher;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;
import android.widget.Toast;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import org.json.JSONArray;
import org.json.JSONObject;

public class MainActivity extends Activity implements Dictation.Listener {
    private static final int SAVE_FILE = 1;
    private static final int OPEN_FILE = 2;
    private static final String ACTIONS = "Off,Dictate,Zen,Assistant";
    private static final Map<String, String[]> PERMS = Map.of(
            "agent_allow_calls", new String[]{Manifest.permission.CALL_PHONE},
            "agent_allow_sms", new String[]{Manifest.permission.SEND_SMS},
            "agent_allow_contacts", new String[]{Manifest.permission.READ_CONTACTS},
            "agent_allow_calendar", new String[]{Manifest.permission.READ_CALENDAR, Manifest.permission.WRITE_CALENDAR});
    private static final String[] VOICE_INTENTS = {"android.intent.action.VOICE_COMMAND", "android.speech.action.VOICE_SEARCH_HANDS_FREE"};

    private static final String[][] FIELDS = {
            {"Gestures", "volume_action", "choice", "Hold volume-down", "Hold to record, release to finish. A short press still changes the volume. Dictate pastes where you are typing; Zen sends the recording to your Zen inbox.", ACTIONS},
            {"Gestures", "assistant_action", "choice", "Long-press power (assistant)", "Starts recording and stops after a pause, or tap Send. Needs steezen-stt set as the default digital assistant.", ACTIONS},
            {"Gestures", "earbud_action", "choice", "Earbud assist button", "Long-press on most Bluetooth earbuds. Stops after a pause, like the power gesture.", ACTIONS},
            {"Gestures", "earbud_check", "earbud", "Does the earbud button reach steezen-stt?", "", ""},
            {"Gestures", "earbud_media_session", "bool", "Earbud fallback: catch a long press of play/pause", "For earbuds that only send play/pause. Works until another app plays media; short presses still go to your music app (needs notification access).", ""},
            {"Assistant", "assistant_note", "note", "", "Assistant mode answers a spoken request by operating the phone with Claude. What leaves the phone: your request and, only while it runs, what the assistant reads (screen elements, notifications, contacts), sent straight to Anthropic with your key. Nothing from the screen is stored. It asks before sending, paying, deleting or changing settings, shows a Stop button the whole time, and never acts on the lock screen.", ""},
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
            {"Assistant permissions", "agent_allow_apps", "bool", "Open apps and links", "", ""},
            {"Assistant permissions", "agent_allow_contacts", "bool", "Look up contacts", "Asks Android for contacts access.", ""},
            {"Assistant permissions", "agent_allow_calls", "bool", "Place calls", "Always asks you first. Asks Android for phone access.", ""},
            {"Assistant permissions", "agent_allow_sms", "bool", "Send text messages", "Always asks you first. Asks Android for SMS access.", ""},
            {"Assistant permissions", "agent_allow_alarms", "bool", "Alarms, timers and reminders", "", ""},
            {"Assistant permissions", "agent_allow_calendar", "bool", "Read and add calendar events", "Asks Android for calendar access.", ""},
            {"Assistant permissions", "agent_allow_notifications", "bool", "Read notifications", "Needs notification access (Setup).", ""},
            {"Assistant permissions", "agent_allow_zen", "bool", "Send notes and long tasks to Zen", "", ""},
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
            "Set a gesture to Assistant and say \"text Sam I'm running late\": it asks before sending anything.",
            "The assistant's Stop button stays at the top of the screen while it works; the notification has one too.",
            "Assistant → Action log shows everything the assistant did, and Clear log wipes it.",
    };

    private JSONObject settings;
    private LinearLayout list;
    private EditText search;
    private TextView testResult;
    private String pendingExport;
    private float dp;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        dp = getResources().getDisplayMetrics().density;
        settings = Core.settings();
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setFitsSystemWindows(true);
        root.setBackgroundColor(getColor(R.color.bg));
        LinearLayout bar = new LinearLayout(this);
        bar.setOrientation(LinearLayout.VERTICAL);
        bar.setBackgroundColor(getColor(R.color.bar));
        bar.setPadding(px(16), px(12), px(16), px(12));
        TextView title = text("steezen-stt", 20, R.color.text);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        bar.addView(title);
        search = new EditText(this);
        search.setHint("Search settings and dictionary");
        search.setSingleLine();
        style(search);
        search.addTextChangedListener(new TextWatcher() {
            public void beforeTextChanged(CharSequence s, int a, int b, int c) {
            }

            public void onTextChanged(CharSequence s, int a, int b, int c) {
                rebuild();
            }

            public void afterTextChanged(Editable s) {
            }
        });
        bar.addView(search, matchWrap(px(8)));
        root.addView(bar);
        ScrollView scroll = new ScrollView(this);
        list = new LinearLayout(this);
        list.setOrientation(LinearLayout.VERTICAL);
        list.setPadding(px(16), px(8), px(16), px(32));
        scroll.addView(list);
        root.addView(scroll, new LinearLayout.LayoutParams(-1, 0, 1));
        setContentView(root);
        Dictation.listeners.add(this);
        handleIntent(getIntent());
    }

    @Override
    protected void onDestroy() {
        Dictation.listeners.remove(this);
        super.onDestroy();
    }

    @Override
    protected void onResume() {
        super.onResume();
        settings = Core.settings();
        rebuild();
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        handleIntent(intent);
    }

    private int px(float v) {
        return (int) (v * dp);
    }

    private LinearLayout.LayoutParams matchWrap(int top) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, -2);
        lp.topMargin = top;
        return lp;
    }

    private TextView text(String s, float size, int color) {
        TextView t = new TextView(this);
        t.setText(s);
        t.setTextSize(size);
        t.setTextColor(getColor(color));
        return t;
    }

    private GradientDrawable box(int fill, int stroke) {
        GradientDrawable g = new GradientDrawable();
        g.setColor(getColor(fill));
        g.setStroke(px(1), getColor(stroke));
        g.setCornerRadius(px(5));
        return g;
    }

    private void style(EditText e) {
        e.setTextColor(getColor(R.color.text));
        e.setHintTextColor(getColor(R.color.muted));
        e.setTextSize(15);
        e.setBackground(box(R.color.input, R.color.border));
        e.setPadding(px(10), px(8), px(10), px(8));
    }

    private void secret(EditText e) {
        e.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
        e.setTypeface(Typeface.DEFAULT);
    }

    private Button button(String label, boolean primary, View.OnClickListener click) {
        Button b = new Button(this);
        b.setText(label);
        b.setAllCaps(false);
        b.setTextColor(getColor(R.color.text));
        b.setBackground(box(primary ? R.color.accent : R.color.raised, primary ? R.color.accent : R.color.border));
        b.setOnClickListener(click);
        return b;
    }

    private LinearLayout card() {
        LinearLayout c = new LinearLayout(this);
        c.setOrientation(LinearLayout.VERTICAL);
        c.setBackground(box(R.color.surface, R.color.border));
        c.setPadding(px(14), px(10), px(14), px(12));
        list.addView(c, matchWrap(px(10)));
        return c;
    }

    private String query() {
        return search.getText().toString().trim().toLowerCase(Locale.ROOT);
    }

    private boolean matches(String... parts) {
        String q = query();
        if (q.isEmpty()) return true;
        return String.join(" ", parts).toLowerCase(Locale.ROOT).contains(q);
    }

    private void persist() {
        try {
            Core.save(settings);
            KeyService.syncEarbuds();
        } catch (Exception e) {
            Toast.makeText(this, "Couldn't save: " + e.getMessage(), Toast.LENGTH_LONG).show();
        }
    }

    private void rebuild() {
        list.removeAllViews();
        boolean searching = !query().isEmpty();
        if (!searching) tip();
        setup(searching);
        if (!searching) tester();
        String section = null;
        LinearLayout c = null;
        for (String[] f : FIELDS) {
            if (!matches(f[0], f[3], f[4])) continue;
            if (!f[0].equals(section)) {
                section = f[0];
                c = section(section, section, false);
            }
            c.addView(field(f));
        }
        dictionary();
        actionLog();
        backup();
        if (list.getChildCount() == 0) list.addView(text("Nothing matches \"" + search.getText() + "\".", 14, R.color.muted));
    }

    private SharedPreferences prefs() {
        return getSharedPreferences("ui", MODE_PRIVATE);
    }

    private LinearLayout section(String id, String title, boolean forceOpen) {
        LinearLayout c = card();
        LinearLayout head = new LinearLayout(this);
        head.setGravity(Gravity.CENTER_VERTICAL);
        head.setPadding(0, px(4), 0, px(4));
        TextView name = text(title, 15, R.color.text);
        name.setTypeface(Typeface.DEFAULT_BOLD);
        head.addView(name, new LinearLayout.LayoutParams(0, -2, 1));
        TextView chevron = text("", 22, R.color.muted);
        head.addView(chevron);
        c.addView(head);
        LinearLayout body = new LinearLayout(this);
        body.setOrientation(LinearLayout.VERTICAL);
        c.addView(body);
        boolean open = forceOpen || !query().isEmpty() || prefs().getStringSet("open", Set.of()).contains(id);
        Runnable show = () -> chevron.setText(body.getVisibility() == View.VISIBLE ? "▾" : "▸");
        body.setVisibility(open ? View.VISIBLE : View.GONE);
        show.run();
        head.setOnClickListener(v -> {
            boolean now = body.getVisibility() != View.VISIBLE;
            body.setVisibility(now ? View.VISIBLE : View.GONE);
            show.run();
            Set<String> ids = new HashSet<>(prefs().getStringSet("open", Set.of()));
            if (now) ids.add(id);
            else ids.remove(id);
            prefs().edit().putStringSet("open", ids).apply();
        });
        return body;
    }

    private View field(String[] f) {
        String key = f[1], kind = f[2];
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.VERTICAL);
        row.setPadding(0, px(8), 0, px(4));
        if (kind.equals("bool")) {
            Switch s = new Switch(this);
            s.setText(f[3]);
            s.setTextColor(getColor(R.color.text));
            s.setTextSize(15);
            s.setChecked(settings.optBoolean(key));
            s.setOnCheckedChangeListener((v, on) -> {
                put(key, on);
                persist();
                if (on) askFor(key);
            });
            row.addView(s);
        } else if (!kind.equals("note")) {
            row.addView(text(f[3], 15, R.color.text));
        }
        if (kind.equals("earbud")) {
            String who = earbudTarget();
            row.addView(text(who == null ? "Android finds no app for the earbud button on this phone; try the play/pause fallback below." : who.equals(getPackageName()) ? "Yes: the earbud button opens steezen-stt." : "Not yet: it opens " + appName(who) + ". Press the earbud button once and pick steezen-stt with \"Always\", or clear that app's defaults (App info → Open by default).", 13, who != null && who.equals(getPackageName()) ? R.color.green : R.color.amber));
        } else if (kind.equals("choice")) {
            LinearLayout pills = new LinearLayout(this);
            pills.setPadding(0, px(6), 0, 0);
            Object current = settings.opt(key);
            String value = current == null || current == JSONObject.NULL ? "None" : current.toString();
            for (String option : options(f[5])) {
                Button b = button(option, option.equals(value), v -> {
                    put(key, option.equals("None") ? JSONObject.NULL : option);
                    persist();
                    rebuild();
                });
                LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(0, -2, 1);
                lp.rightMargin = px(6);
                pills.addView(b, lp);
            }
            row.addView(pills);
        } else if (kind.equals("secret") || kind.equals("text") || kind.equals("int")) {
            EditText e = new EditText(this);
            e.setSingleLine();
            style(e);
            e.setText(settings.opt(key) == null ? "" : settings.opt(key).toString());
            if (kind.equals("secret")) secret(e);
            if (kind.equals("int")) e.setInputType(InputType.TYPE_CLASS_NUMBER);
            e.setOnFocusChangeListener((v, focused) -> {
                if (focused) return;
                String t = e.getText().toString().trim();
                put(key, kind.equals("int") ? (Object) parseInt(t) : t);
                persist();
            });
            row.addView(e, matchWrap(px(6)));
        }
        if (!f[4].isEmpty()) row.addView(text(f[4], kind.equals("note") ? 13 : 12, R.color.muted));
        return row;
    }

    private List<String> options(String spec) {
        if (!spec.startsWith("@")) return List.of(spec.split(","));
        List<String> out = new ArrayList<>();
        try {
            JSONArray a = Core.obj("agent_options", null).getJSONArray(spec.substring(1));
            for (int i = 0; i < a.length(); i++) out.add(a.getString(i));
        } catch (Exception ignored) {
        }
        return out;
    }

    private String earbudTarget() {
        for (String action : VOICE_INTENTS) {
            android.content.pm.ResolveInfo r = getPackageManager().resolveActivity(new Intent(action), PackageManager.MATCH_DEFAULT_ONLY);
            if (r != null && r.activityInfo != null && !"android".equals(r.activityInfo.packageName)) return r.activityInfo.packageName;
        }
        return null;
    }

    private String appName(String pkg) {
        try {
            return getPackageManager().getApplicationLabel(getPackageManager().getApplicationInfo(pkg, 0)).toString();
        } catch (Exception e) {
            return pkg;
        }
    }

    private void askFor(String key) {
        String[] perms = PERMS.get(key);
        if (perms != null) {
            for (String p : perms) if (checkSelfPermission(p) != PackageManager.PERMISSION_GRANTED) {
                requestPermissions(perms, 1);
                return;
            }
        }
        if ((key.equals("agent_allow_notifications") || key.equals("earbud_media_session")) && !AgentBridge.notificationsEnabled(this)) notificationAccess();
    }

    private void restricted(String title, String find, Intent settings) {
        new AlertDialog.Builder(this, R.style.Dialog).setTitle(title)
                .setMessage("Find " + find + " and turn it on.\n\nIf Android says it is a restricted setting (Android 13 and later, for apps not installed from a store): open the app info page (next button), tap ⋮ → Allow restricted settings, then come back here and try again.")
                .setPositiveButton("Open settings", (d, w) -> startActivity(settings))
                .setNeutralButton("App info", (d, w) -> startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:" + getPackageName()))))
                .setNegativeButton("Later", null)
                .show();
    }

    private void notificationAccess() {
        restricted("Turn on notification access", "steezen-stt assistant", new Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS));
    }

    private boolean usesAssistant() {
        for (String k : new String[]{"volume_action", "assistant_action", "earbud_action"}) if (Dictation.ASSISTANT.equals(settings.optString(k))) return true;
        return false;
    }

    private void actionLog() {
        List<String> lines = AgentLog.read(this);
        if (!matches("assistant action log history clear privacy") || (lines.isEmpty() && !usesAssistant())) return;
        LinearLayout c = section("Action log", "Assistant action log (" + lines.size() + ")", false);
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

    private static int parseInt(String s) {
        try {
            return Integer.parseInt(s);
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    private void put(String key, Object value) {
        try {
            settings.put(key, value);
        } catch (Exception ignored) {
        }
    }

    private void tip() {
        SharedPreferences prefs = getSharedPreferences("ui", MODE_PRIVATE);
        if (!prefs.getBoolean("tips", true) || prefs.getBoolean("tip_seen_" + todayIndex(), false)) return;
        LinearLayout c = card();
        c.addView(text("Tip", 12, R.color.accent_hi));
        c.addView(text(TIPS[todayIndex()], 14, R.color.text));
        LinearLayout row = new LinearLayout(this);
        row.setPadding(0, px(8), 0, 0);
        LinearLayout.LayoutParams gap = new LinearLayout.LayoutParams(-2, -2);
        gap.rightMargin = px(8);
        row.addView(button("Got it", false, v -> {
            prefs.edit().putBoolean("tip_seen_" + todayIndex(), true).apply();
            rebuild();
        }), gap);
        row.addView(button("Stop showing tips", false, v -> {
            prefs.edit().putBoolean("tips", false).apply();
            Toast.makeText(this, "Tips are off. Turn them back on under Export & import → Show tips.", Toast.LENGTH_LONG).show();
            rebuild();
        }));
        c.addView(row);
    }

    private int todayIndex() {
        return (int) (System.currentTimeMillis() / 86_400_000L % TIPS.length);
    }

    private boolean assistantSet() {
        RoleManager rm = getSystemService(RoleManager.class);
        return rm != null && rm.isRoleHeld(RoleManager.ROLE_ASSISTANT);
    }

    private void setup(boolean searching) {
        Object[][] steps = {
                {"Microphone", checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED, "Grant", (Runnable) () -> requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO, Manifest.permission.POST_NOTIFICATIONS}, 0)},
                {"Volume-down push-to-talk and pasting (accessibility service)", KeyService.enabled(), "Open settings", (Runnable) () -> restricted("Turn on the accessibility service", "steezen-stt push-to-talk", new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))},
                {"Long-press power: default digital assistant", assistantSet(), "Open settings", (Runnable) () -> startActivity(new Intent(Settings.ACTION_VOICE_INPUT_SETTINGS))},
                {"Assistant: turn on Zen phone control (a second, separate accessibility service)", !usesAssistant() || AgentBridge.enabled(this, AgentService.class), "Open settings", (Runnable) () -> restricted("Turn on Zen phone control", "Zen phone control", new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))},
                {"Assistant: add your Anthropic API key (Assistant section)", !usesAssistant() || !settings.optString("anthropic_api_key").trim().isEmpty(), "Show", (Runnable) () -> search.setText("Anthropic")},
                {"Notification access (assistant notifications, earbud fallback)", !(settings.optBoolean("agent_allow_notifications") || settings.optBoolean("earbud_media_session")) || AgentBridge.notificationsEnabled(this), "Open settings", (Runnable) this::notificationAccess},
                {"Keep running in the background (no battery optimisation)", getSystemService(PowerManager.class).isIgnoringBatteryOptimizations(getPackageName()), "Allow", (Runnable) () -> startActivity(new Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:" + getPackageName())))},
                {"Exact reminder times (optional)", !settings.optBoolean("agent_allow_alarms") || !usesAssistant() || getSystemService(AlarmManager.class).canScheduleExactAlarms(), "Allow", (Runnable) () -> startActivity(new Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.parse("package:" + getPackageName())))},
        };
        List<Object[]> todo = new ArrayList<>();
        for (Object[] s : steps) if (!(Boolean) s[1] && matches("setup", (String) s[0])) todo.add(s);
        if (todo.isEmpty()) return;
        LinearLayout c = section("Setup", "Setup", true);
        for (Object[] s : todo) {
            LinearLayout row = new LinearLayout(this);
            row.setGravity(Gravity.CENTER_VERTICAL);
            row.setPadding(0, px(4), 0, px(4));
            TextView label = text((String) s[0], 14, R.color.amber);
            row.addView(label, new LinearLayout.LayoutParams(0, -2, 1));
            row.addView(button((String) s[2], true, v -> ((Runnable) s[3]).run()));
            c.addView(row);
        }
    }

    @SuppressWarnings("ClickableViewAccessibility")
    private void tester() {
        LinearLayout c = section("Try it", "Try it", false);
        Button hold = button("Hold to dictate here", true, null);
        hold.setOnTouchListener((v, e) -> {
            if (e.getAction() == MotionEvent.ACTION_DOWN) Dictation.start(this, "Dictate", false);
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

    private void dictionary() {
        JSONArray rules = settings.optJSONArray("dictionary");
        if (rules == null) rules = new JSONArray();
        List<Integer> shown = new ArrayList<>();
        for (int i = 0; i < rules.length(); i++) {
            JSONObject r = rules.optJSONObject(i);
            if (matches("dictionary word", r.optString("word"), r.optString("triggers"), r.optString("meaning"))) shown.add(i);
        }
        boolean addMatches = matches("dictionary add word");
        if (shown.isEmpty() && !addMatches) return;
        LinearLayout c = section("Dictionary words", "Dictionary words (" + rules.length() + ")", false);
        for (int i : shown) {
            JSONObject r = rules.optJSONObject(i);
            LinearLayout row = new LinearLayout(this);
            row.setOrientation(LinearLayout.VERTICAL);
            row.setPadding(0, px(6), 0, px(6));
            row.addView(text(r.optString("word") + (r.optBoolean("enabled", true) ? "" : "  (off)"), 15, R.color.text));
            if (!r.optString("triggers").isEmpty()) row.addView(text("heard as: " + r.optString("triggers"), 12, R.color.muted));
            row.setOnClickListener(v -> editRule(i));
            c.addView(row);
        }
        c.addView(button("Add word", false, v -> editRule(-1)), matchWrap(px(6)));
    }

    private void editRule(int index) {
        JSONArray rules = settings.optJSONArray("dictionary");
        JSONObject rule;
        try {
            rule = index >= 0 ? rules.getJSONObject(index) : new JSONObject().put("enabled", true).put("threshold", 0.6).put("alternatives", new JSONArray());
        } catch (Exception e) {
            return;
        }
        String[][] parts = {{"word", "Word (how it should be written)"}, {"triggers", "Heard as (comma separated)"}, {"meaning", "What it means"}, {"context", "When it applies"}};
        LinearLayout form = new LinearLayout(this);
        form.setOrientation(LinearLayout.VERTICAL);
        form.setPadding(px(20), px(8), px(20), 0);
        List<EditText> edits = new ArrayList<>();
        for (String[] p : parts) {
            form.addView(text(p[1], 13, R.color.muted));
            EditText e = new EditText(this);
            style(e);
            e.setText(rule.optString(p[0]));
            form.addView(e, matchWrap(px(4)));
            edits.add(e);
        }
        CheckBox on = new CheckBox(this);
        on.setText("On");
        on.setTextColor(getColor(R.color.text));
        on.setChecked(rule.optBoolean("enabled", true));
        form.addView(on);
        AlertDialog.Builder b = new AlertDialog.Builder(this, R.style.Dialog).setTitle(index >= 0 ? "Edit word" : "Add word").setView(form)
                .setPositiveButton("Save", (d, w) -> {
                    try {
                        for (int i = 0; i < parts.length; i++) rule.put(parts[i][0], edits.get(i).getText().toString().trim());
                        rule.put("enabled", on.isChecked());
                        if (rule.optString("word").isEmpty()) return;
                        if (index < 0) rules.put(rule);
                        persist();
                        rebuild();
                    } catch (Exception ignored) {
                    }
                })
                .setNegativeButton("Cancel", null);
        if (index >= 0) b.setNeutralButton("Delete", (d, w) -> {
            rules.remove(index);
            persist();
            rebuild();
        });
        b.show();
    }

    private void backup() {
        if (!matches("export import backup settings file share email phone desktop show tips general")) return;
        LinearLayout c = section("Export & import", "Export & import", false);
        c.addView(text("Move settings between this phone and the desktop app. API keys only travel in a file locked with a passphrase.", 13, R.color.muted));
        LinearLayout row = new LinearLayout(this);
        row.setPadding(0, px(8), 0, 0);
        LinearLayout.LayoutParams half = new LinearLayout.LayoutParams(0, -2, 1);
        half.rightMargin = px(6);
        row.addView(button("Export…", true, v -> exportDialog()), half);
        row.addView(button("Import file…", false, v -> startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*"), OPEN_FILE)), new LinearLayout.LayoutParams(0, -2, 1));
        c.addView(row);
        SharedPreferences prefs = getSharedPreferences("ui", MODE_PRIVATE);
        Switch tips = new Switch(this);
        tips.setText("Show tips");
        tips.setTextColor(getColor(R.color.text));
        tips.setChecked(prefs.getBoolean("tips", true));
        tips.setOnCheckedChangeListener((v, on) -> prefs.edit().putBoolean("tips", on).apply());
        c.addView(tips, matchWrap(px(10)));
    }

    private void exportDialog() {
        JSONArray groups;
        try {
            groups = (JSONArray) Core.run("groups", null, null);
        } catch (Exception e) {
            Toast.makeText(this, "Couldn't list settings groups: " + e, Toast.LENGTH_LONG).show();
            return;
        }
        LinearLayout form = new LinearLayout(this);
        form.setOrientation(LinearLayout.VERTICAL);
        form.setPadding(px(20), px(8), px(20), 0);
        List<CheckBox> boxes = new ArrayList<>();
        for (int i = 0; i < groups.length(); i++) {
            JSONObject g = groups.optJSONObject(i);
            CheckBox b = new CheckBox(this);
            b.setText(g.optString("label"));
            b.setTextColor(getColor(R.color.text));
            b.setTag(g.optString("id"));
            b.setChecked(!g.optBoolean("secret"));
            form.addView(b);
            boxes.add(b);
        }
        EditText pass = new EditText(this);
        style(pass);
        pass.setHint("Passphrase (needed for API keys)");
        secret(pass);
        form.addView(pass, matchWrap(px(8)));
        AlertDialog d = new AlertDialog.Builder(this, R.style.Dialog).setTitle("Export settings").setView(form)
                .setPositiveButton("Share…", null).setNeutralButton("Save to file", null).setNegativeButton("Cancel", null).show();
        View.OnClickListener go = v -> {
            try {
                JSONArray chosen = new JSONArray();
                for (CheckBox b : boxes) if (b.isChecked()) chosen.put(b.getTag());
                String text = Core.obj("export", new JSONObject().put("groups", chosen).put("pass", pass.getText().toString())).getString("text");
                d.dismiss();
                if (v == d.getButton(AlertDialog.BUTTON_POSITIVE)) share(text);
                else {
                    pendingExport = text;
                    startActivityForResult(new Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("application/json").putExtra(Intent.EXTRA_TITLE, "steezen-stt-settings.steezen.json"), SAVE_FILE);
                }
            } catch (Exception e) {
                Toast.makeText(this, e.getMessage(), Toast.LENGTH_LONG).show();
            }
        };
        d.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(go);
        d.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener(go);
    }

    private void share(String text) throws Exception {
        File f = new File(ShareProvider.dir(this), "steezen-stt-settings.steezen.json");
        try (FileOutputStream out = new FileOutputStream(f)) {
            out.write(text.getBytes(StandardCharsets.UTF_8));
        }
        Intent send = new Intent(Intent.ACTION_SEND).setType("application/json").putExtra(Intent.EXTRA_STREAM, ShareProvider.uri(f))
                .putExtra(Intent.EXTRA_SUBJECT, "steezen-stt settings").addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        startActivity(Intent.createChooser(send, "Send settings"));
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (result != RESULT_OK || data == null || data.getData() == null) return;
        try {
            if (request == SAVE_FILE && pendingExport != null) {
                try (OutputStream out = getContentResolver().openOutputStream(data.getData())) {
                    out.write(pendingExport.getBytes(StandardCharsets.UTF_8));
                }
                pendingExport = null;
                Toast.makeText(this, "Saved", Toast.LENGTH_SHORT).show();
            } else if (request == OPEN_FILE) {
                importText(read(data.getData()), "");
            }
        } catch (Exception e) {
            Toast.makeText(this, e.getMessage(), Toast.LENGTH_LONG).show();
        }
    }

    private String read(Uri uri) throws Exception {
        try (InputStream in = getContentResolver().openInputStream(uri)) {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buf = new byte[8192];
            for (int n; (n = in.read(buf)) > 0; ) out.write(buf, 0, n);
            return out.toString(StandardCharsets.UTF_8);
        }
    }

    private void handleIntent(Intent intent) {
        if (intent == null || Intent.ACTION_MAIN.equals(intent.getAction())) return;
        try {
            Uri uri = Intent.ACTION_SEND.equals(intent.getAction()) ? intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri.class) : intent.getData();
            String text = uri != null ? read(uri) : intent.getStringExtra(Intent.EXTRA_TEXT);
            if (text != null) importText(text, "");
        } catch (Exception e) {
            Toast.makeText(this, "Couldn't read the file: " + e.getMessage(), Toast.LENGTH_LONG).show();
        }
        setIntent(new Intent(Intent.ACTION_MAIN));
    }

    private void importText(String text, String pass) {
        JSONObject opened;
        try {
            opened = Core.obj("open", new JSONObject().put("text", text).put("pass", pass));
        } catch (Exception e) {
            Toast.makeText(this, e.getMessage(), Toast.LENGTH_LONG).show();
            return;
        }
        if (opened.optBoolean("locked")) {
            EditText p = new EditText(this);
            style(p);
            secret(p);
            new AlertDialog.Builder(this, R.style.Dialog).setTitle("This file is locked").setMessage("Enter the passphrase it was exported with.").setView(p)
                    .setPositiveButton("Open", (d, w) -> importText(text, p.getText().toString())).setNegativeButton("Cancel", null).show();
            return;
        }
        JSONArray groups = opened.optJSONArray("groups");
        if (groups == null || groups.length() == 0) {
            Toast.makeText(this, "The file has no settings in it", Toast.LENGTH_LONG).show();
            return;
        }
        LinearLayout form = new LinearLayout(this);
        form.setOrientation(LinearLayout.VERTICAL);
        form.setPadding(px(20), px(8), px(20), 0);
        form.addView(text("Pick what to apply. Everything else stays as it is.", 13, R.color.muted));
        List<CheckBox> boxes = new ArrayList<>();
        boolean hasDictionary = false;
        for (int i = 0; i < groups.length(); i++) {
            JSONObject g = groups.optJSONObject(i);
            CheckBox b = new CheckBox(this);
            b.setText(g.optString("label"));
            b.setTextColor(getColor(R.color.text));
            b.setTag(g.optString("id"));
            b.setChecked(!g.optString("id").equals("desktop"));
            form.addView(b);
            boxes.add(b);
            hasDictionary |= g.optString("id").equals("dictionary");
        }
        CheckBox merge = new CheckBox(this);
        merge.setText("Merge the dictionary (keep my words, add and update theirs)");
        merge.setTextColor(getColor(R.color.text));
        merge.setChecked(true);
        if (hasDictionary) form.addView(merge, matchWrap(px(8)));
        new AlertDialog.Builder(this, R.style.Dialog).setTitle("Import settings").setView(form).setPositiveButton("Apply", (d, w) -> {
            try {
                JSONArray choices = new JSONArray();
                for (CheckBox b : boxes) if (b.isChecked()) choices.put(new JSONArray().put(b.getTag()).put(b.getTag().equals("dictionary") && merge.isChecked()));
                settings = Core.obj("apply", new JSONObject().put("text", text).put("pass", pass).put("choices", choices));
                rebuild();
                Toast.makeText(this, "Imported " + choices.length() + " group(s)", Toast.LENGTH_SHORT).show();
            } catch (Exception e) {
                Toast.makeText(this, e.getMessage(), Toast.LENGTH_LONG).show();
            }
        }).setNegativeButton("Cancel", null).show();
    }
}
