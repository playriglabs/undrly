-- V1 gold's unit of measure (docs/v1.1-universe.md §8).
--
-- The V1 curated universe predates `instruments.unit_of_measure` (0011). Its
-- gold instrument is "Gold (troy ounce)", priced by gold-api's XAU series per
-- troy ounce, exactly as the curated silver, platinum and palladium are. The
-- curated file now states `troy_ounce` for gold; this backfills databases
-- seeded before that, so re-seeding them finds the same fact. A no-op on a
-- fresh database, where the seed writes the unit itself. Pinned V1 id
-- (undrly:instrument:01m3bbjhnjewwb8qvvkn6rf2c1); only fills a missing value.
UPDATE instruments
SET unit_of_measure = 'troy_ounce'
WHERE id = '01a0d6b9-46b2-7738-b45f-7b9d4d878981'
  AND unit_of_measure IS NULL;
