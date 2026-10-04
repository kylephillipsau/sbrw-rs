package com.soapboxrace.reference;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;

import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;

/**
 * Test vectors for JDK behaviour the port must reproduce bit for bit, written
 * to fixtures/vectors/*.json: {@code java.util.Random} (which every loot roll
 * goes through) and {@code Collections.shuffle}.
 */
public final class Vectors {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();

    public static void main(String[] args) throws Exception {
        Path out = Paths.get(args[0]).resolve("fixtures/vectors");
        Files.createDirectories(out);
        write(out.resolve("java_random.json"), javaRandom());
    }

    private static Map<String, Object> javaRandom() {
        Map<String, Object> root = new LinkedHashMap<>();
        root.put("about", "java.util.Random outputs from " + System.getProperty("java.vendor") + " "
                + System.getProperty("java.version"));
        List<Map<String, Object>> cases = new ArrayList<>();
        long[] seeds = {0L, 1L, -1L, 42L, 123456789L, Long.MIN_VALUE, Long.MAX_VALUE, 0x5DEECE66DL};
        int[] bounds = {1, 2, 3, 7, 10, 16, 100, 1000, 1 << 30, (1 << 30) + 1, Integer.MAX_VALUE};
        for (long seed : seeds) {
            Map<String, Object> c = new LinkedHashMap<>();
            c.put("seed", Long.toString(seed));
            Random r = new Random(seed);
            List<Integer> ints = new ArrayList<>();
            for (int i = 0; i < 8; i++) ints.add(r.nextInt());
            c.put("next_int", ints);

            r = new Random(seed);
            List<List<Integer>> bounded = new ArrayList<>();
            for (int bound : bounds) {
                List<Integer> draws = new ArrayList<>();
                draws.add(bound);
                for (int i = 0; i < 6; i++) draws.add(r.nextInt(bound));
                bounded.add(draws);
            }
            c.put("next_int_bounded", bounded);

            r = new Random(seed);
            List<String> longs = new ArrayList<>();
            for (int i = 0; i < 4; i++) longs.add(Long.toString(r.nextLong()));
            c.put("next_long", longs);

            r = new Random(seed);
            List<String> doubles = new ArrayList<>();
            for (int i = 0; i < 6; i++) doubles.add(Long.toHexString(Double.doubleToRawLongBits(r.nextDouble())));
            c.put("next_double_bits", doubles);

            r = new Random(seed);
            List<String> floats = new ArrayList<>();
            for (int i = 0; i < 6; i++) floats.add(Integer.toHexString(Float.floatToRawIntBits(r.nextFloat())));
            c.put("next_float_bits", floats);

            r = new Random(seed);
            List<Boolean> bools = new ArrayList<>();
            for (int i = 0; i < 8; i++) bools.add(r.nextBoolean());
            c.put("next_boolean", bools);

            r = new Random(seed);
            List<Integer> list = new ArrayList<>();
            for (int i = 0; i < 10; i++) list.add(i);
            Collections.shuffle(list, r);
            c.put("shuffle_0_to_9", list);
            c.put("after_shuffle_next_int", r.nextInt());
            cases.add(c);
        }
        root.put("cases", cases);
        return root;
    }

    private static void write(Path file, Object value) throws Exception {
        try (Writer w = Files.newBufferedWriter(file, StandardCharsets.UTF_8)) {
            GSON.toJson(value, w);
            w.write('\n');
        }
        System.out.println("wrote " + file);
    }
}
