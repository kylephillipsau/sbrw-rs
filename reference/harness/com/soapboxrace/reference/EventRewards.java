package com.soapboxrace.reference;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.soapboxrace.core.bo.ParameterBO;
import com.soapboxrace.core.bo.RewardBO;
import com.soapboxrace.core.bo.util.RewardVO;
import com.soapboxrace.core.jpa.EventEntity;
import com.soapboxrace.core.jpa.EventRewardEntity;
import com.soapboxrace.core.jpa.PersonaEntity;
import com.soapboxrace.jaxb.http.EnumRewardType;
import com.soapboxrace.jaxb.http.PursuitArbitrationPacket;
import com.soapboxrace.jaxb.http.RewardPart;
import com.soapboxrace.jaxb.http.RouteArbitrationPacket;

import javax.persistence.EntityManager;
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
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;

/**
 * The reward arithmetic of {@code RewardBO}, into fixtures/vectors/event_rewards.json: the chains of
 * public methods the event reward BOs call (a sprint's, and a pursuit's statistics), on every
 * {@code event_reward} row and on random configurations, for random levels, ranks, times, top
 * speeds and packets, under several parameter sets.
 *
 * <pre>java ... com.soapboxrace.reference.EventRewards &lt;repo&gt;</pre>
 *
 * Skill mods, amplifiers and paying out need a driver in the database; the scenarios cover those.
 */
public final class EventRewards {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().serializeNulls().create();

    /** Parameter overrides for each set, applied over the data's own. */
    private static final List<Map<String, String>> PARAMETER_SETS = List.of(
            Map.of(),
            Map.of("REP_REWARD_MULTIPLIER", "1.5", "CASH_REWARD_MULTIPLIER", "0.75",
                    "REWARD_REP_BASELINE_LEVEL", "2", "REWARD_CASH_BASELINE_LEVEL", "5"),
            Map.of("REP_REWARD_MULTIPLIER", "0.3", "CASH_REWARD_MULTIPLIER", "2.7",
                    "PURSUIT_COP_CARS_DEPLOYED_REP_MULTIPLIER", "0.033", "PURSUIT_HEAT_LEVEL_CASH_MULTIPLIER", "0.37",
                    "PURSUIT_PURSUIT_LENGTH_REP_MULTIPLIER", "0.0000123", "PURSUIT_COST_TO_STATE_CASH_MULTIPLIER", "0.0007"));

    private static final EnumRewardType[] PURSUIT_TYPES = {
            EnumRewardType.COP_CARS_DEPLOYED, EnumRewardType.COP_CARS_DISABLED, EnumRewardType.COP_CARS_RAMMED,
            EnumRewardType.COST_TO_STATE, EnumRewardType.PURSUIT_LENGTH, EnumRewardType.HEAT_LEVEL,
            EnumRewardType.INFRACTIONS, EnumRewardType.ROADBLOCKS_DODGED, EnumRewardType.SPIKE_STRIPS_DODGED};

    public static void main(String[] args) throws Exception {
        Path repo = Paths.get(args[0]).toAbsolutePath().normalize();
        String url = env("REFERENCE_JDBC_URL", "jdbc:mysql://mysql:3306/SOAPBOX?useSSL=false&allowPublicKeyRetrieval=true");
        String password = env("REFERENCE_DB_PASSWORD", "reference");
        Map<String, Object> jpa = new HashMap<>();
        jpa.put("javax.persistence.jdbc.url", url);
        jpa.put("javax.persistence.jdbc.user", "root");
        jpa.put("javax.persistence.jdbc.password", password);

        List<Map<String, Object>> cases = new ArrayList<>();
        List<Map<String, Object>> configMaps = new ArrayList<>();
        try (Db db = new Db(url, "root", password)) {
            db.reset(Collections.emptyMap());
            Determinism.reset(1, Instant.parse("2026-01-01T00:00:00Z"));
            EntityManagerFactory emf = Persistence.createEntityManagerFactory("soapboxrace-pu", jpa);
            Fakes.recording = new Recording();
            Fakes.personaGroups = Collections.emptyMap();
            Container container = new Container(emf, Fakes.recording, CoreClasses.load(), Fakes.substitutes());
            container.boot();
            RewardBO rewardBO = container.get(RewardBO.class);
            ParameterBO parameterBO = container.get(ParameterBO.class);

            List<EventRewardEntity> configs = new ArrayList<>();
            EntityManager em = emf.createEntityManager();
            configs.addAll(em.createQuery("SELECT obj FROM EventRewardEntity obj ORDER BY obj.id", EventRewardEntity.class)
                    .getResultList());
            em.close();
            Random r = new Random(11);
            for (int i = 0; i < 60; i++) configs.add(randomConfig(r, "random_" + i));

            for (EventRewardEntity config : configs) configMaps.add(configMap(config));
            for (int set = 0; set < PARAMETER_SETS.size(); set++) {
                db.reset(PARAMETER_SETS.get(set));
                parameterBO.loadParameters();
                for (int index = 0; index < configs.size(); index++) {
                    for (int n = 0; n < 3; n++) {
                        cases.add(route(rewardBO, r, set, index, configs.get(index)));
                        cases.add(pursuit(rewardBO, r, set, index, configs.get(index)));
                    }
                }
            }
            db.reset(Collections.emptyMap());
            emf.close();
        }

        Map<String, Object> root = new LinkedHashMap<>();
        root.put("about", "RewardBO's arithmetic (reference/harness/.../EventRewards.java): setBaseReward, then a"
                + " sprint's setRankReward, setPerfectStartReward, setTopSpeedReward and setMultiplierReward, or a"
                + " pursuit's setPursitParamReward for each statistic, setTopSpeedReward and setMultiplierReward;"
                + " parameter_set indexes the overrides listed in parameter_sets, config the configurations in"
                + " configs (every event_reward row, then random ones)");
        root.put("upstream", CoreClasses.UPSTREAM);
        root.put("parameter_sets", PARAMETER_SETS);
        root.put("configs", configMaps);
        root.put("cases", cases);
        Path out = repo.resolve("fixtures/vectors/event_rewards.json");
        try (Writer w = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
            GSON.toJson(root, w);
            w.write('\n');
        }
        System.out.println("wrote " + cases.size() + " event reward cases");
        System.exit(0);
    }

    private static EventRewardEntity randomConfig(Random r, String id) {
        EventRewardEntity c = new EventRewardEntity();
        c.setId(id);
        c.setBaseRepReward(r.nextInt(2000));
        c.setBaseCashReward(r.nextInt(2000));
        c.setLevelRepRewardMultiplier(r.nextFloat() * 3);
        c.setLevelCashRewardMultiplier(r.nextFloat() * 3);
        c.setFinalRepRewardMultiplier(r.nextFloat());
        c.setFinalCashRewardMultiplier(r.nextFloat());
        c.setPerfectStartRepMultiplier(r.nextFloat());
        c.setPerfectStartCashMultiplier(r.nextFloat());
        c.setTopSpeedRepMultiplier(r.nextFloat());
        c.setTopSpeedCashMultiplier(r.nextFloat());
        c.setRank1RepMultiplier(r.nextFloat() * 2);
        c.setRank2RepMultiplier(r.nextFloat() * 2);
        c.setRank3RepMultiplier(r.nextFloat() * 2);
        c.setRank4RepMultiplier(r.nextFloat() * 2);
        c.setRank5RepMultiplier(r.nextFloat() * 2);
        c.setRank6RepMultiplier(r.nextFloat() * 2);
        c.setRank7RepMultiplier(r.nextFloat() * 2);
        c.setRank8RepMultiplier(r.nextFloat() * 2);
        c.setRank1CashMultiplier(r.nextFloat() * 2);
        c.setRank2CashMultiplier(r.nextFloat() * 2);
        c.setRank3CashMultiplier(r.nextFloat() * 2);
        c.setRank4CashMultiplier(r.nextFloat() * 2);
        c.setRank5CashMultiplier(r.nextFloat() * 2);
        c.setRank6CashMultiplier(r.nextFloat() * 2);
        c.setRank7CashMultiplier(r.nextFloat() * 2);
        c.setRank8CashMultiplier(r.nextFloat() * 2);
        c.setMinTopSpeedTrigger(r.nextFloat() * 100);
        return c;
    }

    private static long timeLimit(Random r) {
        long[] limits = {0, 20000, 150000, 310000, 999999, 3600000};
        return r.nextInt(4) == 0 ? r.nextInt(5_000_000) : limits[r.nextInt(limits.length)];
    }

    private static long duration(Random r, long limit) {
        switch (r.nextInt(6)) {
            case 0: return 0;
            case 1: return limit;
            case 2: return limit + 1;
            case 3: return Math.max(0, limit - 1);
            default: return r.nextInt(4_000_000);
        }
    }

    private static float topSpeed(Random r, float trigger) {
        switch (r.nextInt(4)) {
            case 0: return trigger;
            case 1: return Math.nextDown(trigger);
            default: return r.nextFloat() * 150;
        }
    }

    private static Map<String, Object> setup(Random r, int set, int config, PersonaEntity persona, EventEntity event,
                                             Map<String, Object> c) {
        persona.setLevel(1 + r.nextInt(60));
        event.setRewardsTimeLimit(timeLimit(r));
        c.put("parameter_set", set);
        c.put("config", config);
        c.put("level", persona.getLevel());
        c.put("rewards_time_limit", event.getRewardsTimeLimit());
        return c;
    }

    private static Map<String, Object> route(RewardBO rewardBO, Random r, int set, int index, EventRewardEntity config) {
        Map<String, Object> c = new LinkedHashMap<>();
        c.put("chain", "route");
        PersonaEntity persona = new PersonaEntity();
        EventEntity event = new EventEntity();
        setup(r, set, index, persona, event, c);
        RouteArbitrationPacket packet = new RouteArbitrationPacket();
        packet.setRank(r.nextInt(10));
        packet.setPerfectStart(r.nextInt(3) == 0 ? 1 : r.nextInt(3));
        packet.setTopSpeed(topSpeed(r, config.getMinTopSpeedTrigger()));
        packet.setEventDurationInMilliseconds(duration(r, event.getRewardsTimeLimit()));
        boolean economy = r.nextInt(5) != 0;
        boolean reputation = r.nextInt(5) != 0;
        c.put("rank", packet.getRank());
        c.put("perfect_start", packet.getPerfectStart());
        c.put("top_speed", packet.getTopSpeed());
        c.put("event_duration", packet.getEventDurationInMilliseconds());
        c.put("economy", economy);
        c.put("reputation", reputation);

        RewardVO vo = new RewardVO(economy, reputation);
        rewardBO.setBaseReward(persona, event, config, packet, vo);
        rewardBO.setRankReward(config, packet, vo);
        rewardBO.setPerfectStartReward(config, packet.getPerfectStart(), vo);
        rewardBO.setTopSpeedReward(config, packet.getTopSpeed(), vo);
        rewardBO.setMultiplierReward(config, vo);
        result(c, vo);
        return c;
    }

    private static Map<String, Object> pursuit(RewardBO rewardBO, Random r, int set, int index, EventRewardEntity config) {
        Map<String, Object> c = new LinkedHashMap<>();
        c.put("chain", "pursuit");
        PersonaEntity persona = new PersonaEntity();
        EventEntity event = new EventEntity();
        setup(r, set, index, persona, event, c);
        PursuitArbitrationPacket packet = new PursuitArbitrationPacket();
        packet.setEventDurationInMilliseconds(duration(r, event.getRewardsTimeLimit()));
        packet.setCopsDeployed(r.nextInt(40));
        packet.setCopsDisabled(r.nextInt(20));
        packet.setCopsRammed(r.nextInt(30));
        packet.setCostToState(r.nextInt(3_000_000));
        packet.setHeat(r.nextFloat() * 6);
        packet.setInfractions(r.nextInt(15));
        packet.setRoadBlocksDodged(r.nextInt(10));
        packet.setSpikeStripsDodged(r.nextInt(10));
        packet.setTopSpeed(topSpeed(r, config.getMinTopSpeedTrigger()));
        boolean economy = r.nextInt(5) != 0;
        boolean reputation = r.nextInt(5) != 0;
        c.put("event_duration", packet.getEventDurationInMilliseconds());
        c.put("cops_deployed", packet.getCopsDeployed());
        c.put("cops_disabled", packet.getCopsDisabled());
        c.put("cops_rammed", packet.getCopsRammed());
        c.put("cost_to_state", packet.getCostToState());
        c.put("heat", packet.getHeat());
        c.put("infractions", packet.getInfractions());
        c.put("road_blocks_dodged", packet.getRoadBlocksDodged());
        c.put("spike_strips_dodged", packet.getSpikeStripsDodged());
        c.put("top_speed", packet.getTopSpeed());
        c.put("economy", economy);
        c.put("reputation", reputation);

        RewardVO vo = new RewardVO(economy, reputation);
        rewardBO.setBaseReward(persona, event, config, packet, vo);
        float[] values = {packet.getCopsDeployed(), packet.getCopsDisabled(), packet.getCopsRammed(),
                packet.getCostToState(), packet.getEventDurationInMilliseconds(), packet.getHeat(),
                packet.getInfractions(), packet.getRoadBlocksDodged(), packet.getSpikeStripsDodged()};
        for (int i = 0; i < PURSUIT_TYPES.length; i++) {
            rewardBO.setPursitParamReward(values[i], PURSUIT_TYPES[i], vo);
        }
        rewardBO.setTopSpeedReward(config, packet.getTopSpeed(), vo);
        rewardBO.setMultiplierReward(config, vo);
        result(c, vo);
        return c;
    }

    private static Map<String, Object> configMap(EventRewardEntity e) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("id", e.getId());
        m.put("base_rep_reward", e.getBaseRepReward());
        m.put("base_cash_reward", e.getBaseCashReward());
        m.put("level_rep_reward_multiplier", e.getLevelRepRewardMultiplier());
        m.put("level_cash_reward_multiplier", e.getLevelCashRewardMultiplier());
        m.put("final_rep_reward_multiplier", e.getFinalRepRewardMultiplier());
        m.put("final_cash_reward_multiplier", e.getFinalCashRewardMultiplier());
        m.put("perfect_start_rep_multiplier", e.getPerfectStartRepMultiplier());
        m.put("perfect_start_cash_multiplier", e.getPerfectStartCashMultiplier());
        m.put("top_speed_rep_multiplier", e.getTopSpeedRepMultiplier());
        m.put("top_speed_cash_multiplier", e.getTopSpeedCashMultiplier());
        m.put("rank_rep_multipliers", List.of(e.getRank1RepMultiplier(), e.getRank2RepMultiplier(),
                e.getRank3RepMultiplier(), e.getRank4RepMultiplier(), e.getRank5RepMultiplier(),
                e.getRank6RepMultiplier(), e.getRank7RepMultiplier(), e.getRank8RepMultiplier()));
        m.put("rank_cash_multipliers", List.of(e.getRank1CashMultiplier(), e.getRank2CashMultiplier(),
                e.getRank3CashMultiplier(), e.getRank4CashMultiplier(), e.getRank5CashMultiplier(),
                e.getRank6CashMultiplier(), e.getRank7CashMultiplier(), e.getRank8CashMultiplier()));
        m.put("min_top_speed_trigger", e.getMinTopSpeedTrigger());
        return m;
    }

    private static void result(Map<String, Object> c, RewardVO vo) {
        c.put("base_rep", vo.getBaseRep());
        c.put("base_cash", vo.getBaseCash());
        c.put("rep", vo.getRep());
        c.put("cash", vo.getCash());
        // "rep cash CATEGORY TYPE", one line each.
        List<String> parts = new ArrayList<>();
        for (RewardPart p : vo.getArrayOfRewardPart().getRewardPart()) {
            parts.add(p.getRepPart() + " " + p.getTokenPart() + " " + p.getRewardCategory().name() + " "
                    + p.getRewardType().name());
        }
        c.put("parts", parts);
    }

    private static String env(String name, String fallback) {
        String v = System.getenv(name);
        return v == null || v.isEmpty() ? fallback : v;
    }
}
