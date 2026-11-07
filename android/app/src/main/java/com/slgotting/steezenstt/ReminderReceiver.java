package com.slgotting.steezenstt;

import android.app.Notification;
import android.app.NotificationManager;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

public class ReminderReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context c, Intent i) {
        String text = i.getStringExtra("text");
        c.getSystemService(NotificationManager.class).notify((int) (System.currentTimeMillis() / 1000), new Notification.Builder(c, App.ASSISTANT)
                .setSmallIcon(R.drawable.ic_stat).setContentTitle("Reminder").setContentText(text).setStyle(new Notification.BigTextStyle().bigText(text)).setAutoCancel(true).build());
    }
}
