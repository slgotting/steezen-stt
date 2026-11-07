package com.slgotting.steezenstt;

import android.content.Context;
import android.graphics.drawable.GradientDrawable;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.service.voice.VoiceInteractionSession;

public class AssistSession extends VoiceInteractionSession implements Dictation.Listener {
    private static final long SILENCE_MS = 1800;
    private static final long MAX_MS = 120_000;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private TextView status;
    private boolean mine;
    private final Runnable watch = new Runnable() {
        @Override
        public void run() {
            if (!mine || !Dictation.recording()) return;
            if (Dictation.silentMs() > SILENCE_MS || Dictation.elapsedMs() > MAX_MS) send();
            else handler.postDelayed(this, 200);
        }
    };

    AssistSession(Context c) {
        super(c);
    }

    @Override
    public View onCreateContentView() {
        Context c = getContext();
        float d = c.getResources().getDisplayMetrics().density;
        LinearLayout panel = new LinearLayout(c);
        panel.setOrientation(LinearLayout.VERTICAL);
        panel.setGravity(Gravity.CENTER_HORIZONTAL);
        panel.setPadding((int) (20 * d), (int) (20 * d), (int) (20 * d), (int) (28 * d));
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(c.getColor(R.color.surface));
        bg.setStroke((int) d, c.getColor(R.color.border));
        bg.setCornerRadii(new float[]{16 * d, 16 * d, 16 * d, 16 * d, 0, 0, 0, 0});
        panel.setBackground(bg);
        status = new TextView(c);
        status.setTextColor(c.getColor(R.color.text));
        status.setTextSize(18);
        status.setGravity(Gravity.CENTER);
        panel.addView(status);
        LinearLayout row = new LinearLayout(c);
        row.setGravity(Gravity.CENTER);
        row.setPadding(0, (int) (16 * d), 0, 0);
        row.addView(button(c, "Cancel", R.color.raised, v -> {
            mine = false;
            Dictation.cancel(c);
            hide();
        }));
        row.addView(button(c, "Send", R.color.accent, v -> send()));
        panel.addView(row);
        LinearLayout root = new LinearLayout(c);
        root.setGravity(Gravity.BOTTOM);
        root.addView(panel, new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        root.setOnClickListener(v -> {
            if (mine) Dictation.cancel(c);
            mine = false;
            hide();
        });
        return root;
    }

    private static Button button(Context c, String text, int color, View.OnClickListener click) {
        Button b = new Button(c);
        b.setText(text);
        b.setAllCaps(false);
        b.setTextColor(c.getColor(R.color.text));
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(c.getColor(color));
        bg.setCornerRadius(8 * c.getResources().getDisplayMetrics().density);
        b.setBackground(bg);
        b.setOnClickListener(click);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1);
        lp.setMargins(12, 0, 12, 0);
        b.setLayoutParams(lp);
        return b;
    }

    private void send() {
        if (!mine) return;
        mine = false;
        handler.removeCallbacks(watch);
        Dictation.stop(getContext());
        hide();
    }

    @Override
    public void onShow(Bundle args, int flags) {
        super.onShow(args, flags);
        Dictation.listeners.add(this);
        String action = args != null && args.getString("action") != null ? args.getString("action") : Core.settings().optString("assistant_action", Dictation.ZEN);
        if ("Off".equals(action)) {
            status.setText("The assistant gesture is off. Turn it on in steezen-stt → Gestures.");
            return;
        }
        String problem = Dictation.ASSISTANT.equals(action) ? AgentBridge.problem(getContext()) : null;
        if (problem != null) {
            status.setText(problem);
            return;
        }
        mine = Dictation.start(getContext(), action, true);
        if (mine) handler.postDelayed(watch, 200);
        else if (!Dictation.recording()) status.setText("Couldn't start the microphone.");
    }

    @Override
    public void onHide() {
        if (mine) Dictation.cancel(getContext());
        mine = false;
        handler.removeCallbacks(watch);
        Dictation.listeners.remove(this);
        super.onHide();
    }

    @Override
    public void state(String phase, String detail) {
        if (status != null && "recording".equals(phase)) status.setText(detail + "\nStops after a pause, or tap Send.");
    }
}
