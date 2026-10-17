package com.soapboxrace.reference;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;

import com.soapboxrace.jaxb.login.LoginStatusVO;
import com.soapboxrace.jaxb.util.JAXBUtility;
import org.apache.commons.validator.routines.DomainValidator;
import org.apache.commons.validator.routines.EmailValidator;

import java.io.Writer;
import java.nio.charset.Charset;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.format.DateTimeFormatter;
import java.time.format.FormatStyle;
import java.util.Arrays;
import java.util.Locale;
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
        write(out.resolve("jaxb_escaping.json"), jaxbEscaping());
        write(out.resolve("email_validator.json"), emailValidator());
        write(out.resolve("ban_expires.json"), banExpires());
        write(out.resolve("java_number_to_string.json"), numberToString());
        write(out.resolve("nashorn.json"), NashornVectors.vectors());
        write(out.resolve("java_multimap_order.json"), multimapOrder());
        write(out.resolve("java_weight_sum.json"), weightSum());
    }

    /** Float.toString and Double.toString, which JAXB uses for xs:float and xs:double. */
    private static Map<String, Object> numberToString() {
        Map<String, Object> root = about("Float.toString / Double.toString");
        float[] floats = {0f, -0f, 1f, -1f, 0.1f, 0.001f, 0.0009999f, 1.0E7f, 9999999f, 1.0E-5f, 123456.78f, 1.2345678E-4f,
                3.4028235E38f, Float.MIN_VALUE, Float.MIN_NORMAL, 1.17549421E-38f, 650.5f, 2.5E-5f, 100f, 1.0E10f,
                Float.NaN, Float.POSITIVE_INFINITY, Float.NEGATIVE_INFINITY, 0.3f, 2f / 3f, 16777216f, 16777217f, 3.0E-45f};
        double[] doubles = {0d, -0d, 1d, -1d, 0.1, 0.001, 9.999999E-4, 1.0E7, 9999999.999, 1.0E-5, 1.0 / 3.0, 2.0 / 3.0,
                Double.MAX_VALUE, Double.MIN_VALUE, Double.MIN_NORMAL, 1.0E23, 2.0E23, 8.41E21, 5.0E-324, 9.0E-324,
                2.82879384806159E17, 1.18575755E-316, 2.2250738585072014E-308, 350000d, 9.223372036854776E18, 0.3,
                Double.NaN, Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY, 4.35E-5, 1.0E-3, 123456789012345680d};
        Random r = new Random(42);
        List<List<String>> fl = new ArrayList<>();
        for (float f : floats) fl.add(Arrays.asList(Integer.toHexString(Float.floatToRawIntBits(f)), Float.toString(f)));
        for (int i = 0; i < 20000; i++) {
            float f = Float.intBitsToFloat(r.nextInt());
            if (Float.isNaN(f)) continue;
            fl.add(Arrays.asList(Integer.toHexString(Float.floatToRawIntBits(f)), Float.toString(f)));
        }
        for (int i = 0; i < 2000; i++) {
            float f = (r.nextInt(2_000_000) - 1_000_000) / (float) Math.pow(10, r.nextInt(7));
            fl.add(Arrays.asList(Integer.toHexString(Float.floatToRawIntBits(f)), Float.toString(f)));
        }
        List<List<String>> dl = new ArrayList<>();
        for (double d : doubles) dl.add(Arrays.asList(Long.toHexString(Double.doubleToRawLongBits(d)), Double.toString(d)));
        for (int i = 0; i < 20000; i++) {
            double d = Double.longBitsToDouble(r.nextLong());
            if (Double.isNaN(d)) continue;
            dl.add(Arrays.asList(Long.toHexString(Double.doubleToRawLongBits(d)), Double.toString(d)));
        }
        for (int i = 0; i < 2000; i++) {
            double d = (r.nextInt(2_000_000_000) - 1_000_000_000) / Math.pow(10, r.nextInt(10));
            dl.add(Arrays.asList(Long.toHexString(Double.doubleToRawLongBits(d)), Double.toString(d)));
        }
        root.put("float_bits_and_string", fl);
        root.put("double_bits_and_string", dl);
        return root;
    }

    private static Map<String, Object> about(String what) {
        Map<String, Object> root = new LinkedHashMap<>();
        root.put("about", what + " on " + System.getProperty("java.vendor") + " " + System.getProperty("java.version")
                + ", locale " + Locale.getDefault() + ", charset " + Charset.defaultCharset()
                + ", zone " + ZoneId.systemDefault());
        return root;
    }

    /**
     * The order {@code ArrayListMultimap.create().entries()} gives, which CommerceBO checks and
     * prices a basket's parts in: keys put in turn, and the indexes of the puts in entry order.
     */
    private static Map<String, Object> multimapOrder() {
        Map<String, Object> root = about("Guava 28.1 ArrayListMultimap.create().entries()");
        Random r = new Random(7);
        List<Map<String, Object>> cases = new ArrayList<>();
        for (int n = 0; n < 300; n++) {
            int size = n < 60 ? n : 1 + r.nextInt(80);
            com.google.common.collect.ArrayListMultimap<Integer, Integer> m = com.google.common.collect.ArrayListMultimap.create();
            List<Integer> keys = new ArrayList<>();
            for (int i = 0; i < size; i++) {
                int key;
                switch (r.nextInt(4)) {
                    case 0:
                        key = r.nextInt(64);
                        break;
                    case 1:
                        key = keys.isEmpty() ? r.nextInt() : keys.get(r.nextInt(keys.size()));
                        break;
                    default:
                        key = r.nextInt();
                }
                keys.add(key);
                m.put(key, i);
            }
            List<Integer> order = new ArrayList<>();
            for (Map.Entry<Integer, Integer> e : m.entries()) order.add(e.getValue());
            Map<String, Object> c = new LinkedHashMap<>();
            c.put("keys", keys);
            c.put("order", order);
            cases.add(c);
        }
        root.put("cases", cases);
        return root;
    }

    /**
     * How ItemRewardBO sums drop weights: a {@code HashMap<Long, Double>} from row id to weight,
     * {@code weights.values().stream().mapToDouble(d -> d).sum()}. Each case gives the ids and weights
     * in the order they were put, the order {@code values()} gives back (as indexes), and the sum's bits.
     */
    private static Map<String, Object> weightSum() {
        Map<String, Object> root = about("HashMap<Long, Double>.values() order and DoubleStream.sum()");
        Random r = new Random(11);
        double[] common = {0.1, 0.55, 0.04, 0.2, 0.001, 0.009, 0.25, 0.05, 0.3, 0.15, 0.6, 0.02, 0.005, 0.0001, 1e-7};
        List<Map<String, Object>> cases = new ArrayList<>();
        for (int n = 0; n < 400; n++) {
            int size = 1 + r.nextInt(n < 200 ? 12 : 120);
            long base = r.nextInt(5) == 0 ? r.nextInt(Integer.MAX_VALUE) * 3L : 1 + r.nextInt(5000);
            java.util.HashMap<Long, Double> weights = new java.util.HashMap<>();
            List<Long> ids = new ArrayList<>();
            List<String> put = new ArrayList<>();
            double defaultWeight = 1.0d / size;
            for (int i = 0; i < size; i++) {
                long id = base + i * (1 + r.nextInt(3)) + (r.nextInt(10) == 0 ? (1L << 32) : 0);
                double w;
                switch (r.nextInt(4)) {
                    case 0:
                        w = defaultWeight;
                        break;
                    case 1:
                        w = r.nextDouble();
                        break;
                    default:
                        w = common[r.nextInt(common.length)];
                }
                ids.add(id);
                put.add(Long.toHexString(Double.doubleToRawLongBits(w)));
                weights.put(id, w);
            }
            List<Integer> order = new ArrayList<>();
            for (Long id : weights.keySet()) order.add(ids.indexOf(id));
            double sum = weights.values().stream().mapToDouble(d -> d).sum();
            Map<String, Object> c = new LinkedHashMap<>();
            c.put("ids", ids);
            c.put("weight_bits", put);
            c.put("order", order);
            c.put("sum_bits", Long.toHexString(Double.doubleToRawLongBits(sum)));
            cases.add(c);
        }
        root.put("cases", cases);
        return root;
    }

    /** How JAXBUtility.marshal escapes text, using LoginStatusVO as the carrier. */
    private static Map<String, Object> jaxbEscaping() {
        Map<String, Object> root = about("JAXBUtility.marshal(LoginStatusVO)");
        String[] inputs = {"", "plain", "a & b < c > d", "quote \" apostrophe '", "cr\rlf\ntab\t",
                "unicode \u00e9 \u6f22\u5b57 \ud83d\ude00", "control \u0001\u001f\u007f", "]]> end", "&amp; already"};
        List<Map<String, Object>> cases = new ArrayList<>();
        for (String input : inputs) {
            LoginStatusVO vo = new LoginStatusVO(7L, "token", true);
            vo.setDescription(input);
            Map<String, Object> c = new LinkedHashMap<>();
            c.put("description", input);
            c.put("xml", JAXBUtility.marshal(vo));
            cases.add(c);
        }
        LoginStatusVO banned = new LoginStatusVO(0L, "", false);
        banned.setDescription("");
        banned.setBan(new LoginStatusVO.Ban("Cheating", "Friday, January 2, 2026"));
        Map<String, Object> c = new LinkedHashMap<>();
        c.put("description", "");
        c.put("ban", Arrays.asList("Cheating", "Friday, January 2, 2026"));
        c.put("xml", JAXBUtility.marshal(banned));
        cases.add(c);
        LoginStatusVO nulls = new LoginStatusVO(null, null, false);
        c = new LinkedHashMap<>();
        c.put("nulls", true);
        c.put("xml", JAXBUtility.marshal(nulls));
        cases.add(c);
        root.put("cases", cases);
        return root;
    }

    /** commons-validator 1.7 EmailValidator.getInstance() and the TLD lists it relies on. */
    private static Map<String, Object> emailValidator() {
        Map<String, Object> root = about("commons-validator EmailValidator.getInstance().isValid");
        String a64 = "a".repeat(64);
        String label63 = "b".repeat(63);
        String[] inputs = {
                "player@example.com", "a@b.co", "a@b.c", "x@x.x", "user@localhost", "user@example",
                "user@192.168.0.1", "user@[192.168.0.1]", "user@[300.1.1.1]", "user@[::1]", "user@[IPv6:::1]",
                "user.name+tag@example.co.uk", ".user@example.com", "user.@example.com", "us..er@example.com",
                "user@-example.com", "user@example-.com", "user@exa_mple.com", "user@xn--d1acufc.xn--p1ai",
                "user@\u043f\u0440\u0438\u043c\u0435\u0440.\u0440\u0444", "\u00fcser@example.com",
                "USER@EXAMPLE.COM", " user@example.com", "user@example.com ", "user@@example.com", "@example.com",
                "user@", "", "user@example.c0m", "\"quoted\"@example.com", "\"quoted user\"@example.com",
                "\"a\\\"b\"@example.com", "user@sub.example.museum", "user@example.web", "user@example.invalidtld",
                a64 + "@example.com", a64 + "a@example.com", "user@" + label63 + ".com", "user@" + label63 + "b.com",
                "user@example..com", "user@.example.com", "user@example.com.", "user!#$%&'*+/=?^_`{|}~@example.com",
                "user(comment)@example.com", "user@example.com\n", "user name@example.com", "user@exam ple.com",
                "test@example.local", "test@example.onion", "test@example.photography", "test@123.com",
                "test@example.123", "test@example.arpa", "test@example.localhost", "test@EXAMPLE.Com",
                "test@example.co.u", "a@b.cc", "\u00e9@example.com", "test@\u00e9xample.com", "test@example.xn--vermgensberater-ctb"};
        List<Map<String, Object>> cases = new ArrayList<>();
        EmailValidator v = EmailValidator.getInstance();
        for (String input : inputs) {
            Map<String, Object> c = new LinkedHashMap<>();
            c.put("email", input);
            c.put("valid", v.isValid(input));
            cases.add(c);
        }
        root.put("cases", cases);
        Map<String, Object> tlds = new LinkedHashMap<>();
        for (DomainValidator.ArrayType t : new DomainValidator.ArrayType[]{DomainValidator.ArrayType.GENERIC_RO,
                DomainValidator.ArrayType.COUNTRY_CODE_RO, DomainValidator.ArrayType.INFRASTRUCTURE_RO,
                DomainValidator.ArrayType.LOCAL_RO}) {
            tlds.put(t.name(), Arrays.asList(DomainValidator.getTLDEntries(t)));
        }
        root.put("tlds", tlds);
        return root;
    }

    /** TokenSessionBO.login's ban expiry: ofLocalizedDateTime(FULL) in the system zone. */
    private static Map<String, Object> banExpires() {
        Map<String, Object> root = about("DateTimeFormatter.ofLocalizedDateTime(FormatStyle.FULL).withZone(ZoneId.systemDefault())");
        DateTimeFormatter f = DateTimeFormatter.ofLocalizedDateTime(FormatStyle.FULL).withZone(ZoneId.systemDefault());
        List<Map<String, Object>> cases = new ArrayList<>();
        for (String t : new String[]{"2026-01-01T00:00:00", "2026-01-02T12:00:00", "2026-03-09T09:05:07",
                "2026-12-31T23:59:59", "2027-07-04T13:30:00", "2028-02-29T00:00:01"}) {
            Map<String, Object> c = new LinkedHashMap<>();
            c.put("ends_at", t);
            c.put("expires", f.format(LocalDateTime.parse(t)));
            cases.add(c);
        }
        root.put("cases", cases);
        return root;
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
