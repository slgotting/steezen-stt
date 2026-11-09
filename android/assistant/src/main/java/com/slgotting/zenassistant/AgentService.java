package com.slgotting.zenassistant;

import android.accessibilityservice.AccessibilityService;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.view.accessibility.AccessibilityEvent;
import com.slgotting.steezen.common.EarbudSession;
import com.slgotting.steezen.common.Dictation;

public class AgentService extends AccessibilityService implements Dictation.Listener {
    private Agent agent;
    private EarbudSession earbuds;
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
                case AgentBridge.SYNC -> syncEarbuds();
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
        f.addAction(AgentBridge.SYNC);
        f.addAction(Intent.ACTION_SCREEN_OFF);
        registerReceiver(receiver, f, Context.RECEIVER_NOT_EXPORTED);
        Dictation.listeners.add(this);
        earbuds = new EarbudSession(this);
        syncEarbuds();
    }

    private void syncEarbuds() {
        if (earbuds == null) return;
        try {
            earbuds.sync();
        } catch (Exception e) {
            Dictation.toast(this, "Earbud button fallback failed: " + e.getMessage());
        }
    }

    @Override
    public void state(String phase, String detail) {
        if (agent != null) agent.listening(phase, detail);
    }

    @Override
    public boolean onUnbind(Intent intent) {
        try {
            unregisterReceiver(receiver);
        } catch (Exception ignored) {
        }
        Dictation.listeners.remove(this);
        if (earbuds != null) earbuds.release();
        earbuds = null;
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
