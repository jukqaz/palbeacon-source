-- Additive detail fields used by the image-first catalog and native
-- comparison screens. A dataset remains inactive until a separate operator
-- verification step sets verified=1 and activated_at.
ALTER TABLE catalog_species ADD COLUMN paldex_number INTEGER;
ALTER TABLE catalog_species ADD COLUMN rarity INTEGER;
ALTER TABLE catalog_species ADD COLUMN run_speed INTEGER;
ALTER TABLE catalog_species ADD COLUMN transport_speed INTEGER;
ALTER TABLE catalog_species ADD COLUMN food_amount INTEGER;
ALTER TABLE catalog_species ADD COLUMN nocturnal INTEGER NOT NULL DEFAULT 0
    CHECK (nocturnal IN (0, 1));
ALTER TABLE catalog_species ADD COLUMN learned_skills_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE catalog_species ADD COLUMN guaranteed_passives_json TEXT NOT NULL DEFAULT '[]';

CREATE INDEX idx_catalog_species_paldex
ON catalog_species(dataset_version, paldex_number);

-- Only a verified candidate may become the one active dataset. The seed
-- generator never emits activation SQL; activation remains a separate,
-- operator-reviewed action after the exact Build gate.
DROP INDEX idx_one_active_dataset;

CREATE UNIQUE INDEX idx_one_active_verified_dataset
ON data_versions(verified)
WHERE activated_at IS NOT NULL AND verified = 1;

-- Activation guard triggers are reconciled by
-- schema/post_migration_triggers.sql after migrations complete.
