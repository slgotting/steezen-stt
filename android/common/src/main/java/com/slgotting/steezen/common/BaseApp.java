package com.slgotting.steezen.common;

import android.app.Application;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.content.Context;
import org.json.JSONObject;

public abstract class BaseApp extends Application {
    public static final String CHANNEL = "status";
    public static final String ASSISTANT = "assistant";

    protected abstract Dictation.Actions actions();

    @Override
    public void onCreate() {
        super.onCreate();
        try {
            Core.run("init", new JSONObject().put("dir", getFilesDir().getAbsolutePath()), null);
        } catch (Exception ignored) {
        }
        Dictation.actions = actions();
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel(CHANNEL, "Recording", NotificationManager.IMPORTANCE_LOW));
        nm.createNotificationChannel(new NotificationChannel(ASSISTANT, "Assistant and reminders", NotificationManager.IMPORTANCE_DEFAULT));
    }

    public static String label(Context c) {
        return c.getApplicationInfo().loadLabel(c.getPackageManager()).toString();
    }
}
