package com.slgotting.zenassistant;

import android.accessibilityservice.AccessibilityService;
import android.accessibilityservice.GestureDescription;
import android.graphics.Bitmap;
import android.graphics.Path;
import android.graphics.Rect;
import android.os.Bundle;
import android.util.Base64;
import android.view.Display;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import android.view.accessibility.AccessibilityWindowInfo;
import java.io.ByteArrayOutputStream;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import org.json.JSONObject;
import com.slgotting.steezen.common.Core;

final class Screen {
    private static final int MAX_NODES = 250;
    private static final int MAX_TEXT = 120;
    private static final int SHOT_EDGE = 1024;
    private static final Map<String, Integer> GLOBAL = Map.of(
            "back", AccessibilityService.GLOBAL_ACTION_BACK,
            "home", AccessibilityService.GLOBAL_ACTION_HOME,
            "recents", AccessibilityService.GLOBAL_ACTION_RECENTS,
            "notifications", AccessibilityService.GLOBAL_ACTION_NOTIFICATIONS,
            "quick_settings", AccessibilityService.GLOBAL_ACTION_QUICK_SETTINGS);
    private static final Map<String, AccessibilityAction> SCROLL = Map.of(
            "up", AccessibilityAction.ACTION_SCROLL_UP,
            "down", AccessibilityAction.ACTION_SCROLL_DOWN,
            "left", AccessibilityAction.ACTION_SCROLL_LEFT,
            "right", AccessibilityAction.ACTION_SCROLL_RIGHT);

    private final AccessibilityService service;
    private final List<AccessibilityNodeInfo> nodes = new ArrayList<>();
    String foreground = "";

    Screen(AccessibilityService service) {
        this.service = service;
    }

    String read() {
        nodes.clear();
        StringBuilder out = new StringBuilder();
        AccessibilityNodeInfo active = service.getRootInActiveWindow();
        foreground = active == null || active.getPackageName() == null ? "" : active.getPackageName().toString();
        out.append("Foreground app: ").append(label(foreground)).append(" (").append(foreground).append(")\n");
        Rect size = new Rect();
        for (AccessibilityWindowInfo w : service.getWindows()) {
            if (w.getType() == AccessibilityWindowInfo.TYPE_ACCESSIBILITY_OVERLAY) continue;
            AccessibilityNodeInfo root = w.getRoot();
            if (root == null || service.getPackageName().contentEquals(root.getPackageName() == null ? "" : root.getPackageName())) continue;
            w.getBoundsInScreen(size);
            out.append("Window: ").append(w.getTitle() == null ? kind(w.getType()) : w.getTitle()).append(w.isActive() ? " (active)" : "").append('\n');
            walk(root, out, 1);
        }
        if (nodes.size() >= MAX_NODES) out.append("(list cut at ").append(MAX_NODES).append(" elements; scroll or open a narrower screen)\n");
        try {
            return (String) Core.run("redact", new JSONObject().put("text", out.toString()), null);
        } catch (Exception e) {
            return out.toString();
        }
    }

    private static String kind(int type) {
        return switch (type) {
            case AccessibilityWindowInfo.TYPE_INPUT_METHOD -> "keyboard";
            case AccessibilityWindowInfo.TYPE_SYSTEM -> "system";
            default -> "app";
        };
    }

    private String label(String pkg) {
        try {
            return service.getPackageManager().getApplicationLabel(service.getPackageManager().getApplicationInfo(pkg, 0)).toString();
        } catch (Exception e) {
            return pkg;
        }
    }

    private void walk(AccessibilityNodeInfo n, StringBuilder out, int depth) {
        if (n == null || nodes.size() >= MAX_NODES || !n.isVisibleToUser()) return;
        String text = n.isPassword() ? "(password field, hidden)" : clip(n.getText());
        String desc = clip(n.getContentDescription());
        boolean useful = !text.isEmpty() || !desc.isEmpty() || n.isClickable() || n.isEditable() || n.isScrollable() || n.isCheckable();
        if (useful) {
            int id = nodes.size();
            nodes.add(n);
            Rect r = new Rect();
            n.getBoundsInScreen(r);
            out.append("  ".repeat(Math.min(depth, 6))).append('[').append(id).append("] ").append(role(n));
            if (!text.isEmpty()) out.append(" \"").append(text).append('"');
            if (!desc.isEmpty() && !desc.equals(text)) out.append(" desc=\"").append(desc).append('"');
            if (n.isShowingHintText() && n.getHintText() != null) out.append(" hint=\"").append(clip(n.getHintText())).append('"');
            if (n.getViewIdResourceName() != null) out.append(" id=").append(n.getViewIdResourceName().replaceFirst("^.*:id/", ""));
            List<String> flags = new ArrayList<>();
            if (n.isClickable()) flags.add("clickable");
            if (n.isEditable()) flags.add("editable");
            if (n.isScrollable()) flags.add("scrollable");
            if (n.isCheckable()) flags.add(n.isChecked() ? "checked" : "unchecked");
            if (n.isFocused()) flags.add("focused");
            if (!n.isEnabled()) flags.add("disabled");
            if (!flags.isEmpty()) out.append(" (").append(String.join(", ", flags)).append(')');
            out.append(" @").append(r.centerX()).append(',').append(r.centerY()).append('\n');
        }
        for (int i = 0; i < n.getChildCount(); i++) walk(n.getChild(i), out, useful ? depth + 1 : depth);
    }

    private static String role(AccessibilityNodeInfo n) {
        String c = n.getClassName() == null ? "View" : n.getClassName().toString();
        return c.substring(c.lastIndexOf('.') + 1);
    }

    private static String clip(CharSequence s) {
        if (s == null) return "";
        String t = s.toString().replaceAll("\\s+", " ").trim();
        return t.length() > MAX_TEXT ? t.substring(0, MAX_TEXT) + "…" : t;
    }

    AccessibilityNodeInfo node(JSONObject in) throws Exception {
        if (!in.has("node")) return null;
        int id = in.getInt("node");
        if (id < 0 || id >= nodes.size()) throw new Exception("No node " + id + " in the latest read_screen; read the screen again.");
        AccessibilityNodeInfo n = nodes.get(id);
        if (!n.refresh()) throw new Exception("That element is gone; read the screen again.");
        return n;
    }

    AccessibilityNodeInfo at(int x, int y) {
        AccessibilityNodeInfo best = null;
        long area = Long.MAX_VALUE;
        Rect r = new Rect();
        for (AccessibilityNodeInfo n : nodes) {
            n.getBoundsInScreen(r);
            long a = (long) r.width() * r.height();
            if (r.contains(x, y) && a < area) {
                best = n;
                area = a;
            }
        }
        return best;
    }

    AccessibilityNodeInfo target(JSONObject in) throws Exception {
        AccessibilityNodeInfo n = node(in);
        return n != null ? n : in.has("x") ? at(in.getInt("x"), in.getInt("y")) : null;
    }

    static String describe(AccessibilityNodeInfo n) {
        if (n == null) return "";
        StringBuilder s = new StringBuilder();
        for (AccessibilityNodeInfo p = n; p != null && s.length() < 300; p = p.getParent()) {
            s.append(clip(p.getText())).append(' ').append(clip(p.getContentDescription())).append(' ');
            if (p.getViewIdResourceName() != null) s.append(p.getViewIdResourceName().replaceFirst("^.*:id/", "").replace('_', ' ')).append(' ');
            if (p.isClickable()) break;
        }
        return s.toString().trim();
    }

    static String packageOf(AccessibilityNodeInfo n, String fallback) {
        return n == null || n.getPackageName() == null ? fallback : n.getPackageName().toString();
    }

    String tap(JSONObject in, boolean longPress) throws Exception {
        AccessibilityNodeInfo n = node(in);
        if (n != null) {
            int action = longPress ? AccessibilityNodeInfo.ACTION_LONG_CLICK : AccessibilityNodeInfo.ACTION_CLICK;
            for (AccessibilityNodeInfo p = n; p != null; p = p.getParent()) {
                if ((longPress ? p.isLongClickable() : p.isClickable()) && p.performAction(action)) return "done";
            }
            Rect r = new Rect();
            n.getBoundsInScreen(r);
            return gesture(r.centerX(), r.centerY(), r.centerX(), r.centerY(), longPress ? 700 : 60);
        }
        if (!in.has("x")) throw new Exception("Give a node id or x and y.");
        return gesture(in.getInt("x"), in.getInt("y"), in.getInt("x"), in.getInt("y"), longPress ? 700 : 60);
    }

    String type(JSONObject in) throws Exception {
        AccessibilityNodeInfo n = node(in);
        if (n == null) n = service.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
        if (n == null || !n.isEditable()) throw new Exception("That element is not a text box; tap one first or give its node id.");
        if (n.isPassword()) throw new Exception("Passwords are typed by the owner, never by the assistant.");
        Bundle args = new Bundle();
        args.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, in.getString("text"));
        if (!n.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) throw new Exception("The text box refused the text.");
        if (in.optBoolean("submit") && !n.performAction(AccessibilityAction.ACTION_IME_ENTER.getId())) return "typed, but the box has no Enter action; tap its send button";
        return "done";
    }

    String scroll(JSONObject in) throws Exception {
        String dir = in.getString("direction");
        AccessibilityNodeInfo n = node(in);
        if (n == null) for (AccessibilityNodeInfo c : nodes) if (c.isScrollable()) {
            n = c;
            break;
        }
        AccessibilityAction a = SCROLL.get(dir);
        if (a == null) throw new Exception("Unknown direction " + dir);
        if (n != null) {
            boolean forward = dir.equals("down") || dir.equals("right");
            if (n.performAction(a.getId()) || n.performAction(forward ? AccessibilityNodeInfo.ACTION_SCROLL_FORWARD : AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD)) return "done";
        }
        int w = service.getResources().getDisplayMetrics().widthPixels, h = service.getResources().getDisplayMetrics().heightPixels;
        int dx = dir.equals("left") ? -w / 3 : dir.equals("right") ? w / 3 : 0, dy = dir.equals("up") ? -h / 3 : dir.equals("down") ? h / 3 : 0;
        return gesture(w / 2 + dx, h / 2 + dy, w / 2 - dx, h / 2 - dy, 350);
    }

    String swipe(JSONObject in) throws Exception {
        return gesture(in.getInt("x1"), in.getInt("y1"), in.getInt("x2"), in.getInt("y2"), Math.max(80, Math.min(in.optInt("ms", 300), 3000)));
    }

    String global(JSONObject in) throws Exception {
        Integer a = GLOBAL.get(in.getString("action"));
        if (a == null) throw new Exception("Unknown action");
        return service.performGlobalAction(a) ? "done" : "the system refused";
    }

    private String gesture(int x1, int y1, int x2, int y2, long ms) throws Exception {
        Path p = new Path();
        p.moveTo(x1, y1);
        if (x1 != x2 || y1 != y2) p.lineTo(x2, y2);
        CompletableFuture<Boolean> done = new CompletableFuture<>();
        boolean sent = service.dispatchGesture(new GestureDescription.Builder().addStroke(new GestureDescription.StrokeDescription(p, 0, ms)).build(), new AccessibilityService.GestureResultCallback() {
            @Override
            public void onCompleted(GestureDescription g) {
                done.complete(true);
            }

            @Override
            public void onCancelled(GestureDescription g) {
                done.complete(false);
            }
        }, null);
        if (!sent || !done.get(ms + 3000, TimeUnit.MILLISECONDS)) throw new Exception("The gesture was cancelled");
        return "done";
    }

    String screenshot() throws Exception {
        CompletableFuture<Bitmap> shot = new CompletableFuture<>();
        service.takeScreenshot(Display.DEFAULT_DISPLAY, service.getMainExecutor(), new AccessibilityService.TakeScreenshotCallback() {
            @Override
            public void onSuccess(AccessibilityService.ScreenshotResult r) {
                Bitmap hw = Bitmap.wrapHardwareBuffer(r.getHardwareBuffer(), r.getColorSpace());
                shot.complete(hw == null ? null : hw.copy(Bitmap.Config.ARGB_8888, false));
                r.getHardwareBuffer().close();
            }

            @Override
            public void onFailure(int code) {
                shot.completeExceptionally(new Exception(code == AccessibilityService.ERROR_TAKE_SCREENSHOT_SECURE_WINDOW ? "This screen blocks screenshots." : "Screenshot failed (" + code + ")"));
            }
        });
        Bitmap b = shot.get(5, TimeUnit.SECONDS);
        if (b == null) throw new Exception("Screenshot failed");
        float scale = Math.min(1f, SHOT_EDGE / (float) Math.max(b.getWidth(), b.getHeight()));
        Bitmap small = Bitmap.createScaledBitmap(b, Math.round(b.getWidth() * scale), Math.round(b.getHeight() * scale), true);
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        small.compress(Bitmap.CompressFormat.JPEG, 70, out);
        return Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP);
    }

    float shotScale() {
        int w = service.getResources().getDisplayMetrics().widthPixels, h = service.getResources().getDisplayMetrics().heightPixels;
        return Math.min(1f, SHOT_EDGE / (float) Math.max(w, h));
    }
}
