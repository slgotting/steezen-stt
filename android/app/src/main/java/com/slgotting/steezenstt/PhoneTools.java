package com.slgotting.steezenstt;

import android.app.AlarmManager;
import android.app.PendingIntent;
import android.content.ContentValues;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.database.Cursor;
import android.net.Uri;
import android.provider.AlarmClock;
import android.provider.CalendarContract;
import android.provider.ContactsContract;
import android.service.notification.StatusBarNotification;
import android.telephony.SmsManager;
import java.time.Instant;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import org.json.JSONArray;
import org.json.JSONObject;

final class PhoneTools {
    private static final Set<String> URL_SCHEMES = Set.of("https", "http", "geo", "mailto", "market");
    private static final DateTimeFormatter SHOWN = DateTimeFormatter.ofPattern("EEE d MMM HH:mm", Locale.getDefault());

    private final Context c;

    PhoneTools(Context c) {
        this.c = c;
    }

    private void start(Intent i) {
        c.startActivity(i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
    }

    private Map<String, String> apps() {
        Map<String, String> byLabel = new TreeMap<>(String.CASE_INSENSITIVE_ORDER);
        PackageManager pm = c.getPackageManager();
        for (ResolveInfo r : pm.queryIntentActivities(new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER), 0)) {
            byLabel.put(r.loadLabel(pm).toString(), r.activityInfo.packageName);
        }
        return byLabel;
    }

    String openApp(JSONObject in) throws Exception {
        String want = in.getString("name").trim().toLowerCase(Locale.ROOT);
        Map<String, String> apps = apps();
        List<String> starts = new ArrayList<>(), contains = new ArrayList<>();
        for (String label : apps.keySet()) {
            String l = label.toLowerCase(Locale.ROOT);
            if (l.equals(want)) return launch(label, apps.get(label));
            if (l.startsWith(want)) starts.add(label);
            else if (l.contains(want)) contains.add(label);
        }
        List<String> found = starts.isEmpty() ? contains : starts;
        if (found.size() == 1) return launch(found.get(0), apps.get(found.get(0)));
        if (found.isEmpty()) throw new Exception("No installed app is called " + in.getString("name") + ". Use list_apps to see the names.");
        return "Several apps match: " + String.join(", ", found) + ". Call open_app with the exact name.";
    }

    private String launch(String label, String pkg) throws Exception {
        if (pkg.equals(c.getPackageName())) throw new Exception("The assistant doesn't open its own settings.");
        Intent i = c.getPackageManager().getLaunchIntentForPackage(pkg);
        if (i == null) throw new Exception(label + " can't be opened");
        start(i);
        return "Opened " + label;
    }

    String listApps(JSONObject in) {
        String q = in.optString("query").toLowerCase(Locale.ROOT);
        List<String> out = new ArrayList<>();
        for (String label : apps().keySet()) if (label.toLowerCase(Locale.ROOT).contains(q) && out.size() < 120) out.add(label);
        return out.isEmpty() ? "No app names contain " + q : String.join(", ", out);
    }

    String openUrl(JSONObject in) throws Exception {
        Uri u = Uri.parse(in.getString("url").trim());
        if (u.getScheme() == null || !URL_SCHEMES.contains(u.getScheme().toLowerCase(Locale.ROOT))) throw new Exception("Only https, geo, mailto and market links can be opened.");
        start(new Intent(Intent.ACTION_VIEW, u));
        return "Opened";
    }

    String findContact(JSONObject in) throws Exception {
        Map<String, List<String>> people = new LinkedHashMap<>();
        Uri phones = Uri.withAppendedPath(ContactsContract.CommonDataKinds.Phone.CONTENT_FILTER_URI, Uri.encode(in.getString("name")));
        String[][] queries = {
                {phones.toString(), ContactsContract.CommonDataKinds.Phone.DISPLAY_NAME, ContactsContract.CommonDataKinds.Phone.NUMBER, ContactsContract.CommonDataKinds.Phone.TYPE, "phone"},
                {Uri.withAppendedPath(ContactsContract.CommonDataKinds.Email.CONTENT_FILTER_URI, Uri.encode(in.getString("name"))).toString(), ContactsContract.CommonDataKinds.Email.DISPLAY_NAME, ContactsContract.CommonDataKinds.Email.ADDRESS, ContactsContract.CommonDataKinds.Email.TYPE, "email"},
        };
        for (String[] q : queries) {
            try (Cursor cur = c.getContentResolver().query(Uri.parse(q[0]), new String[]{q[1], q[2], q[3]}, null, null, null)) {
                while (cur != null && cur.moveToNext() && people.size() < 10) {
                    String type = q[4].equals("phone") ? ContactsContract.CommonDataKinds.Phone.getTypeLabel(c.getResources(), cur.getInt(2), "").toString() : "email";
                    people.computeIfAbsent(cur.getString(0), k -> new ArrayList<>()).add(type + " " + cur.getString(1));
                }
            }
        }
        if (people.isEmpty()) return "No contact matches " + in.getString("name");
        StringBuilder s = new StringBuilder();
        people.forEach((name, ways) -> s.append(name).append(": ").append(String.join(", ", ways)).append('\n'));
        return s.toString();
    }

    String call(JSONObject in) throws Exception {
        start(new Intent(Intent.ACTION_CALL, Uri.fromParts("tel", in.getString("number"), null)));
        return "Calling";
    }

    String sms(JSONObject in) throws Exception {
        SmsManager sm = c.getSystemService(SmsManager.class);
        sm.sendMultipartTextMessage(in.getString("to"), null, sm.divideMessage(in.getString("body")), null, null);
        return "Sent";
    }

    String alarm(JSONObject in) throws Exception {
        Intent i = new Intent(AlarmClock.ACTION_SET_ALARM).putExtra(AlarmClock.EXTRA_HOUR, in.getInt("hour")).putExtra(AlarmClock.EXTRA_MINUTES, in.getInt("minute")).putExtra(AlarmClock.EXTRA_SKIP_UI, true);
        if (in.has("label")) i.putExtra(AlarmClock.EXTRA_MESSAGE, in.getString("label"));
        JSONArray days = in.optJSONArray("days");
        if (days != null && days.length() > 0) {
            ArrayList<Integer> d = new ArrayList<>();
            for (int k = 0; k < days.length(); k++) d.add(days.getInt(k));
            i.putExtra(AlarmClock.EXTRA_DAYS, d);
        }
        start(i);
        return String.format(Locale.ROOT, "Alarm set for %02d:%02d", in.getInt("hour"), in.getInt("minute"));
    }

    String timer(JSONObject in) throws Exception {
        Intent i = new Intent(AlarmClock.ACTION_SET_TIMER).putExtra(AlarmClock.EXTRA_LENGTH, in.getInt("seconds")).putExtra(AlarmClock.EXTRA_SKIP_UI, true);
        if (in.has("label")) i.putExtra(AlarmClock.EXTRA_MESSAGE, in.getString("label"));
        start(i);
        return "Timer started";
    }

    private static long millis(String local) {
        return LocalDateTime.parse(local.length() == 16 ? local : local.substring(0, 16)).atZone(ZoneId.systemDefault()).toInstant().toEpochMilli();
    }

    private static String shown(long ms) {
        return SHOWN.format(Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault()));
    }

    String reminder(JSONObject in) throws Exception {
        long at = in.has("at") ? millis(in.getString("at")) : System.currentTimeMillis() + in.optInt("minutes", 0) * 60_000L;
        if (at <= System.currentTimeMillis()) throw new Exception("That time has passed.");
        PendingIntent pi = PendingIntent.getBroadcast(c, (int) (at / 1000), new Intent(c, ReminderReceiver.class).putExtra("text", in.getString("text")), PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        AlarmManager am = c.getSystemService(AlarmManager.class);
        boolean exact = am.canScheduleExactAlarms();
        if (exact) am.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pi);
        else am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pi);
        return "Reminder set for " + shown(at) + (exact ? "" : " (may be a few minutes late: exact alarms are off for steezen-stt)");
    }

    String events(JSONObject in) {
        long now = System.currentTimeMillis(), end = now + Math.max(1, Math.min(in.optInt("days", 1), 31)) * 86_400_000L;
        Uri.Builder b = CalendarContract.Instances.CONTENT_URI.buildUpon();
        android.content.ContentUris.appendId(b, now);
        android.content.ContentUris.appendId(b, end);
        StringBuilder s = new StringBuilder();
        try (Cursor cur = c.getContentResolver().query(b.build(), new String[]{CalendarContract.Instances.TITLE, CalendarContract.Instances.BEGIN, CalendarContract.Instances.EVENT_LOCATION, CalendarContract.Instances.ALL_DAY}, null, null, CalendarContract.Instances.BEGIN)) {
            while (cur != null && cur.moveToNext() && s.length() < 6000) {
                s.append(cur.getInt(3) == 1 ? SHOWN.format(Instant.ofEpochMilli(cur.getLong(1)).atZone(ZoneId.of("UTC"))).replaceAll(" \\d\\d:\\d\\d$", " (all day)") : shown(cur.getLong(1))).append(" — ").append(cur.getString(0));
                if (cur.getString(2) != null && !cur.getString(2).isEmpty()) s.append(" @ ").append(cur.getString(2));
                s.append('\n');
            }
        }
        return s.length() == 0 ? "No events" : s.toString();
    }

    String addEvent(JSONObject in) throws Exception {
        long calendar = -1;
        try (Cursor cur = c.getContentResolver().query(CalendarContract.Calendars.CONTENT_URI, new String[]{CalendarContract.Calendars._ID},
                CalendarContract.Calendars.CALENDAR_ACCESS_LEVEL + ">=" + CalendarContract.Calendars.CAL_ACCESS_CONTRIBUTOR + " AND " + CalendarContract.Calendars.VISIBLE + "=1", null,
                CalendarContract.Calendars.IS_PRIMARY + " DESC")) {
            if (cur != null && cur.moveToFirst()) calendar = cur.getLong(0);
        }
        if (calendar < 0) throw new Exception("No writable calendar on this phone.");
        ContentValues v = new ContentValues();
        v.put(CalendarContract.Events.CALENDAR_ID, calendar);
        v.put(CalendarContract.Events.TITLE, in.getString("title"));
        v.put(CalendarContract.Events.DTSTART, millis(in.getString("start")));
        v.put(CalendarContract.Events.DTEND, millis(in.getString("end")));
        v.put(CalendarContract.Events.EVENT_TIMEZONE, ZoneId.systemDefault().getId());
        if (in.has("location")) v.put(CalendarContract.Events.EVENT_LOCATION, in.getString("location"));
        if (in.has("description")) v.put(CalendarContract.Events.DESCRIPTION, in.getString("description"));
        c.getContentResolver().insert(CalendarContract.Events.CONTENT_URI, v);
        return "Added " + in.getString("title") + " on " + shown(millis(in.getString("start")));
    }

    String notifications(JSONObject in) throws Exception {
        Notifs n = Notifs.instance;
        if (n == null) throw new Exception("Notification access is off. Turn it on in steezen-stt → Setup.");
        String app = in.optString("app").toLowerCase(Locale.ROOT);
        PackageManager pm = c.getPackageManager();
        StringBuilder s = new StringBuilder();
        for (StatusBarNotification sb : n.getActiveNotifications()) {
            if (sb.getPackageName().equals(c.getPackageName()) || s.length() > 6000) continue;
            String label;
            try {
                label = pm.getApplicationLabel(pm.getApplicationInfo(sb.getPackageName(), 0)).toString();
            } catch (Exception e) {
                label = sb.getPackageName();
            }
            if (!app.isEmpty() && !label.toLowerCase(Locale.ROOT).contains(app)) continue;
            CharSequence title = sb.getNotification().extras.getCharSequence(android.app.Notification.EXTRA_TITLE);
            CharSequence text = sb.getNotification().extras.getCharSequence(android.app.Notification.EXTRA_TEXT);
            s.append(label).append(": ").append(title == null ? "" : title).append(" — ").append(text == null ? "" : text).append('\n');
        }
        String out = s.length() == 0 ? "No notifications" : s.toString();
        return (String) Core.run("redact", new JSONObject().put("text", out), null);
    }

    String zen(JSONObject in) throws Exception {
        return ((JSONObject) Core.run("zen_text", new JSONObject().put("text", in.getString("text")), null)).optString("text", "Sent to Zen");
    }
}
