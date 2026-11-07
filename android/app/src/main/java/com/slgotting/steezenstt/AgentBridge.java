package com.slgotting.steezenstt;

import android.app.KeyguardManager;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.provider.Settings;

final class AgentBridge {
    static final String REQUEST = "com.slgotting.steezenstt.AGENT_REQUEST";
    static final String STOP = "com.slgotting.steezenstt.AGENT_STOP";

    static boolean enabled(Context c, Class<?> service) {
        String on = Settings.Secure.getString(c.getContentResolver(), Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES);
        return on != null && on.contains(new ComponentName(c, service).flattenToString());
    }

    static boolean notificationsEnabled(Context c) {
        String on = Settings.Secure.getString(c.getContentResolver(), "enabled_notification_listeners");
        return on != null && on.contains(new ComponentName(c, Notifs.class).flattenToString());
    }

    static boolean locked(Context c) {
        return c.getSystemService(KeyguardManager.class).isKeyguardLocked();
    }

    static String problem(Context c) {
        if (locked(c)) return "Unlock your phone first. The assistant never acts on the lock screen.";
        if (!enabled(c, AgentService.class)) return "Turn on Zen phone control in steezen-stt → Setup to use the assistant.";
        if (Core.settings().optString("anthropic_api_key").trim().isEmpty()) return "Add your Anthropic API key in steezen-stt → Assistant.";
        return null;
    }

    static void send(Context c, String text) {
        c.sendBroadcast(new Intent(REQUEST).setPackage(c.getPackageName()).putExtra("text", text));
    }

    static void stop(Context c) {
        c.sendBroadcast(new Intent(STOP).setPackage(c.getPackageName()));
    }
}
