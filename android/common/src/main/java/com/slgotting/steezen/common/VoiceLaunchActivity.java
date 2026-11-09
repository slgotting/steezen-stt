package com.slgotting.steezen.common;

import android.app.Activity;
import android.content.Context;
import android.os.Bundle;

public class VoiceLaunchActivity extends Activity {
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        trigger(this, getIntent().getStringExtra("action"));
        finish();
    }

    public static void trigger(Context c) {
        trigger(c, null);
    }

    public static void trigger(Context c, String wanted) {
        String action = wanted != null ? wanted : Core.settings().optString("earbud_action", Dictation.ZEN);
        if ("Off".equals(action)) return;
        AssistService assist = AssistService.instance;
        if (assist != null) {
            Bundle args = new Bundle();
            args.putString("action", action);
            assist.showSession(args, 0);
        } else {
            Dictation.handsFree(c, action, true);
        }
    }
}
