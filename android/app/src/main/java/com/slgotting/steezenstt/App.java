package com.slgotting.steezenstt;

import android.app.Application;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import org.json.JSONObject;

public class App extends Application {
    static final String CHANNEL = "status";

    @Override
    public void onCreate() {
        super.onCreate();
        try {
            Core.run("init", new JSONObject().put("dir", getFilesDir().getAbsolutePath()), null);
        } catch (Exception ignored) {
        }
        getSystemService(NotificationManager.class).createNotificationChannel(new NotificationChannel(CHANNEL, "Recording", NotificationManager.IMPORTANCE_LOW));
    }
}
