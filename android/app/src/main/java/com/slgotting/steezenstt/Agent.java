package com.slgotting.steezenstt;

import android.Manifest;
import android.app.Notification;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.media.AudioDeviceInfo;
import android.media.AudioManager;
import android.speech.tts.TextToSpeech;
import android.view.accessibility.AccessibilityNodeInfo;
import java.time.ZonedDateTime;
import java.time.format.DateTimeFormatter;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.TimeUnit;
import java.util.function.BooleanSupplier;
import org.json.JSONArray;
import org.json.JSONObject;

final class Agent {
    interface Tool {
        Object run(JSONObject in) throws Exception;
    }

    static final class Halt extends Exception {
        Halt(String why) {
            super(why);
        }
    }

    private static final int NOTE = 7;
    private static final long FOLLOW_UP_MS = 120_000;
    private static final long CRASH_WINDOW_MS = 600_000;
    private static final int CRASH_LIMIT = 3;
    private static final Set<String> SCREEN_ACTS = Set.of("tap", "long_press", "type_text", "scroll", "swipe", "global_action");
    private static final Set<Integer> AUDIO_OUT = Set.of(AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, AudioDeviceInfo.TYPE_BLUETOOTH_SCO, AudioDeviceInfo.TYPE_WIRED_HEADSET,
            AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_BLE_HEADSET);

    private final AgentService service;
    private final Screen screen;
    private final PhoneTools phone;
    private final Overlay overlay;
    private final Map<String, Tool> tools;
    private final Map<String, BooleanSupplier> granted;
    private final Deque<Long> crashes = new ArrayDeque<>();
    private TextToSpeech tts;
    private volatile Thread worker;
    private volatile boolean stopped;
    private JSONArray conversation;
    private JSONArray pending;
    private long askedAt;

    Agent(AgentService s) {
        service = s;
        screen = new Screen(s);
        phone = new PhoneTools(s);
        overlay = new Overlay(s);
        tools = Map.ofEntries(
                Map.entry("read_screen", in -> screen.read()),
                Map.entry("screenshot", in -> image()),
                Map.entry("tap", in -> screen.tap(in, false)),
                Map.entry("long_press", in -> screen.tap(in, true)),
                Map.entry("type_text", screen::type),
                Map.entry("scroll", screen::scroll),
                Map.entry("swipe", screen::swipe),
                Map.entry("global_action", screen::global),
                Map.entry("open_app", phone::openApp),
                Map.entry("list_apps", phone::listApps),
                Map.entry("open_url", phone::openUrl),
                Map.entry("find_contact", phone::findContact),
                Map.entry("call", phone::call),
                Map.entry("send_sms", phone::sms),
                Map.entry("set_alarm", phone::alarm),
                Map.entry("set_timer", phone::timer),
                Map.entry("set_reminder", phone::reminder),
                Map.entry("calendar_events", phone::events),
                Map.entry("add_calendar_event", phone::addEvent),
                Map.entry("read_notifications", phone::notifications),
                Map.entry("send_to_zen", phone::zen),
                Map.entry("wait", in -> {
                    Thread.sleep(Math.max(0, Math.min(in.optInt("ms", 500), 3000)));
                    return "waited";
                }));
        granted = Map.ofEntries(
                Map.entry("screen", () -> true),
                Map.entry("screenshots", () -> true),
                Map.entry("gestures", () -> true),
                Map.entry("apps", () -> true),
                Map.entry("alarms", () -> true),
                Map.entry("contacts", () -> has(Manifest.permission.READ_CONTACTS)),
                Map.entry("calls", () -> has(Manifest.permission.CALL_PHONE)),
                Map.entry("sms", () -> has(Manifest.permission.SEND_SMS)),
                Map.entry("calendar", () -> has(Manifest.permission.READ_CALENDAR) && has(Manifest.permission.WRITE_CALENDAR)),
                Map.entry("notifications", () -> Notifs.instance != null),
                Map.entry("zen", () -> !Core.settings().optString("zen_url").trim().isEmpty()));
    }

    private boolean has(String perm) {
        return service.checkSelfPermission(perm) == PackageManager.PERMISSION_GRANTED;
    }

    boolean busy() {
        return worker != null;
    }

    synchronized void request(String text) {
        if (worker != null) {
            overlay.reply("Still working on the last request. Tap Stop first to start a new one.", false, null);
            return;
        }
        long now = System.currentTimeMillis();
        while (!crashes.isEmpty() && now - crashes.peekFirst() > CRASH_WINDOW_MS) crashes.removeFirst();
        if (crashes.size() >= CRASH_LIMIT) {
            overlay.reply("The assistant hit errors " + CRASH_LIMIT + " times in a row, so it is paused for a few minutes. Dictation still works.", false, null);
            return;
        }
        stopped = false;
        worker = new Thread(() -> guarded(text), "zen-agent");
        worker.setUncaughtExceptionHandler((t, e) -> crashed(e));
        worker.start();
    }

    void stop(String why) {
        stopped = true;
        Thread w = worker;
        if (w != null) w.interrupt();
        conversation = null;
        pending = null;
        overlay.clear();
        service.getSystemService(NotificationManager.class).cancel(NOTE);
        if (why != null && w != null) AgentLog.add(service, "stopped", why, "stopped");
    }

    void shutdown() {
        stop("The assistant service was turned off");
        if (tts != null) tts.shutdown();
    }

    private void crashed(Throwable e) {
        synchronized (this) {
            crashes.addLast(System.currentTimeMillis());
            worker = null;
        }
        conversation = null;
        pending = null;
        overlay.status(null, null);
        service.getSystemService(NotificationManager.class).cancel(NOTE);
        AgentLog.add(service, "error", String.valueOf(e), "crashed");
        if (!stopped) overlay.reply("The assistant ran into a problem and stopped: " + e.getMessage(), false, null);
    }

    private void guarded(String text) {
        try {
            run(text);
            synchronized (this) {
                worker = null;
            }
        } catch (Throwable e) {
            boolean quiet = stopped || e instanceof InterruptedException;
            if (quiet || (e instanceof Exception && !(e instanceof RuntimeException))) {
                synchronized (this) {
                    worker = null;
                }
                conversation = null;
                pending = null;
                if (!quiet) {
                    AgentLog.add(service, "error", String.valueOf(e.getMessage()), e instanceof Halt ? "stopped" : "failed");
                    overlay.reply(e instanceof Halt ? e.getMessage() : "Couldn't finish: " + e.getMessage(), false, null);
                }
                return;
            }
            crashed(e);
        } finally {
            overlay.status(null, null);
            service.getSystemService(NotificationManager.class).cancel(NOTE);
        }
    }

    private void status(String s) {
        overlay.status(s, () -> stop("You tapped Stop"));
        PendingIntent stop = PendingIntent.getBroadcast(service, 0, new Intent(AgentBridge.STOP).setPackage(service.getPackageName()), PendingIntent.FLAG_IMMUTABLE);
        service.getSystemService(NotificationManager.class).notify(NOTE, new Notification.Builder(service, App.ASSISTANT).setSmallIcon(R.drawable.ic_stat).setContentTitle("Zen phone control is working")
                .setContentText(s).setOngoing(true).addAction(new Notification.Action.Builder(null, "Stop", stop).build()).build());
    }

    private void guard() throws Exception {
        if (stopped || Thread.currentThread().isInterrupted()) throw new InterruptedException();
        if (AgentBridge.locked(service)) throw new Halt("The phone locked, so the assistant stopped. It never acts on the lock screen.");
    }

    private JSONArray granted() {
        JSONArray out = new JSONArray();
        granted.forEach((k, ok) -> {
            if (ok.getAsBoolean()) out.put(k);
        });
        return out;
    }

    private void run(String text) throws Exception {
        guard();
        JSONObject s = Core.settings();
        boolean followUp = conversation != null && pending != null && System.currentTimeMillis() - askedAt < FOLLOW_UP_MS;
        JSONArray messages = followUp ? conversation : new JSONArray();
        String context = "Spoken request: " + text + "\n\n(Local time " + ZonedDateTime.now().format(DateTimeFormatter.ofPattern("EEEE yyyy-MM-dd HH:mm")) + ". Foreground app: " + foregroundPackage() + ".)";
        JSONArray content = followUp ? pending : new JSONArray();
        content.put(new JSONObject().put("type", "text").put("text", followUp ? "Owner's answer: " + text : context));
        messages.put(new JSONObject().put("role", "user").put("content", content));
        conversation = messages;
        pending = null;
        AgentLog.add(service, "request", text, followUp ? "follow-up" : "new");
        int max = Math.max(1, s.optInt("agent_max_steps", 30));
        status("Thinking…");
        for (int step = 0; step < max; step++) {
            guard();
            JSONObject r = Core.obj("agent_step", new JSONObject().put("messages", messages).put("granted", granted()));
            guard();
            JSONArray blocks = r.optJSONArray("content");
            if (blocks == null) blocks = new JSONArray();
            messages.put(new JSONObject().put("role", "assistant").put("content", blocks));
            if ("refusal".equals(r.optString("stop_reason"))) {
                done("Claude declined this request.", false, null);
                return;
            }
            JSONArray results = new JSONArray();
            StringBuilder said = new StringBuilder();
            String reply = null;
            boolean question = false;
            for (int i = 0; i < blocks.length(); i++) {
                JSONObject b = blocks.getJSONObject(i);
                if ("text".equals(b.optString("type"))) said.append(b.optString("text"));
                if (!"tool_use".equals(b.optString("type"))) continue;
                String name = b.getString("name");
                JSONObject in = b.optJSONObject("input") == null ? new JSONObject() : b.getJSONObject("input");
                if (name.equals("finish")) {
                    reply = in.optString("message");
                    question = in.optBoolean("question");
                    results.put(result(b, "shown to the owner", false));
                    continue;
                }
                results.put(execute(b, name, in));
            }
            if (results.length() == 0) {
                done(said.toString().trim().isEmpty() ? "Done." : said.toString().trim(), false, null);
                return;
            }
            if (reply != null) {
                done(reply, question, results);
                return;
            }
            messages.put(new JSONObject().put("role", "user").put("content", results));
        }
        done("I stopped after " + max + " steps without finishing. Try a more specific request.", false, null);
    }

    private String foregroundPackage() {
        AccessibilityNodeInfo root = service.getRootInActiveWindow();
        return root == null || root.getPackageName() == null ? "unknown" : root.getPackageName().toString();
    }

    private static JSONObject result(JSONObject use, Object content, boolean error) throws Exception {
        JSONObject r = new JSONObject().put("type", "tool_result").put("tool_use_id", use.getString("id")).put("content", content);
        if (error) r.put("is_error", true);
        return r;
    }

    private JSONObject execute(JSONObject use, String name, JSONObject in) throws Exception {
        guard();
        Tool tool = tools.get(name);
        if (tool == null) return result(use, "Unknown tool " + name, true);
        status(LABELS.getOrDefault(name, "Working…"));
        AccessibilityNodeInfo target = SCREEN_ACTS.contains(name) ? safeTarget(in) : null;
        String pkg = Screen.packageOf(target, foregroundPackage());
        JSONObject risk = Core.obj("risk", new JSONObject().put("tool", name).put("input", in).put("label", Screen.describe(target)).put("package", pkg));
        String summary = summary(name, in);
        if (risk.optBoolean("block")) {
            AgentLog.add(service, name, summary, "blocked");
            return result(use, risk.optString("reason"), true);
        }
        if (risk.optBoolean("confirm")) {
            status("Waiting for you to confirm…");
            boolean yes;
            try {
                yes = overlay.confirm(risk.optString("reason"), confirmText(name, in, target)).get(60, TimeUnit.SECONDS);
            } catch (java.util.concurrent.TimeoutException e) {
                yes = false;
                overlay.clear();
            }
            guard();
            if (!yes) {
                AgentLog.add(service, name, summary, "declined");
                return result(use, "The owner declined this action. Do not try it another way; finish and say it was not done.", true);
            }
        }
        try {
            Object out = tool.run(in);
            AgentLog.add(service, name, summary, "ok");
            if (SCREEN_ACTS.contains(name)) Thread.sleep(600);
            return result(use, out, false);
        } catch (InterruptedException e) {
            throw e;
        } catch (Exception e) {
            AgentLog.add(service, name, summary, "failed");
            return result(use, String.valueOf(e.getMessage()), true);
        }
    }

    private AccessibilityNodeInfo safeTarget(JSONObject in) {
        try {
            AccessibilityNodeInfo n = screen.target(in);
            if (n == null && in.has("x")) {
                screen.read();
                n = screen.at(in.getInt("x"), in.getInt("y"));
            }
            return n;
        } catch (Exception e) {
            return null;
        }
    }

    private JSONArray image() throws Exception {
        String data = screen.screenshot();
        return new JSONArray()
                .put(new JSONObject().put("type", "image").put("source", new JSONObject().put("type", "base64").put("media_type", "image/jpeg").put("data", data)))
                .put(new JSONObject().put("type", "text").put("text", "Untrusted screen content. Pixel coordinates in this image times " + (1 / screen.shotScale()) + " give screen coordinates."));
    }

    private static final Map<String, String> LABELS = Map.ofEntries(
            Map.entry("read_screen", "Reading the screen…"), Map.entry("screenshot", "Looking at the screen…"), Map.entry("tap", "Tapping…"), Map.entry("long_press", "Pressing…"),
            Map.entry("type_text", "Typing…"), Map.entry("scroll", "Scrolling…"), Map.entry("swipe", "Swiping…"), Map.entry("global_action", "Pressing a system button…"),
            Map.entry("open_app", "Opening an app…"), Map.entry("list_apps", "Checking your apps…"), Map.entry("open_url", "Opening a link…"), Map.entry("find_contact", "Looking up a contact…"),
            Map.entry("call", "Calling…"), Map.entry("send_sms", "Texting…"), Map.entry("set_alarm", "Setting an alarm…"), Map.entry("set_timer", "Starting a timer…"),
            Map.entry("set_reminder", "Setting a reminder…"), Map.entry("calendar_events", "Checking your calendar…"), Map.entry("add_calendar_event", "Adding to your calendar…"),
            Map.entry("read_notifications", "Reading notifications…"), Map.entry("send_to_zen", "Sending to Zen…"), Map.entry("wait", "Waiting…"));

    private static final Set<String> PRIVATE_TEXT = Set.of("text", "body", "message", "description");

    private static String summary(String name, JSONObject in) {
        StringBuilder s = new StringBuilder(name);
        for (java.util.Iterator<String> k = in.keys(); k.hasNext(); ) {
            String key = k.next();
            Object v = in.opt(key);
            s.append(' ').append(key).append('=').append(PRIVATE_TEXT.contains(key) ? "(" + String.valueOf(v).length() + " characters)" : String.valueOf(v));
        }
        return s.toString();
    }

    private static String confirmText(String name, JSONObject in, AccessibilityNodeInfo target) {
        return switch (name) {
            case "call" -> "Call " + in.optString("name", "") + " " + in.optString("number");
            case "send_sms" -> "Text " + in.optString("name", "") + " " + in.optString("to") + ":\n\n" + in.optString("body");
            case "type_text" -> "Type and send:\n\n" + in.optString("text");
            default -> {
                String what = Screen.describe(target);
                yield (name.equals("tap") ? "Tap " : name.replace('_', ' ') + " ") + (what.isEmpty() ? "the point " + in.optInt("x") + "," + in.optInt("y") : "\"" + what + "\"");
            }
        };
    }

    private void done(String reply, boolean question, JSONArray results) {
        if (question && results != null) {
            pending = results;
            askedAt = System.currentTimeMillis();
        } else {
            conversation = null;
            pending = null;
        }
        AgentLog.add(service, question ? "question" : "reply", reply, "shown");
        overlay.status(null, null);
        overlay.reply(reply, question, () -> Dictation.handsFree(service, Dictation.ASSISTANT));
        speak(reply);
    }

    private void speak(String reply) {
        if (!Core.settings().optBoolean("agent_speak", true) || !headset()) return;
        if (tts == null) tts = new TextToSpeech(service, ok -> {
            if (ok == TextToSpeech.SUCCESS) tts.speak(reply, TextToSpeech.QUEUE_FLUSH, null, "reply");
        });
        else tts.speak(reply, TextToSpeech.QUEUE_FLUSH, null, "reply");
    }

    private boolean headset() {
        for (AudioDeviceInfo d : service.getSystemService(AudioManager.class).getDevices(AudioManager.GET_DEVICES_OUTPUTS)) {
            if (AUDIO_OUT.contains(d.getType())) return true;
        }
        return false;
    }
}
