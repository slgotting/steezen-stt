package com.slgotting.steezenstt;

import android.accessibilityservice.AccessibilityService;
import android.graphics.PixelFormat;
import android.graphics.drawable.GradientDrawable;
import android.media.AudioManager;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.WindowManager;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import android.widget.TextView;

public class KeyService extends AccessibilityService implements Dictation.Listener {
    private static final long HOLD_MS = 350;
    private static KeyService instance;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private boolean started;
    private String action = "Off";
    private TextView pill;
    private final Runnable begin = () -> started = Dictation.start(this, action, true);
    private final Runnable hide = () -> showPill(null, 0);

    static boolean enabled() {
        return instance != null;
    }

    @Override
    protected void onServiceConnected() {
        instance = this;
        Dictation.listeners.add(this);
    }

    @Override
    public boolean onUnbind(android.content.Intent intent) {
        Dictation.listeners.remove(this);
        instance = null;
        return super.onUnbind(intent);
    }

    @Override
    public void onAccessibilityEvent(AccessibilityEvent event) {
    }

    @Override
    public void onInterrupt() {
    }

    @Override
    protected boolean onKeyEvent(KeyEvent e) {
        if (e.getKeyCode() != KeyEvent.KEYCODE_VOLUME_DOWN) return false;
        if (e.getAction() == KeyEvent.ACTION_DOWN) {
            if (e.getRepeatCount() > 0) return true;
            action = Core.settings().optString("volume_action", "Dictate");
            if ("Off".equals(action) && !started) return false;
            handler.postDelayed(begin, HOLD_MS);
            return true;
        }
        if ("Off".equals(action) && !started) return false;
        handler.removeCallbacks(begin);
        if (started) {
            started = false;
            Dictation.stop(this);
        } else {
            getSystemService(AudioManager.class).adjustSuggestedStreamVolume(AudioManager.ADJUST_LOWER, AudioManager.USE_DEFAULT_STREAM_TYPE, AudioManager.FLAG_SHOW_UI);
        }
        return true;
    }

    static boolean insert(String text, boolean viaClipboard, boolean enter) {
        KeyService s = instance;
        if (s == null) return false;
        AccessibilityNodeInfo node = s.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
        if (node == null || !node.isEditable()) return false;
        boolean ok = viaClipboard && node.performAction(AccessibilityNodeInfo.ACTION_PASTE);
        if (!ok) ok = setText(node, text);
        if (!ok && !viaClipboard) ok = node.performAction(AccessibilityNodeInfo.ACTION_PASTE);
        if (ok && enter) s.handler.postDelayed(() -> pressEnter(s), 150);
        return ok;
    }

    private static boolean setText(AccessibilityNodeInfo node, String text) {
        CharSequence current = node.isShowingHintText() || node.getText() == null ? "" : node.getText();
        int start = Math.max(0, Math.min(node.getTextSelectionStart(), current.length()));
        int end = Math.max(start, Math.min(node.getTextSelectionEnd(), current.length()));
        if (node.getTextSelectionStart() < 0) start = end = current.length();
        String next = current.subSequence(0, start) + text + current.subSequence(end, current.length());
        Bundle args = new Bundle();
        args.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, next);
        if (!node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) return false;
        Bundle sel = new Bundle();
        sel.putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, start + text.length());
        sel.putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, start + text.length());
        node.performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, sel);
        return true;
    }

    private static void pressEnter(KeyService s) {
        AccessibilityNodeInfo node = s.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
        if (node != null) node.performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.getId());
    }

    @Override
    public void state(String phase, String detail) {
        handler.removeCallbacks(hide);
        switch (phase) {
            case "recording" -> showPill("● " + detail, getColor(R.color.red));
            case "working" -> showPill(detail, getColor(R.color.accent));
            case "error" -> {
                showPill(detail, getColor(R.color.red));
                handler.postDelayed(hide, 3000);
            }
            default -> showPill(null, 0);
        }
    }

    private void showPill(String text, int color) {
        WindowManager wm = getSystemService(WindowManager.class);
        if (text == null) {
            if (pill != null) wm.removeView(pill);
            pill = null;
            return;
        }
        boolean add = pill == null;
        if (add) {
            pill = new TextView(this);
            pill.setTextColor(getColor(R.color.text));
            pill.setTextSize(14);
            int pad = (int) (12 * getResources().getDisplayMetrics().density);
            pill.setPadding(pad, pad / 2, pad, pad / 2);
        }
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(getColor(R.color.surface));
        bg.setStroke(3, color);
        bg.setCornerRadius(100);
        pill.setBackground(bg);
        pill.setText(text);
        if (!add) return;
        WindowManager.LayoutParams lp = new WindowManager.LayoutParams(WindowManager.LayoutParams.WRAP_CONTENT, WindowManager.LayoutParams.WRAP_CONTENT,
                WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY, WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE | WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE, PixelFormat.TRANSLUCENT);
        lp.gravity = Gravity.TOP | Gravity.CENTER_HORIZONTAL;
        lp.y = (int) (48 * getResources().getDisplayMetrics().density);
        wm.addView(pill, lp);
    }
}
