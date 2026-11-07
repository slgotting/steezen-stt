package com.slgotting.steezenstt;

import android.app.Application;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import org.json.JSONObject;

public class App extends Application {
    static final String CHANNEL = "status";
    static final String ASSISTANT = "assistant";

    @Override
    public void onCreate() {
        super.onCreate();
        try {
            Core.run("init", new JSONObject().put("dir", getFilesDir().getAbsolutePath()), null);
        } catch (Exception ignored) {
        }
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel(CHANNEL, "Recording", NotificationManager.IMPORTANCE_LOW));
        nm.createNotificationChannel(new NotificationChannel(ASSISTANT, "Assistant and reminders", NotificationManager.IMPORTANCE_DEFAULT));
    }
}
