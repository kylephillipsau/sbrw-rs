package com.soapboxrace.reference;

import com.soapboxrace.core.bo.util.AchievementCommerceContext;
import com.soapboxrace.core.bo.util.AchievementProgressionContext;
import com.soapboxrace.core.jpa.CarClassesEntity;
import com.soapboxrace.core.jpa.CarEntity;
import com.soapboxrace.core.jpa.PersonaAchievementEntity;
import com.soapboxrace.core.jpa.PersonaEntity;

import javax.script.ScriptEngine;
import javax.script.ScriptEngineManager;
import javax.script.ScriptException;
import javax.script.SimpleBindings;
import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * What Nashorn makes of the scripts in upstream's data, as ScriptingBO runs them:
 * each script evaluated against named sets of bindings built from upstream's own
 * classes, with the Java class and value of the result, or the exception thrown.
 * The probes pin down how Java values behave in scripts (Long against int,
 * null, strings, bean getters); the rest are the achievement scripts themselves.
 */
final class NashornVectors {
    private NashornVectors() {
    }

    private static final String[] PROBES = {
            "1", "0", "-1", "2147483647", "2147483648", "1.5", "true", "null", "undefined", "'x'",
            "l5", "i5", "d25", "f15", "s", "n", "t", "big",
            "typeof l5", "typeof i5", "typeof d25", "typeof f15", "typeof s", "typeof n", "typeof t", "typeof persona",
            "l5 == 5", "l5 === 5", "l5 == i5", "l5 === i5", "l5 != i5", "l5 == l5b", "l5 === l5b", "l0 == 0", "l0 == false",
            "l5 > 0", "l5 < i5", "l5 >= 5", "i5 == 5", "i5 === 5", "d25 == 2.5", "f15 == 1.5",
            "l5 + 1", "l5 + i5", "l5 * 2", "l5 / 2", "l5 - 7", "i5 + 1", "i5 * 2", "i5 / 2", "i5 * d25", "d25 * 2",
            "l5 * 1609.34", "(l5 * d25) * 1609.34", "f15 * 2", "-l5", "+l5", "+t", "+f", "+(i5 == 5)", "+(i5 == 4)",
            "(l5 + 1) * +(t)", "(l5 + 1) * +(f)", "(l0 + 1) * +(i5 == 5)", "i5 * +(f)", "1 / 0", "0 / 0", "-1 / 0",
            "big + 0", "big == 9007199254740992", "big == 9007199254740993",
            "n == null", "n === null", "n == undefined", "!!n", "!n", "!!persona", "!persona",
            "s == 'DODGE'", "s === 'DODGE'", "s == \"dodge\"", "s + 1", "s.length",
            "t && f", "t || f", "!t", "t == true", "t === true",
            "n.foo", "nothing", "persona.nothing", "persona.getNothing()",
            "persona.level", "persona.getLevel()", "persona.personaId", "persona.cash", "persona.name",
            "persona.level == 60", "persona.personaId == 100", "persona.cash > 0",
            "ownedCar.expirationDate", "ownedCar.getExpirationDate()", "ownedCar.getExpirationDate() == null",
            "ownedCar.expirationDate == null", "rental.getExpirationDate() == null", "!!rental.expirationDate",
            "ownedCar.isPreset", "ownedCar.preset", "ownedCar.isPreset()",
            "commerceCtx.commerceTypeVal", "commerceCtx.getCommerceTypeVal()", "commerceCtx.commerceTypeVal == 0",
            "commerceCtx.commerceType", "commerceCtx.commerceType == 'CAR_PURCHASE'",
            "commerceCtx.carClassesEntity.manufactor", "commerceCtx.carClassesEntity.manufactor == 'DODGE'",
            "bundleCtx.carClassesEntity", "bundleCtx.carClassesEntity == null",
            "bundleCtx.carClassesEntity.manufactor == 'DODGE'",
            "personaAchievement.currentValue", "personaAchievement.currentValue + 1",
            "personaAchievement.currentValue != progression.level", "personaAchievement.currentValue != progression.score",
            "personaAchievement.currentValue == 27", "personaAchievement.canProgress",
            "progression.cash", "progression.cash > 0", "progression.level", "progression.treasureStreak",
            "progression.treasureStreakChanged", "progression.levelChanged", "progression.inEvent",
    };

    private static final String[] COMMERCE = {
            "commerceCtx.commerceTypeVal == 0 && ownedCar.getExpirationDate() == null",
            "commerceCtx.commerceTypeVal == 0 && commerceCtx.carClassesEntity.manufactor == \"DODGE\" && ownedCar.getExpirationDate() == null",
            "commerceCtx.commerceTypeVal == 0 && commerceCtx.carClassesEntity.manufactor == \"BMW\" && ownedCar.getExpirationDate() == null",
            "1",
    };

    private static final String[] PROGRESSION = {
            "progression.cash > 0", "progression.cash",
            "personaAchievement.currentValue != progression.level", "progression.level",
            "personaAchievement.currentValue != progression.score", "progression.score",
            "progression.treasureStreakChanged", "progression.treasureStreak",
            "1",
    };

    static Map<String, Object> vectors() throws Exception {
        ScriptEngine engine = new ScriptEngineManager().getEngineByName("nashorn");
        Map<String, Map<String, Object>> sets = bindingSets();
        List<Map<String, Object>> cases = new ArrayList<>();
        for (String script : PROBES) cases.add(eval(engine, "probe", sets, script));
        for (String set : new String[]{"car_purchase", "rental_purchase", "bundle_purchase"}) {
            for (String script : COMMERCE) cases.add(eval(engine, set, sets, script));
        }
        for (String set : new String[]{"progression", "progression_unchanged"}) {
            for (String script : PROGRESSION) cases.add(eval(engine, set, sets, script));
        }
        Map<String, Object> root = new LinkedHashMap<>();
        root.put("about", "ScriptingBO.eval (Nashorn, " + System.getProperty("java.version")
                + "): each script against a named set of bindings (see NashornVectors.java). "
                + "class and value of the result, or the exception thrown.");
        root.put("cases", cases);
        return root;
    }

    private static Map<String, Object> eval(ScriptEngine engine, String set, Map<String, Map<String, Object>> sets,
                                            String script) {
        Map<String, Object> c = new LinkedHashMap<>();
        c.put("bindings", set);
        c.put("script", script);
        try {
            Object result = engine.eval(script, new SimpleBindings(new LinkedHashMap<>(sets.get(set))));
            c.put("class", result == null ? null : result.getClass().getName());
            c.put("value", result == null ? null : String.valueOf(result));
        } catch (ScriptException | RuntimeException e) {
            c.put("error", e.getClass().getName());
        }
        return c;
    }

    private static Map<String, Map<String, Object>> bindingSets() {
        Map<String, Map<String, Object>> sets = new LinkedHashMap<>();

        Map<String, Object> probe = new LinkedHashMap<>();
        probe.put("l0", 0L);
        probe.put("l5", 5L);
        probe.put("l5b", 5L);
        probe.put("i5", 5);
        probe.put("d25", 2.5d);
        probe.put("f15", 1.5f);
        probe.put("s", "DODGE");
        probe.put("n", null);
        probe.put("t", Boolean.TRUE);
        probe.put("f", Boolean.FALSE);
        probe.put("big", 9007199254740993L);
        probe.put("persona", persona());
        probe.put("ownedCar", car(null));
        probe.put("rental", car(LocalDateTime.of(2026, 1, 2, 12, 34, 56)));
        probe.put("commerceCtx", carPurchase("DODGE"));
        probe.put("bundleCtx", new AchievementCommerceContext(AchievementCommerceContext.CommerceType.BUNDLE_PURCHASE));
        probe.put("personaAchievement", personaAchievement(27L));
        probe.put("progression", new AchievementProgressionContext(100, 0, 60, 27, 3, false, true, true, false));
        sets.put("probe", probe);

        Map<String, Object> carPurchase = new LinkedHashMap<>();
        carPurchase.put("persona", persona());
        carPurchase.put("ownedCar", car(null));
        carPurchase.put("commerceCtx", carPurchase("DODGE"));
        carPurchase.put("personaAchievement", personaAchievement(0L));
        sets.put("car_purchase", carPurchase);

        Map<String, Object> rentalPurchase = new LinkedHashMap<>(carPurchase);
        rentalPurchase.put("ownedCar", car(LocalDateTime.of(2026, 1, 2, 12, 34, 56)));
        rentalPurchase.put("personaAchievement", personaAchievement(3L));
        sets.put("rental_purchase", rentalPurchase);

        // As BasketBO.buyBundle binds it: no car.
        Map<String, Object> bundlePurchase = new LinkedHashMap<>();
        bundlePurchase.put("persona", persona());
        bundlePurchase.put("commerceCtx", new AchievementCommerceContext(AchievementCommerceContext.CommerceType.BUNDLE_PURCHASE));
        bundlePurchase.put("personaAchievement", personaAchievement(0L));
        sets.put("bundle_purchase", bundlePurchase);

        Map<String, Object> progression = new LinkedHashMap<>();
        progression.put("persona", persona());
        progression.put("progression", new AchievementProgressionContext(1500, 0, 60, 27, 4, true, true, true, true));
        progression.put("personaAchievement", personaAchievement(1L));
        sets.put("progression", progression);

        Map<String, Object> unchanged = new LinkedHashMap<>();
        unchanged.put("persona", persona());
        unchanged.put("progression", new AchievementProgressionContext(0, 0, 60, 27, 0, false, true, false, false));
        unchanged.put("personaAchievement", personaAchievement(60L));
        sets.put("progression_unchanged", unchanged);
        return sets;
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

    private static AchievementCommerceContext carPurchase(String manufactor) {
        CarClassesEntity carClasses = new CarClassesEntity();
        carClasses.setManufactor(manufactor);
        AchievementCommerceContext ctx = new AchievementCommerceContext(AchievementCommerceContext.CommerceType.CAR_PURCHASE);
        ctx.setCarClassesEntity(carClasses);
        return ctx;
    }

    private static PersonaAchievementEntity personaAchievement(Long currentValue) {
        PersonaAchievementEntity pa = new PersonaAchievementEntity();
        pa.setCanProgress(true);
        pa.setCurrentValue(currentValue);
        return pa;
    }
}
