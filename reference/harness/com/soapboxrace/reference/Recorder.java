package com.soapboxrace.reference;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;

import javax.persistence.EntityManagerFactory;
import javax.persistence.Persistence;
import java.io.IOException;
import java.io.Reader;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.stream.Collectors;
import java.util.stream.Stream;

/**
 * Records fixtures by running scenarios against the original core.
 *
 * <pre>java ... com.soapboxrace.reference.Recorder &lt;repo&gt; [scenario.json ...]</pre>
 *
 * Reads scenarios/**&#47;*.json (or the given files) and writes
 * fixtures/recorded/&lt;same path&gt;.json.
 */
public final class Recorder {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().serializeNulls().create();

    public static void main(String[] args) throws Exception {
        Path repo = Paths.get(args.length > 0 ? args[0] : ".").toAbsolutePath().normalize();
        Path scenariosDir = repo.resolve("scenarios");
        List<Path> scenarios = new ArrayList<>();
        if (args.length > 1) {
            for (int i = 1; i < args.length; i++) scenarios.add(repo.resolve(args[i]).normalize());
        } else {
            try (Stream<Path> s = Files.walk(scenariosDir)) {
                scenarios = s.filter(p -> p.toString().endsWith(".json")).sorted().collect(Collectors.toList());
            }
        }

        String url = env("REFERENCE_JDBC_URL", "jdbc:mysql://mysql:3306/SOAPBOX?useSSL=false&allowPublicKeyRetrieval=true");
        Map<String, Object> jpa = new HashMap<>();
        jpa.put("javax.persistence.jdbc.url", url);
        jpa.put("javax.persistence.jdbc.user", "root");
        jpa.put("javax.persistence.jdbc.password", env("REFERENCE_DB_PASSWORD", "reference"));
        List<Class<?>> coreClasses = CoreClasses.load();

        int failures = 0;
        try (Db db = new Db(url, "root", env("REFERENCE_DB_PASSWORD", "reference"))) {
            EntityManagerFactory emf = Persistence.createEntityManagerFactory("soapboxrace-pu", jpa);
            for (Path file : scenarios) {
                String name = scenariosDir.relativize(file).toString().replace(".json", "");
                try {
                    Map<String, Object> fixture = record(name, read(file), db, emf, coreClasses);
                    Path out = repo.resolve("fixtures/recorded").resolve(name + ".json");
                    Files.createDirectories(out.getParent());
                    try (Writer w = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
                        GSON.toJson(fixture, w);
                        w.write('\n');
                    }
                    System.out.println("recorded " + name);
                } catch (Exception e) {
                    failures++;
                    System.err.println("FAILED " + name);
                    e.printStackTrace();
                }
            }
            emf.close();
        }
        System.exit(failures == 0 ? 0 : 1);
    }

    static Map<String, Object> record(String name, Scenario s, Db db, EntityManagerFactory emf,
                                      List<Class<?>> coreClasses) throws Exception {
        db.reset(s.parameters);
        Determinism.reset(s.seed, Instant.parse(s.clock));
        Recording recording = new Recording();
        Fakes.recording = recording;
        Fakes.personaGroups = s.openfire != null && s.openfire.groups != null ? s.openfire.groups : Collections.emptyMap();
        Fakes.onlineUsers = s.openfire != null ? s.openfire.onlineUsers : 0;

        emf.getCache().evictAll();
        Container container = new Container(emf, recording, coreClasses, Fakes.substitutes());
        container.boot();
        Http http = new Http(container, coreClasses);

        Map<String, Object> fixture = new LinkedHashMap<>();
        fixture.put("scenario", name);
        fixture.put("description", s.description);
        fixture.put("upstream", CoreClasses.UPSTREAM);
        fixture.put("boot", recording.drain());

        List<Map<String, Object>> steps = new ArrayList<>();
        for (Scenario.Step step : s.steps) {
            Map<String, Object> result = new LinkedHashMap<>();
            if (step.note != null) result.put("note", step.note);
            if (step.http != null) {
                result.put("request", step.http);
                result.put("response", http.invoke(step.http));
            } else if (step.advanceMs != null) {
                Determinism.advance(step.advanceMs);
                result.put("advance_ms", step.advanceMs);
            } else if (Boolean.TRUE.equals(step.fireTimers)) {
                result.put("timers_fired", container.fireDueTimers());
            } else if (step.job != null) {
                container.runJob(step.job);
                result.put("job", step.job);
            } else if (step.sql != null) {
                db.execute(step.sql);
                result.put("sql", step.sql);
            }
            Map<String, Object> effects = recording.drain();
            if (!effects.isEmpty()) result.put("effects", effects);
            steps.add(result);
        }
        fixture.put("steps", steps);
        fixture.put("db", db.snapshot());
        return fixture;
    }

    private static Scenario read(Path file) throws IOException {
        try (Reader r = Files.newBufferedReader(file, StandardCharsets.UTF_8)) {
            Scenario s = GSON.fromJson(r, Scenario.class);
            if (s.steps == null) throw new IllegalArgumentException(file + ": no steps");
            return s;
        }
    }

    private static String env(String name, String fallback) {
        String v = System.getenv(name);
        return v == null || v.isEmpty() ? fallback : v;
    }
}
