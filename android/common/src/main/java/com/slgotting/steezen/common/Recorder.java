package com.slgotting.steezen.common;

import android.media.AudioFormat;
import android.media.AudioRecord;
import android.media.MediaRecorder;
import java.io.ByteArrayOutputStream;

final class Recorder {
    static final int RATE = 16000;
    private static final double VOICE = 0.02;

    private final ByteArrayOutputStream out = new ByteArrayOutputStream();
    private AudioRecord rec;
    private Thread thread;
    private volatile boolean running;
    volatile boolean heard;
    volatile long lastVoice = System.currentTimeMillis();
    final long started = System.currentTimeMillis();

    @SuppressWarnings("MissingPermission")
    void start() throws Exception {
        int min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT);
        rec = new AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, Math.max(min, RATE));
        if (rec.getState() != AudioRecord.STATE_INITIALIZED) throw new Exception("The microphone is not available");
        rec.startRecording();
        running = true;
        thread = new Thread(() -> {
            byte[] buf = new byte[RATE / 5];
            while (running) {
                int n = rec.read(buf, 0, buf.length);
                if (n <= 0) continue;
                synchronized (out) {
                    out.write(buf, 0, n);
                }
                double sum = 0;
                for (int i = 0; i + 1 < n; i += 2) {
                    double s = (short) ((buf[i] & 0xff) | (buf[i + 1] << 8)) / 32768.0;
                    sum += s * s;
                }
                if (Math.sqrt(sum / (n / 2.0)) > VOICE) {
                    heard = true;
                    lastVoice = System.currentTimeMillis();
                }
            }
        });
        thread.start();
    }

    long silentMs() {
        return System.currentTimeMillis() - lastVoice;
    }

    byte[] stop() {
        running = false;
        try {
            thread.join(500);
        } catch (InterruptedException ignored) {
        }
        rec.stop();
        rec.release();
        synchronized (out) {
            return out.toByteArray();
        }
    }
}
