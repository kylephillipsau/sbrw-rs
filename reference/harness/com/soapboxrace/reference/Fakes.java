package com.soapboxrace.reference;

import com.soapboxrace.core.bo.ErrorReportingBO;
import com.soapboxrace.core.bo.RedisBO;
import com.soapboxrace.core.bo.StartupBO;
import com.soapboxrace.core.xmpp.OpenFireConnector;
import com.soapboxrace.core.xmpp.OpenFireRestApiCli;
import io.lettuce.core.MapScanCursor;
import io.lettuce.core.api.StatefulRedisConnection;
import io.lettuce.core.api.sync.RedisCommands;
import io.lettuce.core.pubsub.StatefulRedisPubSubConnection;
import io.lettuce.core.pubsub.api.sync.RedisPubSubCommands;
import org.igniterealtime.restclient.entity.MUCRoomEntity;

import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.regex.Pattern;

/**
 * Replacements for the beans that talk to Openfire, Redis and Sentry. They
 * record what the core asked for instead, so the XMPP pushes and REST calls
 * become part of each fixture.
 */
public final class Fakes {
    /** Shared by the fakes of one scenario run. */
    static Recording recording;
    static Map<String, List<Long>> personaGroups = Collections.emptyMap();
    static int onlineUsers;

    private Fakes() {
    }

    public static Map<Class<?>, Class<?>> substitutes() {
        Map<Class<?>, Class<?>> m = new HashMap<>();
        m.put(OpenFireConnector.class, RecordingConnector.class);
        m.put(OpenFireRestApiCli.class, RecordingRestApiCli.class);
        m.put(RedisBO.class, InMemoryRedisBO.class);
        m.put(ErrorReportingBO.class, SilentErrorReportingBO.class);
        m.put(StartupBO.class, StartupWithoutDuplicateStats.class);
        return m;
    }

    /** Captures every XMPP message the engine would send, exactly as marshalled. */
    public static class RecordingConnector extends OpenFireConnector {
        @Override
        public void init() {
        }

        @Override
        public void connect() {
        }

        @Override
        public void send(String msg, Long personaId) {
            recording.xmpp(personaId, msg);
        }
    }

    public static class RecordingRestApiCli extends OpenFireRestApiCli {
        @Override
        public void init() {
            // Upstream creates the engine account and public chat rooms here.
            recording.openfire("init");
        }

        @Override
        public void createUpdatePersona(String user, String password) {
            recording.openfire("createUpdatePersona", user, password);
        }

        @Override
        public void createUpdatePersona(Long personaId, String password) {
            recording.openfire("createUpdatePersona", personaId, password);
        }

        @Override
        public int getTotalOnlineUsers() {
            recording.openfire("getTotalOnlineUsers");
            return onlineUsers;
        }

        @Override
        public List<Long> getAllPersonaByGroup(Long personaId) {
            recording.openfire("getAllPersonaByGroup", personaId);
            return new ArrayList<>(personaGroups.getOrDefault(String.valueOf(personaId), Collections.emptyList()));
        }

        @Override
        public List<Long> getAllOccupantsInRoom(String roomName) {
            recording.openfire("getAllOccupantsInRoom", roomName);
            return new ArrayList<>();
        }

        @Override
        public List<MUCRoomEntity> getAllRooms() {
            recording.openfire("getAllRooms");
            return new ArrayList<>();
        }

        @Override
        public void sendChatAnnouncement(String message) {
            recording.openfire("sendChatAnnouncement", message);
        }
    }

    /**
     * Upstream's StartupBO.init calls OnlineUsersBO.insertOnlineStats, whose own
     * {@code @PostConstruct} has just inserted a row keyed by the same second. Real
     * servers get away with it because a cold start puts the two in different
     * seconds; with a frozen clock they always collide. Keep the one insert.
     */
    public static class StartupWithoutDuplicateStats extends StartupBO {
        @Override
        public void init() {
        }
    }

    public static class SilentErrorReportingBO extends ErrorReportingBO {
        @Override
        public void init() {
        }

        @Override
        public void sendException(Exception exception) {
        }
    }

    /**
     * Redis-on semantics without a Redis server: the commands the core uses
     * (strings, hashes, sets, KEYS, HSCAN, PUBLISH) against in-memory maps.
     */
    public static class InMemoryRedisBO extends RedisBO {
        private final Map<String, String> strings = new TreeMap<>();
        private final Map<String, Map<String, String>> hashes = new TreeMap<>();
        private final Map<String, Set<String>> sets = new TreeMap<>();

        @Override
        public void init() {
        }

        @Override
        @SuppressWarnings("unchecked")
        public StatefulRedisConnection<String, String> getConnection() {
            RedisCommands<String, String> sync = (RedisCommands<String, String>) Proxy.newProxyInstance(
                    RedisCommands.class.getClassLoader(), new Class<?>[]{RedisCommands.class},
                    (p, m, a) -> command(m, a));
            return (StatefulRedisConnection<String, String>) Proxy.newProxyInstance(
                    StatefulRedisConnection.class.getClassLoader(), new Class<?>[]{StatefulRedisConnection.class},
                    (p, m, a) -> {
                        if (m.getName().equals("sync")) return sync;
                        if (m.getName().equals("close")) return null;
                        throw new UnsupportedOperationException("StatefulRedisConnection." + m.getName());
                    });
        }

        @Override
        @SuppressWarnings("unchecked")
        public StatefulRedisPubSubConnection<String, String> createPubSub() {
            RedisPubSubCommands<String, String> sync = (RedisPubSubCommands<String, String>) Proxy.newProxyInstance(
                    RedisPubSubCommands.class.getClassLoader(), new Class<?>[]{RedisPubSubCommands.class},
                    (p, m, a) -> command(m, a));
            return (StatefulRedisPubSubConnection<String, String>) Proxy.newProxyInstance(
                    StatefulRedisPubSubConnection.class.getClassLoader(),
                    new Class<?>[]{StatefulRedisPubSubConnection.class},
                    (p, m, a) -> {
                        if (m.getName().equals("sync")) return sync;
                        if (m.getName().equals("close") || m.getName().equals("addListener")) return null;
                        throw new UnsupportedOperationException("StatefulRedisPubSubConnection." + m.getName());
                    });
        }

        private Object command(Method m, Object[] a) {
            String name = m.getName();
            Object[] args = a == null ? new Object[0] : a;
            switch (name) {
                case "get":
                    return strings.get((String) args[0]);
                case "set":
                    recording.redis("SET", args[0], args[1]);
                    strings.put((String) args[0], (String) args[1]);
                    return "OK";
                case "del": {
                    String[] keys = (String[]) args[0];
                    recording.redis("DEL", (Object[]) keys);
                    long n = 0;
                    for (String k : keys) {
                        if (strings.remove(k) != null | hashes.remove(k) != null | sets.remove(k) != null) n++;
                    }
                    return n;
                }
                case "keys": {
                    Pattern glob = Pattern.compile(((String) args[0]).replace(".", "\\.").replace("*", ".*"));
                    List<String> out = new ArrayList<>();
                    for (String k : allKeys()) if (glob.matcher(k).matches()) out.add(k);
                    return out;
                }
                case "hset":
                    recording.redis("HSET", args[0], args[1], args[2]);
                    return hashes.computeIfAbsent((String) args[0], k -> new LinkedHashMap<>())
                            .put((String) args[1], (String) args[2]) == null;
                case "hdel": {
                    String[] fields = (String[]) args[1];
                    recording.redis("HDEL", args[0], fields);
                    Map<String, String> h = hashes.getOrDefault((String) args[0], new HashMap<>());
                    long n = 0;
                    for (String f : fields) if (h.remove(f) != null) n++;
                    return n;
                }
                case "hscan": {
                    MapScanCursor<String, String> cursor = new MapScanCursor<>();
                    cursor.getMap().putAll(hashes.getOrDefault((String) args[0], new LinkedHashMap<>()));
                    cursor.setCursor("0");
                    cursor.setFinished(true);
                    return cursor;
                }
                case "sadd": {
                    String[] members = (String[]) args[1];
                    recording.redis("SADD", args[0], members);
                    Set<String> s = sets.computeIfAbsent((String) args[0], k -> new LinkedHashSet<>());
                    long n = 0;
                    for (String v : members) if (s.add(v)) n++;
                    return n;
                }
                case "sismember":
                    return sets.getOrDefault((String) args[0], Collections.emptySet()).contains((String) args[1]);
                case "publish":
                    recording.redis("PUBLISH", args[0], args[1]);
                    return 0L;
                default:
                    throw new UnsupportedOperationException("redis " + name);
            }
        }

        private Set<String> allKeys() {
            Set<String> keys = new java.util.TreeSet<>(strings.keySet());
            keys.addAll(hashes.keySet());
            keys.addAll(sets.keySet());
            return keys;
        }
    }

}
