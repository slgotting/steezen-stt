package com.slgotting.zenassistant;

import android.content.Context;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;
import org.json.JSONObject;
import com.slgotting.steezen.common.Core;

final class AgentLog {
    private static final int KEEP = 300;

    private static File file(Context c) {
        return new File(c.getFilesDir(), "assistant-log.jsonl");
    }

    static synchronized void add(Context c, String kind, String text, String outcome) {
        if (!Core.settings().optBoolean("agent_log", true)) return;
        try {
            List<String> lines = read(c);
            lines.add(new JSONObject().put("time", System.currentTimeMillis()).put("kind", kind).put("text", text).put("outcome", outcome).toString());
            if (lines.size() > KEEP) lines = lines.subList(lines.size() - KEEP, lines.size());
            Files.write(file(c).toPath(), lines, StandardCharsets.UTF_8);
        } catch (Exception ignored) {
        }
    }

    static synchronized List<String> read(Context c) {
        try {
            return file(c).exists() ? new ArrayList<>(Files.readAllLines(file(c).toPath(), StandardCharsets.UTF_8)) : new ArrayList<>();
        } catch (Exception e) {
            return new ArrayList<>();
        }
    }

    static synchronized void clear(Context c) {
        file(c).delete();
    }
}
