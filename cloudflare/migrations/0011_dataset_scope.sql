-- Separate a complete catalog release from a rights-safe public metadata
-- projection. Only a complete catalog may approve a game Build for personal
-- save/profile projection. A public metadata dataset may still publish the
-- reviewed Wiki and graph without making that stronger compatibility claim.
ALTER TABLE data_versions
ADD COLUMN dataset_scope TEXT NOT NULL DEFAULT 'full_catalog'
    CHECK (dataset_scope IN ('full_catalog', 'public_metadata'));

DROP INDEX idx_one_active_verified_dataset;

CREATE UNIQUE INDEX idx_one_active_verified_dataset
ON data_versions(dataset_scope)
WHERE activated_at IS NOT NULL AND verified = 1;

CREATE INDEX idx_data_versions_scope_activation
ON data_versions(dataset_scope, verified, activated_at DESC);

-- Dataset activation and publication triggers are reconciled by
-- schema/post_migration_triggers.sql after migrations complete.
