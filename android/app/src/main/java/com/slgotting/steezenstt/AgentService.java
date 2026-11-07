package com.slgotting.steezenstt;

import android.accessibilityservice.AccessibilityService;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.view.accessibility.AccessibilityEvent;

public class AgentService extends AccessibilityService {
    private Agent agent;
    private final BroadcastReceiver receiver = new BroadcastReceiver() {
        @Override
        public void onReceive(Context c, Intent i) {
            if (agent == null) return;
            switch (String.valueOf(i.getAction())) {
                case AgentBridge.REQUEST -> {
                    String problem = AgentBridge.locked(c) ? "Unlock your phone first. The assistant never acts on the lock screen." : null;
                    String text = i.getStringExtra("text");
                    if (problem != null || text == null || text.isBlank()) Dictation.toast(c, problem != null ? problem : "Nothing heard");
                    else agent.request(text);
                }
                case AgentBridge.STOP -> agent.stop("You tapped Stop");
                case Intent.ACTION_SCREEN_OFF -> agent.stop("The screen turned off");
                default -> {
                }
            }
        }
    };

    @Override
    protected void onServiceConnected() {
        try {
            agent = new Agent(this);
        } catch (Throwable e) {
            agent = null;
            AgentLog.add(this, "error", "Couldn't start: " + e, "crashed");
            return;
        }
        IntentFilter f = new IntentFilter();
        f.addAction(AgentBridge.REQUEST);
        f.addAction(AgentBridge.STOP);
        f.addAction(Intent.ACTION_SCREEN_OFF);
        registerReceiver(receiver, f, Context.RECEIVER_NOT_EXPORTED);
    }

    @Override
    public boolean onUnbind(Intent intent) {
        try {
            unregisterReceiver(receiver);
        } catch (Exception ignored) {
        }
        if (agent != null) agent.shutdown();
        agent = null;
        return super.onUnbind(intent);
    }

    @Override
    public void onAccessibilityEvent(AccessibilityEvent event) {
    }

    @Override
    public void onInterrupt() {
        if (agent != null) agent.stop("Android interrupted the assistant");
    }
}
