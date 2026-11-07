package com.slgotting.steezenstt;

import android.content.Context;
import android.graphics.PixelFormat;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.util.concurrent.CompletableFuture;

final class Overlay {
    private final Context c;
    private final WindowManager wm;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final float dp;
    private View pill, sheet;
    private TextView pillText;
    private final Runnable hideSheet = () -> setSheet(null);

    Overlay(Context c) {
        this.c = c;
        wm = c.getSystemService(WindowManager.class);
        dp = c.getResources().getDisplayMetrics().density;
    }

    private int px(float v) {
        return (int) (v * dp);
    }

    private GradientDrawable bg(int fill, int stroke, float radius) {
        GradientDrawable g = new GradientDrawable();
        g.setColor(c.getColor(fill));
        g.setStroke(px(1), c.getColor(stroke));
        g.setCornerRadius(radius);
        return g;
    }

    private TextView text(String s, float size, int color) {
        TextView t = new TextView(c);
        t.setText(s);
        t.setTextSize(size);
        t.setTextColor(c.getColor(color));
        return t;
    }

    private Button button(String label, int fill, View.OnClickListener click) {
        Button b = new Button(c);
        b.setText(label);
        b.setAllCaps(false);
        b.setTextColor(c.getColor(R.color.text));
        b.setBackground(bg(fill, fill == R.color.raised ? R.color.border : fill, px(8)));
        b.setOnClickListener(click);
        return b;
    }

    private WindowManager.LayoutParams params(int gravity, boolean full) {
        WindowManager.LayoutParams lp = new WindowManager.LayoutParams(full ? -1 : -2, full ? -1 : -2, WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
                WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN, PixelFormat.TRANSLUCENT);
        lp.gravity = gravity;
        return lp;
    }

    void status(String s, Runnable stop) {
        main.post(() -> {
            if (s == null) {
                if (pill != null) wm.removeView(pill);
                pill = null;
                return;
            }
            if (pill == null) {
                LinearLayout row = new LinearLayout(c);
                row.setGravity(Gravity.CENTER_VERTICAL);
                row.setPadding(px(14), px(4), px(4), px(4));
                row.setBackground(bg(R.color.surface, R.color.accent, px(100)));
                pillText = text("", 14, R.color.text);
                row.addView(pillText);
                Button b = button("■ Stop", R.color.red, v -> stop.run());
                b.setMinHeight(0);
                b.setMinimumHeight(0);
                b.setPadding(px(12), px(6), px(12), px(6));
                LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-2, -2);
                lp.leftMargin = px(10);
                row.addView(b, lp);
                WindowManager.LayoutParams wlp = params(Gravity.TOP | Gravity.CENTER_HORIZONTAL, false);
                wlp.y = px(40);
                pill = row;
                wm.addView(pill, wlp);
            }
            pillText.setText(s);
        });
    }

    private void setSheet(View v) {
        main.removeCallbacks(hideSheet);
        if (sheet != null) wm.removeView(sheet);
        sheet = v;
        if (v != null) wm.addView(v, params(Gravity.BOTTOM, true));
    }

    private FrameLayout backdrop(LinearLayout card, Runnable outside) {
        FrameLayout root = new FrameLayout(c) {
            @Override
            public boolean dispatchKeyEvent(KeyEvent e) {
                if (e.getKeyCode() == KeyEvent.KEYCODE_BACK && e.getAction() == KeyEvent.ACTION_UP) outside.run();
                return super.dispatchKeyEvent(e);
            }
        };
        root.setBackgroundColor(0x66000000);
        root.setOnClickListener(v -> outside.run());
        card.setClickable(true);
        FrameLayout.LayoutParams lp = new FrameLayout.LayoutParams(-1, -2, Gravity.BOTTOM);
        lp.setMargins(px(10), 0, px(10), px(24));
        root.addView(card, lp);
        return root;
    }

    private LinearLayout card(String heading, int headingColor, String body) {
        LinearLayout card = new LinearLayout(c);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(px(18), px(16), px(18), px(14));
        card.setBackground(bg(R.color.surface, R.color.border, px(16)));
        TextView h = text(heading, 13, headingColor);
        h.setTypeface(Typeface.DEFAULT_BOLD);
        card.addView(h);
        TextView t = text(body, 16, R.color.text);
        t.setPadding(0, px(6), 0, px(10));
        t.setTextIsSelectable(true);
        card.addView(t);
        return card;
    }

    private LinearLayout row(Button... buttons) {
        LinearLayout row = new LinearLayout(c);
        for (Button b : buttons) {
            LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(0, -2, 1);
            lp.setMargins(px(4), 0, px(4), 0);
            row.addView(b, lp);
        }
        return row;
    }

    CompletableFuture<Boolean> confirm(String title, String detail) {
        CompletableFuture<Boolean> answer = new CompletableFuture<>();
        main.post(() -> {
            Runnable no = () -> {
                answer.complete(false);
                setSheet(null);
            };
            LinearLayout card = card("Confirm: " + title, R.color.amber, detail);
            card.addView(row(button("Cancel", R.color.raised, v -> no.run()), button("Allow", R.color.accent, v -> {
                answer.complete(true);
                setSheet(null);
            })));
            setSheet(backdrop(card, no));
        });
        return answer;
    }

    void reply(String message, boolean question, Runnable answer) {
        main.post(() -> {
            LinearLayout card = card(question ? "Zen asks" : "Zen", R.color.accent_hi, message);
            Button close = button("Close", R.color.raised, v -> setSheet(null));
            card.addView(question ? row(close, button("Answer", R.color.accent, v -> {
                setSheet(null);
                answer.run();
            })) : row(close));
            setSheet(backdrop(card, () -> setSheet(null)));
            main.postDelayed(hideSheet, question ? 30_000 : 12_000);
        });
    }

    void clear() {
        main.post(() -> setSheet(null));
        status(null, null);
    }
}
