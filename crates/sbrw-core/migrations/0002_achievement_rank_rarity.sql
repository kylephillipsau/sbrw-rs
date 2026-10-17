-- sbrw-rs's own table. Upstream's rarity job writes each achievement rank's
-- rarity into the static `achievement_rank` table, which MySQL kept across
-- restarts; sbrw-rs rebuilds the static data at every start, so the job also
-- keeps the rarities here, and they are copied back over the static rows at
-- startup.
CREATE TABLE "achievement_rank_rarity" (
    "achievement_rank_id" INTEGER PRIMARY KEY,
    "rarity" REAL NOT NULL
);
