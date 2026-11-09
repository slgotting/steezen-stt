package com.slgotting.steezen.common;

import android.service.notification.NotificationListenerService;

public class Notifs extends NotificationListenerService {
    public static volatile Notifs instance;

    @Override
    public void onListenerConnected() {
        instance = this;
    }

    @Override
    public void onListenerDisconnected() {
        instance = null;
    }
}
