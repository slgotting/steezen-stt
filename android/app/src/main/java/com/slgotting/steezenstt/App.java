package com.slgotting.steezenstt;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import com.slgotting.steezen.common.BaseApp;
import com.slgotting.steezen.common.Core;
import com.slgotting.steezen.common.Dictation;
import org.json.JSONObject;

public class App extends BaseApp {
    static final String ASSISTANT_APP = "com.slgotting.zenassistant";

    static boolean assistantInstalled(Context c) {
        try {
            c.getPackageManager().getPackageInfo(ASSISTANT_APP, 0);
            return true;
        } catch (Exception e) {
            return false;
        }
    }

    static boolean handOff(Context c) {
        if (!assistantInstalled(c)) return false;
        try {
            c.startActivity(new Intent().setComponent(new ComponentName(ASSISTANT_APP, "com.slgotting.steezen.common.VoiceLaunchActivity"))
                    .putExtra("action", Dictation.ASSISTANT).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            return true;
        } catch (Exception e) {
            Dictation.toast(c, "Couldn't open Zen Assistant: " + e.getMessage());
            return false;
        }
    }

    @Override
    protected Dictation.Actions actions() {
        return new Dictation.Actions() {
            @Override
            public String problem(Context c, String act) {
                if (!Dictation.ASSISTANT.equals(act)) return null;
                return handOff(c) ? "" : "Install Zen Assistant to use the assistant.";
            }

            @Override
            public void done(Context c, String act, boolean deliver, String text) {
                JSONObject s = Core.settings();
                boolean clip = s.optBoolean("output_clipboard", true);
                if (clip) c.getSystemService(ClipboardManager.class).setPrimaryClip(ClipData.newPlainText("steezen-stt", text));
                String how = clip ? "Copied" : "Done";
                if (deliver && s.optBoolean("output_cursor", true)) {
                    how = KeyService.insert(text, clip, s.optBoolean("output_enter")) ? (s.optBoolean("output_enter") ? "Pasted + Enter" : "Pasted") : (clip ? "Copied (no text box to paste into)" : "No text box to paste into");
                }
                if (s.optBoolean("notify_complete") || !how.startsWith("Pasted")) Dictation.toast(c, how + ": " + text);
            }
        };
    }
}
