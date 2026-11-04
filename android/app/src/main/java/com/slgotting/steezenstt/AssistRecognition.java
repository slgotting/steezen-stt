package com.slgotting.steezenstt;

import android.content.Intent;
import android.speech.RecognitionService;
import android.speech.SpeechRecognizer;

public class AssistRecognition extends RecognitionService {
    @Override
    protected void onStartListening(Intent intent, Callback cb) {
        try {
            cb.error(SpeechRecognizer.ERROR_CLIENT);
        } catch (Exception ignored) {
        }
    }

    @Override
    protected void onCancel(Callback cb) {
    }

    @Override
    protected void onStopListening(Callback cb) {
    }
}
