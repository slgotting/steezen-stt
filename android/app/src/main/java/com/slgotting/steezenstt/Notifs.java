package com.slgotting.steezenstt;

import android.service.notification.NotificationListenerService;

public class Notifs extends NotificationListenerService {
    static volatile Notifs instance;

    @Override
    public void onListenerConnected() {
        instance = this;
    }

    @Override
    public void onListenerDisconnected() {
        instance = null;
    }
}
