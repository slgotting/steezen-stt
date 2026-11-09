package com.slgotting.zenassistant;

import android.content.Context;
import android.content.SharedPreferences;
import com.slgotting.steezen.common.BaseApp;
import com.slgotting.steezen.common.Core;
import com.slgotting.steezen.common.Dictation;
import org.json.JSONObject;

public class App extends BaseApp {
    private static final String[] GESTURES = {"assistant_action", "earbud_action"};

    @Override
    public void onCreate() {
        super.onCreate();
        SharedPreferences prefs = getSharedPreferences("ui", MODE_PRIVATE);
        if (prefs.getBoolean("defaults_set", false)) return;
        try {
            JSONObject s = Core.settings();
            for (String k : GESTURES) s.put(k, Dictation.ASSISTANT);
            Core.save(s);
            prefs.edit().putBoolean("defaults_set", true).apply();
        } catch (Exception ignored) {
        }
    }

    @Override
    protected Dictation.Actions actions() {
        return new Dictation.Actions() {
            @Override
            public String problem(Context c, String act) {
                return switch (act) {
                    case Dictation.ASSISTANT -> AgentBridge.problem(c);
                    case Dictation.ZEN -> null;
                    default -> "Dictation lives in steezen-stt. Zen Assistant only takes requests.";
                };
            }

            @Override
            public void done(Context c, String act, boolean deliver, String text) {
                if (Dictation.ASSISTANT.equals(act)) AgentBridge.send(c, text);
            }
        };
    }
}
