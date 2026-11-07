package com.slgotting.steezenstt;

import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.media.AudioAttributes;
import android.media.AudioFormat;
import android.media.AudioTrack;
import android.media.session.MediaController;
import android.media.session.MediaSession;
import android.media.session.MediaSessionManager;
import android.media.session.PlaybackState;
import android.os.Handler;
import android.os.Looper;
import android.view.KeyEvent;
import java.util.Set;

final class EarbudSession {
    private static final long LONG_MS = 600;
    private static final Set<Integer> BUTTONS = Set.of(KeyEvent.KEYCODE_HEADSETHOOK, KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE, KeyEvent.KEYCODE_MEDIA_PLAY, KeyEvent.KEYCODE_MEDIA_PAUSE);

    private final Context c;
    private final Handler main = new Handler(Looper.getMainLooper());
    private MediaSession session;
    private boolean fired;
    private final Runnable longPress;

    EarbudSession(Context c) {
        this.c = c;
        longPress = () -> {
            fired = true;
            VoiceLaunchActivity.trigger(c);
        };
    }

    void sync() {
        boolean want = Core.settings().optBoolean("earbud_media_session") && !"Off".equals(Core.settings().optString("earbud_action", "Off"));
        if (want == (session != null)) return;
        if (!want) {
            release();
            return;
        }
        session = new MediaSession(c, "steezen-stt earbuds");
        session.setCallback(new MediaSession.Callback() {
            @Override
            public boolean onMediaButtonEvent(Intent i) {
                return handle(i.getParcelableExtra(Intent.EXTRA_KEY_EVENT, KeyEvent.class));
            }
        });
        session.setPlaybackState(new PlaybackState.Builder().setActions(PlaybackState.ACTION_PLAY_PAUSE).setState(PlaybackState.STATE_PLAYING, 0, 0).build());
        session.setActive(true);
        claim();
        session.setPlaybackState(new PlaybackState.Builder().setActions(PlaybackState.ACTION_PLAY_PAUSE).setState(PlaybackState.STATE_PAUSED, 0, 0).build());
    }

    void release() {
        if (session != null) session.release();
        session = null;
    }

    private void claim() {
        int rate = 16000, n = rate / 4;
        AudioTrack t = new AudioTrack.Builder().setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
                .setAudioFormat(new AudioFormat.Builder().setSampleRate(rate).setEncoding(AudioFormat.ENCODING_PCM_16BIT).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build())
                .setTransferMode(AudioTrack.MODE_STATIC).setBufferSizeInBytes(n * 2).build();
        t.write(new short[n], 0, n);
        t.play();
        main.postDelayed(t::release, 600);
    }

    private boolean handle(KeyEvent e) {
        if (e == null) return false;
        if (e.getKeyCode() == KeyEvent.KEYCODE_VOICE_ASSIST) {
            if (e.getAction() == KeyEvent.ACTION_DOWN && e.getRepeatCount() == 0) VoiceLaunchActivity.trigger(c);
            return true;
        }
        if (!BUTTONS.contains(e.getKeyCode())) return false;
        if (e.getAction() == KeyEvent.ACTION_DOWN && e.getRepeatCount() == 0) {
            fired = false;
            main.postDelayed(longPress, LONG_MS);
        } else if (e.getAction() == KeyEvent.ACTION_UP) {
            main.removeCallbacks(longPress);
            if (!fired) forward(e);
        }
        return true;
    }

    private void forward(KeyEvent up) {
        try {
            for (MediaController mc : c.getSystemService(MediaSessionManager.class).getActiveSessions(new ComponentName(c, Notifs.class))) {
                if (session != null && mc.getSessionToken().equals(session.getSessionToken())) continue;
                mc.dispatchMediaButtonEvent(new KeyEvent(KeyEvent.ACTION_DOWN, up.getKeyCode()));
                mc.dispatchMediaButtonEvent(new KeyEvent(KeyEvent.ACTION_UP, up.getKeyCode()));
                return;
            }
        } catch (SecurityException ignored) {
        }
        Dictation.toast(c, "Play/pause can't be passed on to your music app without notification access (steezen-stt → Setup).");
    }
}
