package com.soapboxrace.reference;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.soapboxrace.core.bo.ItemRewardBO;
import com.soapboxrace.core.bo.util.ItemRewardBase;
import com.soapboxrace.core.bo.util.ItemRewardCash;
import com.soapboxrace.core.bo.util.ItemRewardMulti;
import com.soapboxrace.core.bo.util.ItemRewardProduct;
import com.soapboxrace.core.bo.util.ItemRewardQuantityProduct;

import javax.persistence.EntityManagerFactory;
import javax.persistence.Persistence;
import java.io.Writer;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Every reward script in the game data, run as {@code ItemRewardBO.scriptToItem} runs it (Nashorn,
 * with the reward generator bound as {@code generator}), into fixtures/vectors/reward_scripts.json.
 *
 * <pre>java ... com.soapboxrace.reference.RewardScripts &lt;repo&gt;</pre>
 *
 * Each case is a script and a seed for the shared {@code java.util.Random}. It records the reward
 * the script built, or the exceptions it threw from the outermost in, and the next {@code nextInt()}
 * the Random gives afterwards, which pins how many draws the script made and of which kind.
 */
public final class RewardScripts {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().serializeNulls().create();
    private static final long[] SEEDS = {1, 2, 3};
    private static final Instant CLOCK = Instant.parse("2026-01-01T00:00:00Z");

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
            Set<String> scripts = new LinkedHashSet<>();
            scripts.addAll(db.strings("SELECT script FROM card_pack_item ORDER BY ID"));
            scripts.addAll(db.strings("SELECT script FROM reward_table_item ORDER BY ID"));
            scripts.addAll(db.strings("SELECT rewardScript FROM achievement_reward ORDER BY ID"));

            Determinism.reset(SEEDS[0], CLOCK);
            EntityManagerFactory emf = Persistence.createEntityManagerFactory("soapboxrace-pu", jpa);
            Fakes.recording = new Recording();
            Fakes.personaGroups = Collections.emptyMap();
            Container container = new Container(emf, Fakes.recording, CoreClasses.load(), Fakes.substitutes());
            container.boot();

            Object itemRewardBO = container.target(ItemRewardBO.class);
            // The bean took the shared Random when it was made; give it each case's.
            Field random = ItemRewardBO.class.getDeclaredField("random");
            random.setAccessible(true);
            Method scriptToItem = ItemRewardBO.class.getDeclaredMethod("scriptToItem", String.class);
            scriptToItem.setAccessible(true);

            for (String script : scripts) {
                for (long seed : SEEDS) {
                    Determinism.reset(seed, CLOCK);
                    random.set(itemRewardBO, Determinism.random());
                    Object outcome = container.inTransaction(() -> {
                        try {
                            return scriptToItem.invoke(itemRewardBO, script);
                        } catch (InvocationTargetException e) {
                            return e.getCause();
                        }
                    });
                    Map<String, Object> c = new LinkedHashMap<>();
                    c.put("script", script);
                    c.put("seed", seed);
                    if (outcome instanceof Throwable) {
                        c.put("error", chain((Throwable) outcome));
                    } else {
                        c.put("reward", describe((ItemRewardBase) outcome));
                    }
                    c.put("next_int", Determinism.random().nextInt());
                    cases.add(c);
                }
            }
            emf.close();
        }

        Map<String, Object> root = new LinkedHashMap<>();
        root.put("about", "ItemRewardBO.scriptToItem on every distinct script in card_pack_item, reward_table_item"
                + " and achievement_reward, each with seeds " + java.util.Arrays.toString(SEEDS)
                + "; next_int is the shared Random's nextInt() afterwards (reference/harness/.../RewardScripts.java)");
        root.put("upstream", CoreClasses.UPSTREAM);
        root.put("cases", cases);
        Path out = repo.resolve("fixtures/vectors/reward_scripts.json");
        try (Writer w = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
            GSON.toJson(root, w);
            w.write('\n');
        }
        System.out.println("wrote " + cases.size() + " reward script cases");
        System.exit(0);
    }

    /** The reward as a tree: cash, a product (with its use count when it has one), or several. */
    private static Object describe(ItemRewardBase reward) {
        Map<String, Object> m = new LinkedHashMap<>();
        if (reward instanceof ItemRewardCash) {
            m.put("cash", ((ItemRewardCash) reward).getCash());
        } else if (reward instanceof ItemRewardMulti) {
            List<Object> items = new ArrayList<>();
            for (ItemRewardBase r : ((ItemRewardMulti) reward).getAchievementRewardList()) items.add(describe(r));
            m.put("multi", items);
        } else if (reward instanceof ItemRewardProduct) {
            m.put("product", reward.getProducts().get(0).getProductId());
            if (reward instanceof ItemRewardQuantityProduct) {
                m.put("use_count", ((ItemRewardQuantityProduct) reward).getUseCount());
            }
        } else {
            m.put("other", reward.getClass().getName());
        }
        return m;
    }

    /** Each exception from the outermost to its root cause: class and message. */
    private static List<Map<String, Object>> chain(Throwable t) {
        List<Map<String, Object>> out = new ArrayList<>();
        for (Throwable e = t; e != null && out.size() < 10; e = e.getCause()) {
            Map<String, Object> m = new LinkedHashMap<>();
            m.put("class", e.getClass().getName());
            m.put("message", e.getMessage());
            out.add(m);
        }
        return out;
    }

    private static String env(String name, String fallback) {
        String v = System.getenv(name);
        return v == null || v.isEmpty() ? fallback : v;
    }
}
