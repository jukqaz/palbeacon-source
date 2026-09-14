-- Human-facing catalog search. Internal entity ids remain stable join keys,
-- but are never required as user input.
CREATE TABLE catalog_aliases (
    dataset_version TEXT NOT NULL REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    entity_kind TEXT NOT NULL CHECK (entity_kind IN ('species', 'passive', 'item')),
    entity_id TEXT NOT NULL,
    language TEXT NOT NULL DEFAULT 'ko',
    alias TEXT NOT NULL,
    normalized_alias TEXT NOT NULL,
    PRIMARY KEY (dataset_version, entity_kind, entity_id, normalized_alias)
);

CREATE INDEX idx_catalog_alias_lookup
ON catalog_aliases(dataset_version, entity_kind, normalized_alias);

CREATE INDEX idx_catalog_species_name_ko
ON catalog_species(dataset_version, name_ko);

CREATE INDEX idx_catalog_species_name_en
ON catalog_species(dataset_version, name_en);

CREATE INDEX idx_catalog_passives_name_ko
ON catalog_passives(dataset_version, name_ko);
