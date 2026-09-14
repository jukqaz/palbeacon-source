-- Searchable active-skill catalog. All rows share the same verified dataset
-- publication gate as species, passives and breeding rules.
CREATE TABLE catalog_active_skills (
    dataset_version TEXT NOT NULL REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    skill_id TEXT NOT NULL,
    name_ko TEXT NOT NULL,
    description_ko TEXT,
    element TEXT,
    skill_kind TEXT,
    power INTEGER,
    min_range INTEGER,
    max_range INTEGER,
    cool_time REAL,
    effects_json TEXT NOT NULL DEFAULT '[]',
    source_ref TEXT,
    PRIMARY KEY (dataset_version, skill_id)
);

CREATE INDEX idx_catalog_active_skills_name_ko
ON catalog_active_skills(dataset_version, name_ko);

CREATE INDEX idx_catalog_active_skills_element
ON catalog_active_skills(dataset_version, element);

-- SQLite cannot alter a CHECK constraint in place. Rebuild this leaf table so
-- active-skill aliases use the same deterministic lookup path as other kinds.
DROP INDEX idx_catalog_alias_lookup;

CREATE TABLE catalog_aliases_v2 (
    dataset_version TEXT NOT NULL REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    entity_kind TEXT NOT NULL
        CHECK (entity_kind IN ('species', 'passive', 'active_skill', 'item')),
    entity_id TEXT NOT NULL,
    language TEXT NOT NULL DEFAULT 'ko',
    alias TEXT NOT NULL,
    normalized_alias TEXT NOT NULL,
    PRIMARY KEY (dataset_version, entity_kind, entity_id, normalized_alias)
);

INSERT INTO catalog_aliases_v2
    (dataset_version, entity_kind, entity_id, language, alias, normalized_alias)
SELECT dataset_version, entity_kind, entity_id, language, alias, normalized_alias
FROM catalog_aliases;

DROP TABLE catalog_aliases;
ALTER TABLE catalog_aliases_v2 RENAME TO catalog_aliases;

CREATE INDEX idx_catalog_alias_lookup
ON catalog_aliases(dataset_version, entity_kind, normalized_alias);
