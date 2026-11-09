package com.slgotting.steezen.common;

import org.json.JSONObject;

public final class Core {
    static {
        System.loadLibrary("steezen_mobile");
    }

    private static native String call(String method, String args, byte[] audio);

    public static Object run(String method, JSONObject args, byte[] audio) throws Exception {
        JSONObject r = new JSONObject(call(method, args == null ? "{}" : args.toString(), audio));
        if (!r.optBoolean("ok")) throw new Exception(r.optString("error", "unknown error"));
        return r.opt("value");
    }

    public static JSONObject obj(String method, JSONObject args) throws Exception {
        return (JSONObject) run(method, args, null);
    }

    public static JSONObject settings() {
        try {
            return obj("load", null);
        } catch (Exception e) {
            return new JSONObject();
        }
    }

    public static void save(JSONObject s) throws Exception {
        run("save", new JSONObject().put("settings", s), null);
    }
}
