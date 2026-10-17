package com.soapboxrace.reference;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.soapboxrace.core.bo.PerformanceBO;
import com.soapboxrace.core.jpa.CarEntity;
import com.soapboxrace.core.jpa.PerformancePartEntity;

import javax.persistence.EntityManagerFactory;
import javax.persistence.Persistence;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.Set;

/**
 * Car ratings and classes as {@code PerformanceBO.calcNewCarClass} works them out, into
 * fixtures/vectors/car_ratings.json: every model in {@code car_classes}, bare and with random sets
 * of performance parts, and some physics profiles that aren't a model.
 *
 * <pre>java ... com.soapboxrace.reference.CarRatings &lt;repo&gt;</pre>
 *
 * Each case starts from a car with class hash and rating -1, so a car the BO leaves alone shows.
 */
public final class CarRatings {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().serializeNulls().create();

    public static void main(String[] args) throws Exception {
        Path repo = Paths.get(args[0]).toAbsolutePath().normalize();
        String url = env("REFERENCE_JDBC_URL", "jdbc:mysql://mysql:3306/SOAPBOX?useSSL=false&allowPublicKeyRetrieval=true");
        String password = env("REFERENCE_DB_PASSWORD", "reference");
        Map<String, Object> jpa = new HashMap<>();
        jpa.put("javax.persistence.jdbc.url", url);
        jpa.put("javax.persistence.jdbc.user", "root");
        jpa.put("javax.persistence.jdbc.password", password);

        List<Map<String, Object>> cases = new ArrayList<>();
        try (Db db = new Db(url, "root", password)) {
            db.reset(Collections.emptyMap());
            List<String> profiles = db.strings("SELECT hash FROM car_classes WHERE hash IS NOT NULL ORDER BY store_name");
            List<String> parts = db.strings("SELECT hash FROM product WHERE productType = 'PERFORMANCEPART' AND hash IS NOT NULL ORDER BY id");

            Determinism.reset(1, Instant.parse("2026-01-01T00:00:00Z"));
            EntityManagerFactory emf = Persistence.createEntityManagerFactory("soapboxrace-pu", jpa);
            Fakes.recording = new Recording();
            Fakes.personaGroups = Collections.emptyMap();
            Container container = new Container(emf, Fakes.recording, CoreClasses.load(), Fakes.substitutes());
            container.boot();
            PerformanceBO performanceBO = container.get(PerformanceBO.class);

            Random r = new Random(5);
            List<Integer> physics = new ArrayList<>();
            for (String p : profiles) physics.add(Integer.parseInt(p));
            for (int i = 0; i < 20; i++) physics.add(r.nextInt());
            for (int profile : physics) {
                for (int set = 0; set < 6; set++) {
                    List<Integer> chosen = new ArrayList<>();
                    int n = set == 0 ? 0 : 1 + r.nextInt(set == 5 ? 12 : 6);
                    for (int i = 0; i < n; i++) chosen.add(Integer.parseInt(parts.get(r.nextInt(parts.size()))));
                    for (boolean ignoreParts : new boolean[]{false, true}) {
                        if (ignoreParts && set != 1) continue;
                        CarEntity car = new CarEntity();
                        car.setPhysicsProfileHash(profile);
                        car.setCarClassHash(-1);
                        car.setRating(-1);
                        Set<PerformancePartEntity> entities = new HashSet<>();
                        for (int hash : chosen) {
                            PerformancePartEntity part = new PerformancePartEntity();
                            part.setPerformancePartAttribHash(hash);
                            part.setCar(car);
                            entities.add(part);
                        }
                        car.setPerformanceParts(entities);
                        performanceBO.calcNewCarClass(car, ignoreParts);
                        Map<String, Object> c = new LinkedHashMap<>();
                        c.put("physics_profile_hash", profile);
                        c.put("parts", chosen);
                        c.put("ignore_parts", ignoreParts);
                        c.put("car_class_hash", car.getCarClassHash());
                        c.put("rating", car.getRating());
                        cases.add(c);
                    }
                }
            }
            emf.close();
        }

        Map<String, Object> root = new LinkedHashMap<>();
        root.put("about", "PerformanceBO.calcNewCarClass(car, ignoreParts) for every car_classes model with random"
                + " performance parts, and random physics profiles (reference/harness/.../CarRatings.java);"
                + " cars start with class hash and rating -1");
        root.put("upstream", CoreClasses.UPSTREAM);
        root.put("cases", cases);
        Path out = repo.resolve("fixtures/vectors/car_ratings.json");
        try (Writer w = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
            GSON.toJson(root, w);
            w.write('\n');
        }
        System.out.println("wrote " + cases.size() + " car rating cases");
        System.exit(0);
    }

    private static String env(String name, String fallback) {
        String v = System.getenv(name);
        return v == null || v.isEmpty() ? fallback : v;
    }
}
