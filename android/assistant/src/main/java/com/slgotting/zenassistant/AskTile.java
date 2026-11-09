package com.slgotting.zenassistant;

import android.app.PendingIntent;
import android.content.Intent;
import android.os.Build;
import android.service.quicksettings.TileService;
import com.slgotting.steezen.common.Dictation;
import com.slgotting.steezen.common.VoiceLaunchActivity;

public class AskTile extends TileService {
    @Override
    @SuppressWarnings({"deprecation", "StartActivityAndCollapseDeprecated"})
    public void onClick() {
        Intent i = new Intent(this, VoiceLaunchActivity.class).putExtra("action", Dictation.ASSISTANT).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        Runnable go = () -> {
            if (Build.VERSION.SDK_INT >= 34) startActivityAndCollapse(PendingIntent.getActivity(this, 0, i, PendingIntent.FLAG_IMMUTABLE));
            else startActivityAndCollapse(i);
        };
        if (isLocked()) unlockAndRun(go);
        else go.run();
    }
}
