package com.soapboxrace.reference;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.soapboxrace.core.bo.util.AchievementCommerceContext;
import com.soapboxrace.core.bo.util.AchievementEventContext;
import com.soapboxrace.core.bo.util.AchievementProgressionContext;
import com.soapboxrace.core.jpa.CarClassesEntity;
import com.soapboxrace.core.jpa.CarEntity;
import com.soapboxrace.core.jpa.EventDataEntity;
import com.soapboxrace.core.jpa.EventEntity;
import com.soapboxrace.core.jpa.EventMode;
import com.soapboxrace.core.jpa.EventSessionEntity;
import com.soapboxrace.core.jpa.LobbyEntity;
import com.soapboxrace.core.jpa.PersonaAchievementEntity;
import com.soapboxrace.core.jpa.PersonaEntity;
import com.soapboxrace.jaxb.http.ArbitrationPacket;
import com.soapboxrace.jaxb.http.DragArbitrationPacket;
import com.soapboxrace.jaxb.http.PursuitArbitrationPacket;
import com.soapboxrace.jaxb.http.RouteArbitrationPacket;
import com.soapboxrace.jaxb.http.TeamEscapeArbitrationPacket;

import javax.script.ScriptEngine;
import javax.script.ScriptEngineManager;
import javax.script.ScriptException;
import javax.script.SimpleBindings;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;

/**
 * Every achievement's update trigger and update value, as {@code ScriptingBO} runs them on Nashorn,
 * against generated contexts of the achievement's category, into
 * fixtures/vectors/achievement_triggers.json. Each context is recorded as the few values it's
 * built from, so sbrw-rs builds the same bindings.
 *
 * <pre>java ... com.soapboxrace.reference.AchievementTriggers &lt;repo&gt;</pre>
 *
 * EVENT contexts include packets of another class than the event's mode, where a script reads a
 * property the packet hasn't got.
 */
public final class AchievementTriggers {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().serializeNulls().create();
    private static final String SEPARATOR = "\u0001";

    private static final int[] MODES = {4, 9, 19, 12, 24, 22};
    private static final int[] CLASSES = {-405837480, -406473455, 1866825865, 415909161, 872416321, -2142411446, 607077938};
    private static final String[] MANUFACTURERS = {"DODGE", "BMW", "FORD SHELBY", "MERCEDEZ BENZ", "PORSCHE", "AUDI",
            "NISSAN", "ASTON MARTIN", "dodge", "TESLA"};

    public static void main(String[] args) throws Exception {
        Path repo = Paths.get(args[0]).toAbsolutePath().normalize();
        String url = env("REFERENCE_JDBC_URL", "jdbc:mysql://mysql:3306/SOAPBOX?useSSL=false&allowPublicKeyRetrieval=true");
        String password = env("REFERENCE_DB_PASSWORD", "reference");

        List<String[]> achievements = new ArrayList<>();
        try (Db db = new Db(url, "root", password)) {
            db.reset(Collections.emptyMap());
            for (String row : db.strings("SELECT CONCAT_WS(CHAR(1), ID, IFNULL(category, ''), IFNULL(update_trigger, ''),"
                    + " IFNULL(update_value, '')) FROM achievement ORDER BY ID")) {
                achievements.add(row.split(SEPARATOR, -1));
            }
        }

        ScriptEngine engine = new ScriptEngineManager().getEngineByName("nashorn");
        Random r = new Random(17);
        Map<String, List<Map<String, Object>>> contexts = new LinkedHashMap<>();
        contexts.put("EVENT", new ArrayList<>());
        contexts.put("PROGRESSION", new ArrayList<>());
        contexts.put("COMMERCE", new ArrayList<>());
        for (int i = 0; i < 60; i++) contexts.get("EVENT").add(eventSpec(r));
        for (int i = 0; i < 30; i++) contexts.get("PROGRESSION").add(progressionSpec(r));
        for (int i = 0; i < 24; i++) contexts.get("COMMERCE").add(commerceSpec(r));

        List<Map<String, Object>> cases = new ArrayList<>();
        int scripts = 0;
        for (String[] a : achievements) {
            String category = a[1];
            List<Map<String, Object>> specs = contexts.get(category);
            if (specs == null || a[2].isEmpty()) continue;
            for (int s = 0; s < 2; s++) {
                String script = a[2 + s];
                if (script.isEmpty()) continue;
                scripts++;
                for (int i = 0; i < specs.size(); i++) {
                    Map<String, Object> c = new LinkedHashMap<>();
                    c.put("achievement", Long.parseLong(a[0]));
                    c.put("script", s == 0 ? "trigger" : "value");
                    c.put("context", i);
                    try {
                        Object result = engine.eval(script, new SimpleBindings(bindings(category, specs.get(i))));
                        c.put("class", result == null ? null : result.getClass().getName());
                        c.put("value", result == null ? null : String.valueOf(result));
                    } catch (ScriptException | RuntimeException e) {
                        c.put("error", e.getClass().getName());
                    }
                    cases.add(c);
                }
            }
        }

        Map<String, Object> root = new LinkedHashMap<>();
        root.put("about", "Each achievement's update_trigger and update_value on Nashorn (" + System.getProperty("java.version")
                + "), as ScriptingBO.eval runs them, against generated contexts of its category"
                + " (reference/harness/.../AchievementTriggers.java); class and value of the result, or the exception thrown");
        root.put("upstream", CoreClasses.UPSTREAM);
        root.put("contexts", contexts);
        root.put("cases", cases);
        Path out = repo.resolve("fixtures/vectors/achievement_triggers.json");
        try (Writer w = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
            GSON.toJson(root, w);
            w.write('\n');
        }
        System.out.println("wrote " + cases.size() + " achievement script cases for " + scripts + " scripts");
        System.exit(0);
    }

    private static <T> T pick(Random r, T[] values) {
        return values[r.nextInt(values.length)];
    }

    private static Long currentValue(Random r) {
        switch (r.nextInt(6)) {
            case 0: return 0L;
            case 1: return 1L;
            case 2: return 5L;
            case 3: return null;
            default: return (long) r.nextInt(10_000_000);
        }
    }

    private static Map<String, Object> eventSpec(Random r) {
        Map<String, Object> s = new LinkedHashMap<>();
        int mode = MODES[r.nextInt(MODES.length)];
        s.put("event_id", pick(r, new Integer[]{22, 232, 383, 499, 392, 61, r.nextInt(600)}));
        s.put("event_mode_id", mode);
        s.put("car_class_hash", CLASSES[r.nextInt(CLASSES.length)]);
        s.put("track_length", pick(r, new Float[]{0f, 8f, 10.0565f, 0.46192f, r.nextFloat() * 30}));
        int lobby = r.nextInt(3);
        s.put("lobby", lobby == 0 ? null : Map.of("is_private", lobby == 1));
        String kind;
        if (r.nextInt(6) == 0) {
            kind = pick(r, new String[]{"route", "drag", "pursuit", "team_escape"});
        } else {
            kind = mode == 19 ? "drag" : mode == 12 ? "pursuit" : mode == 24 ? "team_escape" : "route";
        }
        s.put("packet", kind);
        s.put("rank", r.nextInt(3) == 0 ? r.nextInt(5) : 1);
        s.put("finish_reason", pick(r, new Integer[]{22, 518, 266, 2, 8202}));
        s.put("cost_to_state", r.nextInt(3) == 0 ? 0 : r.nextInt(500_000));
        s.put("cops_disabled", r.nextInt(3) == 0 ? 0 : r.nextInt(10));
        s.put("road_blocks_dodged", r.nextInt(3) == 0 ? 0 : r.nextInt(5));
        s.put("sum_of_jumps", r.nextInt(3) == 0 ? 0L : (long) r.nextInt(100_000));
        s.put("fraction_completed", pick(r, new Float[]{0f, 1f, 0.5f, -0.1f, 1.2f, r.nextFloat()}));
        s.put("current_value", currentValue(r));
        return s;
    }

    private static Map<String, Object> progressionSpec(Random r) {
        Map<String, Object> s = new LinkedHashMap<>();
        int level = 1 + r.nextInt(60);
        int score = r.nextInt(500);
        s.put("cash", r.nextInt(3) == 0 ? 0 : r.nextInt(100_000));
        s.put("rep", r.nextInt(3) == 0 ? 0 : r.nextInt(100_000));
        s.put("level", level);
        s.put("score", score);
        s.put("treasure_streak", r.nextInt(10));
        s.put("level_changed", r.nextBoolean());
        s.put("score_changed", r.nextBoolean());
        s.put("treasure_streak_changed", r.nextBoolean());
        s.put("in_event", r.nextBoolean());
        Long current = currentValue(r);
        if (r.nextInt(3) == 0) current = (long) (r.nextBoolean() ? level : score);
        s.put("current_value", current);
        return s;
    }

    private static Map<String, Object> commerceSpec(Random r) {
        Map<String, Object> s = new LinkedHashMap<>();
        boolean car = r.nextInt(5) != 0;
        s.put("commerce_type", car ? "CAR_PURCHASE" : "BUNDLE_PURCHASE");
        s.put("manufactor", car ? pick(r, MANUFACTURERS) : null);
        s.put("rental", car && r.nextInt(3) == 0);
        s.put("current_value", currentValue(r));
        return s;
    }

    private static Map<String, Object> bindings(String category, Map<String, Object> s) {
        Map<String, Object> b = new LinkedHashMap<>();
        b.put("persona", persona());
        PersonaAchievementEntity pa = new PersonaAchievementEntity();
        pa.setCanProgress(true);
        pa.setCurrentValue((Long) s.get("current_value"));
        b.put("personaAchievement", pa);
        switch (category) {
            case "EVENT": {
                EventEntity event = new EventEntity();
                event.setId((Integer) s.get("event_id"));
                event.setEventModeId((Integer) s.get("event_mode_id"));
                event.setCarClassHash((Integer) s.get("car_class_hash"));
                event.setTrackLength((Float) s.get("track_length"));
                EventSessionEntity session = new EventSessionEntity();
                session.setId(1L);
                session.setEvent(event);
                @SuppressWarnings("unchecked")
                Map<String, Object> lobbySpec = (Map<String, Object>) s.get("lobby");
                if (lobbySpec != null) {
                    LobbyEntity lobby = new LobbyEntity();
                    lobby.setId(7L);
                    lobby.setIsPrivate((Boolean) lobbySpec.get("is_private"));
                    session.setLobby(lobby);
                }
                ArbitrationPacket packet = packet(s);
                EventDataEntity data = new EventDataEntity();
                data.setEvent(event);
                data.setPersonaId(100L);
                b.put("event", event);
                b.put("eventData", data);
                b.put("eventSession", session);
                b.put("eventContext", new AchievementEventContext(EventMode.fromId(event.getEventModeId()), packet, session));
                b.put("car", car(null));
                break;
            }
            case "PROGRESSION":
                b.put("progression", new AchievementProgressionContext((Integer) s.get("cash"), (Integer) s.get("rep"),
                        (Integer) s.get("level"), (Integer) s.get("score"), (Integer) s.get("treasure_streak"),
                        (Boolean) s.get("level_changed"), (Boolean) s.get("score_changed"),
                        (Boolean) s.get("treasure_streak_changed"), (Boolean) s.get("in_event")));
                break;
            case "COMMERCE": {
                AchievementCommerceContext ctx = new AchievementCommerceContext(
                        AchievementCommerceContext.CommerceType.valueOf((String) s.get("commerce_type")));
                if (s.get("manufactor") != null) {
                    CarClassesEntity carClasses = new CarClassesEntity();
                    carClasses.setManufactor((String) s.get("manufactor"));
                    ctx.setCarClassesEntity(carClasses);
                    // As BasketBO.buyCar binds it; a bundle has no car.
                    b.put("ownedCar", car((Boolean) s.get("rental") ? LocalDateTime.of(2026, 1, 2, 12, 34, 56) : null));
                }
                b.put("commerceCtx", ctx);
                break;
            }
            default:
                throw new IllegalArgumentException(category);
        }
        return b;
    }

    private static ArbitrationPacket packet(Map<String, Object> s) {
        int rank = (Integer) s.get("rank");
        int finishReason = (Integer) s.get("finish_reason");
        int costToState = (Integer) s.get("cost_to_state");
        int copsDisabled = (Integer) s.get("cops_disabled");
        int roadBlocksDodged = (Integer) s.get("road_blocks_dodged");
        long jumps = (Long) s.get("sum_of_jumps");
        float fraction = (Float) s.get("fraction_completed");
        switch ((String) s.get("packet")) {
            case "route": {
                RouteArbitrationPacket p = new RouteArbitrationPacket();
                p.setSumOfJumpsDurationInMilliseconds(jumps);
                p.setFractionCompleted(fraction);
                return base(p, rank, finishReason);
            }
            case "drag": {
                DragArbitrationPacket p = new DragArbitrationPacket();
                p.setSumOfJumpsDurationInMilliseconds(jumps);
                p.setFractionCompleted(fraction);
                return base(p, rank, finishReason);
            }
            case "pursuit": {
                PursuitArbitrationPacket p = new PursuitArbitrationPacket();
                p.setSumOfJumpsDurationInMilliseconds(jumps);
                p.setCostToState(costToState);
                p.setCopsDisabled(copsDisabled);
                p.setRoadBlocksDodged(roadBlocksDodged);
                return base(p, rank, finishReason);
            }
            default: {
                TeamEscapeArbitrationPacket p = new TeamEscapeArbitrationPacket();
                p.setSumOfJumpsDurationInMilliseconds(jumps);
                p.setFractionCompleted(fraction);
                p.setCostToState(costToState);
                p.setCopsDisabled(copsDisabled);
                p.setRoadBlocksDodged(roadBlocksDodged);
                return base(p, rank, finishReason);
            }
        }
    }

    private static ArbitrationPacket base(ArbitrationPacket p, int rank, int finishReason) {
        p.setRank(rank);
        p.setFinishReason(finishReason);
        p.setCarId(1);
        return p;
    }

    private static PersonaEntity persona() {
        PersonaEntity p = new PersonaEntity();
        p.setPersonaId(100L);
        p.setName("DRIVER1");
        p.setLevel(60);
        p.setScore(27);
        p.setCash(350000.0);
        return p;
    }

    private static CarEntity car(LocalDateTime expires) {
        CarEntity c = new CarEntity();
        c.setId(1L);
        c.setName("challenger71");
        c.setPreset(true);
        c.setExpirationDate(expires);
        return c;
    }

    private static String env(String name, String fallback) {
        String v = System.getenv(name);
        return v == null || v.isEmpty() ? fallback : v;
    }
}
