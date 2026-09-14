PRAGMA foreign_keys = ON;

-- Human identities are created only from a cryptographically verified
-- Cloudflare Access assertion. The browser never chooses principal_id.
CREATE TABLE principals (
    principal_id TEXT PRIMARY KEY,
    access_subject TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL,
    display_name TEXT,
    role TEXT NOT NULL DEFAULT 'member'
        CHECK (role IN ('member', 'operator')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- An operator can bind an opaque game-side owner subject to one Access
-- principal. The raw platform user id and raw save never enter D1.
CREATE TABLE owner_bindings (
    owner_subject TEXT PRIMARY KEY,
    principal_id TEXT NOT NULL REFERENCES principals(principal_id) ON DELETE CASCADE,
    world_id TEXT NOT NULL,
    character_name TEXT,
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    bound_by_principal_id TEXT NOT NULL REFERENCES principals(principal_id),
    bound_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (principal_id, world_id)
);

CREATE TABLE service_agents (
    agent_id TEXT PRIMARY KEY,
    world_id TEXT NOT NULL,
    access_common_name TEXT NOT NULL UNIQUE,
    revoked_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TEXT
);

CREATE TABLE sync_runs (
    sync_id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL REFERENCES service_agents(agent_id),
    world_id TEXT NOT NULL,
    game_build_id TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    projection_schema TEXT NOT NULL,
    status TEXT NOT NULL
        CHECK (status IN ('received', 'validated', 'activated', 'failed', 'revoked')),
    owner_count INTEGER NOT NULL DEFAULT 0,
    pal_count INTEGER NOT NULL DEFAULT 0,
    inventory_slot_count INTEGER NOT NULL DEFAULT 0,
    error_code TEXT,
    error_detail_redacted TEXT,
    received_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TEXT
);

CREATE TABLE projection_commits (
    projection_id TEXT PRIMARY KEY,
    owner_subject TEXT NOT NULL,
    world_id TEXT NOT NULL,
    sync_id TEXT NOT NULL REFERENCES sync_runs(sync_id),
    game_build_id TEXT NOT NULL,
    dataset_version TEXT NOT NULL,
    projected_at TEXT NOT NULL,
    activated_at TEXT,
    is_active INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1)),
    UNIQUE (owner_subject, sync_id)
);

CREATE UNIQUE INDEX idx_projection_one_active
ON projection_commits(owner_subject, world_id)
WHERE is_active = 1;

CREATE TABLE character_profiles (
    projection_id TEXT PRIMARY KEY REFERENCES projection_commits(projection_id) ON DELETE CASCADE,
    character_name TEXT NOT NULL,
    level INTEGER NOT NULL,
    experience INTEGER,
    hp INTEGER,
    stamina INTEGER,
    attack INTEGER,
    defense INTEGER,
    work_speed INTEGER,
    carry_weight INTEGER,
    gold INTEGER,
    last_save_at TEXT NOT NULL
);

CREATE TABLE inventory_slots (
    projection_id TEXT NOT NULL REFERENCES projection_commits(projection_id) ON DELETE CASCADE,
    container_kind TEXT NOT NULL,
    container_ordinal INTEGER NOT NULL DEFAULT 0,
    slot_index INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK (quantity >= 0),
    PRIMARY KEY (projection_id, container_kind, container_ordinal, slot_index)
);

CREATE INDEX idx_inventory_item
ON inventory_slots(projection_id, item_id);

CREATE TABLE equipment_slots (
    projection_id TEXT NOT NULL REFERENCES projection_commits(projection_id) ON DELETE CASCADE,
    slot_kind TEXT NOT NULL,
    item_id TEXT NOT NULL,
    durability REAL,
    PRIMARY KEY (projection_id, slot_kind)
);

CREATE TABLE owned_pals (
    projection_id TEXT NOT NULL REFERENCES projection_commits(projection_id) ON DELETE CASCADE,
    pal_instance_id TEXT NOT NULL,
    species_id TEXT NOT NULL,
    nickname TEXT,
    level INTEGER NOT NULL,
    gender TEXT,
    rank INTEGER NOT NULL DEFAULT 0,
    iv_hp INTEGER,
    iv_attack INTEGER,
    iv_defense INTEGER,
    soul_hp INTEGER NOT NULL DEFAULT 0,
    soul_attack INTEGER NOT NULL DEFAULT 0,
    soul_defense INTEGER NOT NULL DEFAULT 0,
    location_kind TEXT NOT NULL,
    location_label TEXT,
    PRIMARY KEY (projection_id, pal_instance_id)
);

CREATE INDEX idx_owned_pals_species
ON owned_pals(projection_id, species_id);

CREATE TABLE owned_pal_passives (
    projection_id TEXT NOT NULL,
    pal_instance_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    passive_id TEXT NOT NULL,
    PRIMARY KEY (projection_id, pal_instance_id, position),
    FOREIGN KEY (projection_id, pal_instance_id)
        REFERENCES owned_pals(projection_id, pal_instance_id) ON DELETE CASCADE
);

CREATE TABLE owned_pal_skills (
    projection_id TEXT NOT NULL,
    pal_instance_id TEXT NOT NULL,
    skill_kind TEXT NOT NULL CHECK (skill_kind IN ('active', 'learned')),
    position INTEGER NOT NULL,
    skill_id TEXT NOT NULL,
    PRIMARY KEY (projection_id, pal_instance_id, skill_kind, position),
    FOREIGN KEY (projection_id, pal_instance_id)
        REFERENCES owned_pals(projection_id, pal_instance_id) ON DELETE CASCADE
);

CREATE TABLE base_profiles (
    projection_id TEXT NOT NULL REFERENCES projection_commits(projection_id) ON DELETE CASCADE,
    base_id TEXT NOT NULL,
    name TEXT,
    level INTEGER,
    assigned_pal_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (projection_id, base_id)
);

CREATE TABLE progression_flags (
    projection_id TEXT NOT NULL REFERENCES projection_commits(projection_id) ON DELETE CASCADE,
    category TEXT NOT NULL,
    flag_id TEXT NOT NULL,
    completed INTEGER NOT NULL CHECK (completed IN (0, 1)),
    PRIMARY KEY (projection_id, category, flag_id)
);

CREATE TABLE data_versions (
    dataset_version TEXT PRIMARY KEY,
    game_version TEXT NOT NULL,
    game_build_id TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    verified INTEGER NOT NULL DEFAULT 0 CHECK (verified IN (0, 1)),
    activated_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX idx_one_active_dataset
ON data_versions(verified)
WHERE activated_at IS NOT NULL;

CREATE TABLE catalog_species (
    dataset_version TEXT NOT NULL REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    species_id TEXT NOT NULL,
    name_ko TEXT NOT NULL,
    name_en TEXT,
    element_json TEXT NOT NULL DEFAULT '[]',
    work_json TEXT NOT NULL DEFAULT '{}',
    base_hp INTEGER,
    base_attack INTEGER,
    base_defense INTEGER,
    ride_sprint_speed INTEGER,
    ride_stamina INTEGER,
    source_ref TEXT,
    PRIMARY KEY (dataset_version, species_id)
);

CREATE TABLE catalog_passives (
    dataset_version TEXT NOT NULL REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    passive_id TEXT NOT NULL,
    name_ko TEXT NOT NULL,
    description_ko TEXT,
    effect_json TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (dataset_version, passive_id)
);

CREATE TABLE breeding_rules (
    dataset_version TEXT NOT NULL REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    parent_a_species_id TEXT NOT NULL,
    parent_b_species_id TEXT NOT NULL,
    child_species_id TEXT NOT NULL,
    rule_kind TEXT NOT NULL DEFAULT 'exact',
    PRIMARY KEY (
        dataset_version,
        parent_a_species_id,
        parent_b_species_id,
        child_species_id
    )
);

CREATE INDEX idx_breeding_child
ON breeding_rules(dataset_version, child_species_id);

CREATE TABLE catalog_items (
    dataset_version TEXT NOT NULL REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    name_ko TEXT NOT NULL,
    name_en TEXT,
    category TEXT,
    price INTEGER,
    description_ko TEXT,
    PRIMARY KEY (dataset_version, item_id)
);

CREATE TABLE acquisition_methods (
    dataset_version TEXT NOT NULL REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    method_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    method_kind TEXT NOT NULL,
    source_entity_id TEXT,
    location_id TEXT,
    quantity_min REAL,
    quantity_max REAL,
    probability REAL,
    cost INTEGER,
    requirements_json TEXT NOT NULL DEFAULT '{}',
    evidence_ref TEXT,
    PRIMARY KEY (dataset_version, method_id)
);

CREATE INDEX idx_acquisition_item
ON acquisition_methods(dataset_version, item_id);

CREATE TABLE knowledge_documents (
    dataset_version TEXT NOT NULL REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    document_id TEXT NOT NULL,
    title TEXT NOT NULL,
    section TEXT,
    content TEXT NOT NULL,
    source_url TEXT,
    source_kind TEXT NOT NULL,
    confidence TEXT NOT NULL
        CHECK (confidence IN ('exact', 'measured', 'model', 'unknown')),
    PRIMARY KEY (dataset_version, document_id)
);

CREATE VIRTUAL TABLE knowledge_fts USING fts5(
    document_id UNINDEXED,
    dataset_version UNINDEXED,
    title,
    section,
    content,
    tokenize = 'unicode61'
);

CREATE TABLE assistant_runs (
    run_id TEXT PRIMARY KEY,
    principal_id TEXT NOT NULL REFERENCES principals(principal_id) ON DELETE CASCADE,
    dataset_version TEXT NOT NULL,
    game_build_id TEXT NOT NULL,
    evidence_count INTEGER NOT NULL,
    quality TEXT NOT NULL
        CHECK (quality IN ('exact', 'measured', 'model', 'unknown')),
    model_id TEXT,
    status TEXT NOT NULL CHECK (status IN ('answered', 'fallback', 'failed')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Prompts, evidence bodies, answers, tokens, raw ids and raw saves are never
-- written to audit_events. Only bounded operational metadata is retained.
CREATE TABLE audit_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_principal_id TEXT REFERENCES principals(principal_id),
    action TEXT NOT NULL,
    target_kind TEXT NOT NULL,
    target_key_redacted TEXT,
    outcome TEXT NOT NULL,
    request_id TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_audit_recent
ON audit_events(created_at DESC);
