package com.slgotting.steezenstt;

import android.service.voice.VoiceInteractionService;

public class AssistService extends VoiceInteractionService {
    static volatile AssistService instance;

    @Override
    public void onReady() {
        super.onReady();
        instance = this;
    }

    @Override
    public void onShutdown() {
        instance = null;
        super.onShutdown();
    }
}
