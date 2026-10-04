package com.soapboxrace.reference;

import java.time.Clock;
import java.time.Instant;
import java.time.ZoneId;
import java.time.ZoneOffset;
import java.util.Random;
import java.util.UUID;

/**
 * The only sources of randomness, time and UUIDs in the patched reference build
 * (see reference/prepare.py). Every scenario resets them, so recordings are
 * byte-for-byte reproducible and sbrw-rs can replay them with the same seed.
 */
public final class Determinism {
    private static Random random = new Random(0);
    private static long uuidCounter;
    private static final MutableClock CLOCK = new MutableClock();

    private Determinism() {
    }

    public static void reset(long seed, Instant now) {
        random = new Random(seed);
        uuidCounter = 0;
        CLOCK.now = now;
    }

    /** Replaces every {@code new Random()} and {@code Math.random()}. */
    public static Random random() {
        return random;
    }

    /** commons-lang3 3.9 {@code RandomUtils.nextInt(int, int)}, drawing from the shared Random. */
    public static int nextInt(int startInclusive, int endExclusive) {
        if (endExclusive < startInclusive || startInclusive < 0) {
            throw new IllegalArgumentException("invalid range " + startInclusive + ".." + endExclusive);
        }
        if (startInclusive == endExclusive) {
            return startInclusive;
        }
        return startInclusive + random.nextInt(endExclusive - startInclusive);
    }

    /** Version-4-shaped UUIDs from a counter: 00000000-0000-4000-8000-000000000001, ... */
    public static UUID uuid() {
        uuidCounter++;
        return new UUID(0x0000000000004000L, 0x8000000000000000L | uuidCounter);
    }

    public static Clock clock() {
        return CLOCK;
    }

    public static long millis() {
        return CLOCK.now.toEpochMilli();
    }

    public static Instant now() {
        return CLOCK.now;
    }

    public static void advance(long millis) {
        CLOCK.now = CLOCK.now.plusMillis(millis);
    }

    private static final class MutableClock extends Clock {
        private Instant now = Instant.EPOCH;

        @Override
        public ZoneId getZone() {
            return ZoneOffset.UTC;
        }

        @Override
        public Clock withZone(ZoneId zone) {
            throw new UnsupportedOperationException("the reference clock is always UTC");
        }

        @Override
        public Instant instant() {
            return now;
        }
    }
}
