package com.soapboxrace.reference;

import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Side effects observed while one scenario step runs. */
public final class Recording {
    private List<Map<String, Object>> xmpp = new ArrayList<>();
    private List<Map<String, Object>> openfire = new ArrayList<>();
    private List<Map<String, Object>> timers = new ArrayList<>();
    private List<Map<String, Object>> redis = new ArrayList<>();

    public void xmpp(Long to, String body) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("to", to);
        m.put("body", body);
        xmpp.add(m);
    }

    public void openfire(String call, Object... args) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("call", call);
        m.put("args", args);
        openfire.add(m);
    }

    public void timer(String bean, long delayMillis, Object info) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("bean", bean);
        m.put("delay_ms", delayMillis);
        m.put("info", info(info));
        timers.add(m);
    }

    /**
     * A timer's info by its fields, in declaration order. Its toString() is
     * Object's, whose identity hash no other run reproduces.
     */
    private static Object info(Object info) {
        if (info == null || info instanceof String || info instanceof Number || info instanceof Boolean) {
            return info;
        }
        Map<String, Object> fields = new LinkedHashMap<>();
        for (Field field : info.getClass().getDeclaredFields()) {
            if (Modifier.isStatic(field.getModifiers())) {
                continue;
            }
            try {
                field.setAccessible(true);
                fields.put(field.getName(), field.get(info));
            } catch (IllegalAccessException e) {
                throw new IllegalStateException(e);
            }
        }
        return fields;
    }

    public void redis(String command, Object... args) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("command", command);
        m.put("args", args);
        redis.add(m);
    }

    /** Return what was recorded since the last call and start afresh. */
    public Map<String, Object> drain() {
        Map<String, Object> out = new LinkedHashMap<>();
        if (!xmpp.isEmpty()) out.put("xmpp", xmpp);
        if (!openfire.isEmpty()) out.put("openfire", openfire);
        if (!timers.isEmpty()) out.put("timers", timers);
        if (!redis.isEmpty()) out.put("redis", redis);
        xmpp = new ArrayList<>();
        openfire = new ArrayList<>();
        timers = new ArrayList<>();
        redis = new ArrayList<>();
        return out;
    }
}
