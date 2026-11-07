package com.slgotting.steezenstt;

import android.Manifest;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.pm.PackageManager;
import android.media.AudioManager;
import android.media.ToneGenerator;
import android.os.Handler;
import android.os.Looper;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.widget.Toast;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import org.json.JSONObject;

final class Dictation {
    interface Listener {
        void state(String phase, String detail);
    }

    static final String ZEN = "Zen";
    static final String ASSISTANT = "Assistant";
    private static final long SILENCE_MS = 1800;
    private static final long MAX_MS = 120_000;
    private static final Map<String, String[]> PHASES = Map.of(
            ZEN, new String[]{"Listening for Zen…", "Sending to Zen…", "zen"},
            ASSISTANT, new String[]{"Listening for the assistant…", "Understanding…", "hear"});
    private static final String[] DICTATE = {"Listening…", "Transcribing…", "dictate"};
    static final List<Listener> listeners = new CopyOnWriteArrayList<>();
    private static final Handler main = new Handler(Looper.getMainLooper());
    private static final ExecutorService work = Executors.newSingleThreadExecutor();
    private static final ToneGenerator tones = new ToneGenerator(AudioManager.STREAM_NOTIFICATION, 60);
    private static Recorder rec;
    private static String action;
    private static boolean paste;

    static boolean recording() {
        return rec != null;
    }

    static long silentMs() {
        return rec == null || !rec.heard ? 0 : rec.silentMs();
    }

    static long elapsedMs() {
        return rec == null ? 0 : System.currentTimeMillis() - rec.started;
    }

    private static void publish(String phase, String detail) {
        for (Listener l : listeners) l.state(phase, detail);
    }

    private static void beep(JSONObject s, String key, int tone) {
        if (s.optBoolean(key)) tones.startTone(tone, 120);
    }

    private static String[] phases(String act) {
        return PHASES.getOrDefault(act, DICTATE);
    }

    static boolean handsFree(Context c, String act) {
        return handsFree(c, act, false);
    }

    static boolean handsFree(Context c, String act, boolean pasteResult) {
        if (!start(c, act, pasteResult)) return false;
        Recorder mine = rec;
        main.postDelayed(new Runnable() {
            @Override
            public void run() {
                if (rec != mine) return;
                if (silentMs() > SILENCE_MS || elapsedMs() > MAX_MS) stop(c);
                else main.postDelayed(this, 200);
            }
        }, 200);
        return true;
    }

    static boolean start(Context c, String act, boolean pasteResult) {
        if (rec != null) return false;
        String problem = ASSISTANT.equals(act) ? AgentBridge.problem(c) : null;
        if (problem != null) {
            publish("error", problem);
            toast(c, problem);
            return false;
        }
        if (c.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            toast(c, "steezen-stt needs the microphone permission. Open the app to grant it.");
            return false;
        }
        RecordService.start(c);
        Recorder r = new Recorder();
        try {
            r.start();
        } catch (Exception e) {
            RecordService.stop(c);
            toast(c, e.getMessage());
            return false;
        }
        rec = r;
        action = act;
        paste = pasteResult;
        c.getSystemService(Vibrator.class).vibrate(VibrationEffect.createOneShot(30, VibrationEffect.DEFAULT_AMPLITUDE));
        beep(Core.settings(), "sound_start", ToneGenerator.TONE_PROP_BEEP);
        publish("recording", phases(act)[0]);
        return true;
    }

    static void cancel(Context c) {
        if (rec == null) return;
        rec.stop();
        rec = null;
        RecordService.stop(c);
        beep(Core.settings(), "sound_cancel", ToneGenerator.TONE_PROP_NACK);
        publish("idle", "");
    }

    static void stop(Context c) {
        if (rec == null) return;
        byte[] pcm = rec.stop();
        rec = null;
        RecordService.stop(c);
        String act = action;
        boolean deliver = paste;
        JSONObject s = Core.settings();
        beep(s, "sound_stop", ToneGenerator.TONE_PROP_BEEP2);
        publish("working", phases(act)[1]);
        Context app = c.getApplicationContext();
        work.execute(() -> {
            try {
                JSONObject r = (JSONObject) Core.run(phases(act)[2], null, pcm);
                main.post(() -> done(app, act, deliver, r));
            } catch (Exception e) {
                main.post(() -> {
                    publish("error", e.getMessage());
                    toast(app, e.getMessage());
                });
            }
        });
    }

    private static void done(Context c, String act, boolean deliver, JSONObject r) {
        JSONObject s = Core.settings();
        String text = r.optString("text");
        beep(s, "sound_complete", ToneGenerator.TONE_PROP_ACK);
        if (ZEN.equals(act)) {
            publish("done", text);
            toast(c, text);
            return;
        }
        if (ASSISTANT.equals(act)) {
            if (text.trim().isEmpty()) {
                publish("error", "Nothing heard");
                toast(c, "Nothing heard");
                return;
            }
            publish("done", "Asked: " + text);
            AgentBridge.send(c, text);
            return;
        }
        if (text.trim().isEmpty()) {
            publish("error", "Nothing heard");
            toast(c, "Nothing heard");
            return;
        }
        boolean clip = s.optBoolean("output_clipboard", true);
        if (clip) c.getSystemService(ClipboardManager.class).setPrimaryClip(ClipData.newPlainText("steezen-stt", text));
        String how = clip ? "Copied" : "Done";
        if (deliver && s.optBoolean("output_cursor", true)) {
            how = KeyService.insert(text, clip, s.optBoolean("output_enter")) ? (s.optBoolean("output_enter") ? "Pasted + Enter" : "Pasted") : (clip ? "Copied (no text box to paste into)" : "No text box to paste into");
        }
        publish("done", text);
        if (s.optBoolean("notify_complete") || !how.startsWith("Pasted")) toast(c, how + ": " + text);
    }

    static void toast(Context c, String text) {
        main.post(() -> Toast.makeText(c.getApplicationContext(), text, Toast.LENGTH_SHORT).show());
    }
}
