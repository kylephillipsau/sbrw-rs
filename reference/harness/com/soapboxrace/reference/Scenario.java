package com.soapboxrace.reference;

import com.google.gson.annotations.SerializedName;

import java.util.List;
import java.util.Map;

/**
 * A scenario file (scenarios/**&#47;*.json): a sequence of steps run against the
 * original core. sbrw-rs replays the same file and must produce the same fixture.
 */
public final class Scenario {
    public String description;
    /** Seed for the shared java.util.Random. */
    public long seed = 1;
    /** Starting reference time (UTC, ISO-8601). */
    public String clock = "2026-01-01T00:00:00Z";
    /** Overrides for rows of the {@code parameter} table. */
    public Map<String, String> parameters;
    /** Simulated Openfire state the core may query. */
    public Openfire openfire;
    public List<Step> steps;

    public static final class Openfire {
        @SerializedName("online_users")
        public int onlineUsers;
        /** personaId -> members of the group chat room that persona is in. */
        public Map<String, List<Long>> groups;
    }

    public static final class Step {
        public String note;
        public HttpRequest http;
        @SerializedName("advance_ms")
        public Long advanceMs;
        @SerializedName("fire_timers")
        public Boolean fireTimers;
        /** A @Schedule method, e.g. "BanBO#checkExpiredBans". */
        public String job;
    }

    public static final class HttpRequest {
        public String method = "GET";
        public String path;
        public Map<String, String> query;
        public Map<String, String> headers;
        public Map<String, String> form;
        public String body;
        @SerializedName("content_type")
        public String contentType;
        @SerializedName("remote_addr")
        public String remoteAddr;
    }
}
