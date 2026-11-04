package com.slgotting.steezenstt;

import org.json.JSONObject;

final class Core {
    static {
        System.loadLibrary("steezen_mobile");
    }

    private static native String call(String method, String args, byte[] audio);

    static Object run(String method, JSONObject args, byte[] audio) throws Exception {
        JSONObject r = new JSONObject(call(method, args == null ? "{}" : args.toString(), audio));
        if (!r.optBoolean("ok")) throw new Exception(r.optString("error", "unknown error"));
        return r.opt("value");
    }

    static JSONObject obj(String method, JSONObject args) throws Exception {
        return (JSONObject) run(method, args, null);
    }

    static JSONObject settings() {
        try {
            return obj("load", null);
        } catch (Exception e) {
            return new JSONObject();
        }
    }

    static void save(JSONObject s) throws Exception {
        run("save", new JSONObject().put("settings", s), null);
    }
}
