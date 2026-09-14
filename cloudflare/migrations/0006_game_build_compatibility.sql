-- Game updates must not require a desktop or Worker rebuild. Public catalog
-- reads use the active verified dataset without validating a visitor's game.
-- Personal projections are gated by this remotely editable compatibility map.
CREATE TABLE game_build_policies (
    game_build_id TEXT PRIMARY KEY,
    compatibility_state TEXT NOT NULL
        CHECK (compatibility_state IN ('observed', 'compatible', 'blocked')),
    dataset_version TEXT REFERENCES data_versions(dataset_version) ON DELETE SET NULL,
    projection_schema TEXT NOT NULL DEFAULT 'cloud-profile-projection-v1',
    observation_count INTEGER NOT NULL DEFAULT 0 CHECK (observation_count >= 0),
    last_parser_version TEXT,
    last_requested_dataset_version TEXT,
    notes TEXT,
    first_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    reviewed_by_principal_id TEXT REFERENCES principals(principal_id) ON DELETE SET NULL,
    reviewed_at TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (
        compatibility_state != 'compatible'
        OR dataset_version IS NOT NULL
    )
);

CREATE INDEX idx_game_build_policy_state
ON game_build_policies(compatibility_state, last_seen_at DESC);

-- Preserve compatibility for a dataset that was activated before this
-- migration was applied.
INSERT INTO game_build_policies (
    game_build_id,
    compatibility_state,
    dataset_version,
    projection_schema,
    notes,
    reviewed_at
)
SELECT
    CASE
        WHEN game_build_id LIKE 'steam:%' THEN substr(game_build_id, 7)
        ELSE game_build_id
    END,
    'compatible',
    dataset_version,
    'cloud-profile-projection-v1',
    'Automatically approved from an active verified dataset',
    CURRENT_TIMESTAMP
FROM data_versions
WHERE verified = 1 AND activated_at IS NOT NULL
ON CONFLICT(game_build_id) DO UPDATE SET
    compatibility_state = 'compatible',
    dataset_version = excluded.dataset_version,
    projection_schema = excluded.projection_schema,
    notes = excluded.notes,
    reviewed_at = excluded.reviewed_at,
    updated_at = CURRENT_TIMESTAMP;

-- Dataset activation compatibility triggers are reconciled by
-- schema/post_migration_triggers.sql after migrations complete.
