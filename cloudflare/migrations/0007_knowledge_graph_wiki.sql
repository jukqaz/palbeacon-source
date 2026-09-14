-- Exact-build property graph plus reviewed, LLM-readable Korean wiki pages.
--
-- Canonical graph rows are produced only by exact extraction or an explicit
-- reviewed inference. Model-proposed relations live in a separate quarantine
-- table and cannot be retrieved as graph evidence until reviewed.

CREATE TABLE knowledge_graph_nodes (
    dataset_version TEXT NOT NULL
        REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    node_kind TEXT NOT NULL
        CHECK (node_kind IN (
            'species', 'item', 'active_skill', 'passive_skill', 'building',
            'technology', 'shop', 'point_of_interest', 'recipe',
            'breeding_rule', 'official_document', 'wiki_page', 'concept'
        )),
    label_ko TEXT NOT NULL,
    evidence_quality TEXT NOT NULL
        CHECK (evidence_quality IN ('exact', 'measured', 'model', 'unknown')),
    evidence_refs_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(evidence_refs_json)),
    content_hash TEXT NOT NULL CHECK (length(content_hash) = 64),
    PRIMARY KEY (dataset_version, node_id)
);

CREATE INDEX idx_knowledge_graph_nodes_kind
ON knowledge_graph_nodes(dataset_version, node_kind, label_ko);

CREATE TABLE knowledge_graph_edges (
    dataset_version TEXT NOT NULL,
    edge_id TEXT NOT NULL,
    from_node_id TEXT NOT NULL,
    to_node_id TEXT NOT NULL,
    relation_kind TEXT NOT NULL
        CHECK (relation_kind IN (
            'learns_skill', 'has_guaranteed_passive', 'drops_item',
            'ingredient_for', 'produces_item', 'sold_by', 'unlocks',
            'located_at', 'requires', 'breeding_parent', 'breeds_into', 'references',
            'related_to'
        )),
    origin TEXT NOT NULL
        CHECK (origin IN ('exact_extract', 'reviewed_inference')),
    evidence_quality TEXT NOT NULL
        CHECK (evidence_quality IN ('exact', 'measured', 'model', 'unknown')),
    evidence_refs_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(evidence_refs_json)),
    attributes_json TEXT NOT NULL DEFAULT '{}'
        CHECK (json_valid(attributes_json)),
    content_hash TEXT NOT NULL CHECK (length(content_hash) = 64),
    PRIMARY KEY (dataset_version, edge_id),
    FOREIGN KEY (dataset_version, from_node_id)
        REFERENCES knowledge_graph_nodes(dataset_version, node_id)
        ON DELETE CASCADE,
    FOREIGN KEY (dataset_version, to_node_id)
        REFERENCES knowledge_graph_nodes(dataset_version, node_id)
        ON DELETE CASCADE
);

CREATE INDEX idx_knowledge_graph_edges_outgoing
ON knowledge_graph_edges(dataset_version, from_node_id, relation_kind, to_node_id);

CREATE INDEX idx_knowledge_graph_edges_incoming
ON knowledge_graph_edges(dataset_version, to_node_id, relation_kind, from_node_id);

CREATE TABLE knowledge_edge_suggestions (
    suggestion_id TEXT PRIMARY KEY,
    dataset_version TEXT NOT NULL
        REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    from_node_id TEXT NOT NULL,
    to_node_id TEXT NOT NULL,
    proposed_relation_kind TEXT NOT NULL,
    rationale TEXT NOT NULL,
    model_id TEXT NOT NULL,
    source_refs_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(source_refs_json)),
    review_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (review_status IN ('pending', 'accepted', 'rejected')),
    reviewed_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_knowledge_edge_suggestions_review
ON knowledge_edge_suggestions(dataset_version, review_status, created_at);

CREATE TABLE knowledge_wiki_pages (
    dataset_version TEXT NOT NULL
        REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    page_id TEXT NOT NULL,
    page_kind TEXT NOT NULL
        CHECK (page_kind IN ('entity', 'concept', 'guide', 'comparison', 'index')),
    title_ko TEXT NOT NULL,
    summary_ko TEXT NOT NULL,
    body_markdown TEXT NOT NULL,
    related_node_ids_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(related_node_ids_json)),
    source_manifest_sha256 TEXT NOT NULL
        CHECK (length(source_manifest_sha256) = 64),
    review_status TEXT NOT NULL DEFAULT 'draft'
        CHECK (review_status IN ('draft', 'reviewed', 'superseded')),
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (dataset_version, page_id)
);

CREATE TABLE knowledge_wiki_claims (
    dataset_version TEXT NOT NULL,
    page_id TEXT NOT NULL,
    claim_id TEXT NOT NULL,
    claim_text_ko TEXT NOT NULL,
    evidence_quality TEXT NOT NULL
        CHECK (evidence_quality IN ('exact', 'measured', 'model', 'unknown')),
    evidence_refs_json TEXT NOT NULL
        CHECK (json_valid(evidence_refs_json)),
    related_node_ids_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(related_node_ids_json)),
    PRIMARY KEY (dataset_version, page_id, claim_id),
    FOREIGN KEY (dataset_version, page_id)
        REFERENCES knowledge_wiki_pages(dataset_version, page_id)
        ON DELETE CASCADE
);

CREATE VIRTUAL TABLE knowledge_wiki_fts USING fts5(
    page_id UNINDEXED,
    dataset_version UNINDEXED,
    title_ko,
    summary_ko,
    body_markdown,
    tokenize = 'unicode61'
);

-- Compound triggers are reconciled by
-- schema/post_migration_triggers.sql after Wrangler records this migration.
