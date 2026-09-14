-- Dataset lineage, Wiki revisions and retrieval observability.
--
-- This is intentionally a compact OpenLineage-inspired control plane. Exact
-- game data remains in the catalog and canonical graph tables; this schema
-- records which immutable artifacts and compiler run produced each published
-- knowledge component.

CREATE TABLE knowledge_dataset_manifests (
    dataset_version TEXT PRIMARY KEY
        REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    manifest_sha256 TEXT NOT NULL CHECK (length(manifest_sha256) = 64),
    compiler_version TEXT NOT NULL,
    graph_schema_version TEXT NOT NULL,
    wiki_schema_version TEXT NOT NULL,
    publication_status TEXT NOT NULL DEFAULT 'candidate'
        CHECK (publication_status IN ('candidate', 'validated', 'published', 'retired')),
    source_artifact_count INTEGER NOT NULL DEFAULT 0 CHECK (source_artifact_count >= 0),
    graph_node_count INTEGER NOT NULL DEFAULT 0 CHECK (graph_node_count >= 0),
    graph_edge_count INTEGER NOT NULL DEFAULT 0 CHECK (graph_edge_count >= 0),
    wiki_page_count INTEGER NOT NULL DEFAULT 0 CHECK (wiki_page_count >= 0),
    generated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    validated_at TEXT,
    published_at TEXT
);

CREATE INDEX idx_knowledge_dataset_publication
ON knowledge_dataset_manifests(publication_status, generated_at DESC);

CREATE TABLE knowledge_source_artifacts (
    dataset_version TEXT NOT NULL
        REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    artifact_id TEXT NOT NULL,
    artifact_kind TEXT NOT NULL
        CHECK (artifact_kind IN (
            'game_extract', 'localization', 'mapping', 'catalog', 'official_document'
        )),
    logical_source_ref TEXT NOT NULL,
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    evidence_quality TEXT NOT NULL
        CHECK (evidence_quality IN ('exact', 'measured', 'model', 'unknown')),
    PRIMARY KEY (dataset_version, artifact_id)
);

CREATE TABLE knowledge_pipeline_runs (
    run_id TEXT PRIMARY KEY,
    dataset_version TEXT NOT NULL
        REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    pipeline_name TEXT NOT NULL,
    compiler_version TEXT NOT NULL,
    status TEXT NOT NULL
        CHECK (status IN ('started', 'completed', 'failed', 'aborted')),
    input_facets_json TEXT NOT NULL DEFAULT '{}'
        CHECK (json_valid(input_facets_json)),
    output_facets_json TEXT NOT NULL DEFAULT '{}'
        CHECK (json_valid(output_facets_json)),
    error_code TEXT,
    started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TEXT
);

CREATE INDEX idx_knowledge_pipeline_dataset
ON knowledge_pipeline_runs(dataset_version, started_at DESC);

CREATE TABLE knowledge_lineage_edges (
    dataset_version TEXT NOT NULL,
    lineage_id TEXT NOT NULL,
    run_id TEXT NOT NULL
        REFERENCES knowledge_pipeline_runs(run_id) ON DELETE CASCADE,
    input_artifact_id TEXT NOT NULL,
    output_kind TEXT NOT NULL
        CHECK (output_kind IN ('catalog', 'graph', 'wiki', 'search_index')),
    output_id TEXT NOT NULL,
    transform_kind TEXT NOT NULL
        CHECK (transform_kind IN (
            'exact_projection', 'wiki_compilation', 'fts_indexing'
        )),
    PRIMARY KEY (dataset_version, lineage_id),
    FOREIGN KEY (dataset_version, input_artifact_id)
        REFERENCES knowledge_source_artifacts(dataset_version, artifact_id)
        ON DELETE CASCADE
);

CREATE INDEX idx_knowledge_lineage_output
ON knowledge_lineage_edges(dataset_version, output_kind, output_id);

CREATE TABLE knowledge_wiki_revisions (
    dataset_version TEXT NOT NULL,
    page_id TEXT NOT NULL,
    revision_number INTEGER NOT NULL CHECK (revision_number > 0),
    title_ko TEXT NOT NULL,
    summary_ko TEXT NOT NULL,
    body_markdown TEXT NOT NULL,
    claims_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(claims_json)),
    related_node_ids_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(related_node_ids_json)),
    change_origin TEXT NOT NULL
        CHECK (change_origin IN ('exact_compiler', 'human_edit', 'llm_draft')),
    review_status TEXT NOT NULL
        CHECK (review_status IN ('draft', 'reviewed', 'rejected', 'superseded')),
    generator_id TEXT,
    reviewer_id TEXT,
    change_summary_ko TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (dataset_version, page_id, revision_number),
    FOREIGN KEY (dataset_version, page_id)
        REFERENCES knowledge_wiki_pages(dataset_version, page_id)
        ON DELETE CASCADE
);

CREATE INDEX idx_knowledge_wiki_revision_review
ON knowledge_wiki_revisions(dataset_version, review_status, created_at DESC);

-- One stored direction preserves the canonical relation. Both indexes make
-- outgoing and incoming link-follow operations equally cheap.
CREATE TABLE knowledge_wiki_links (
    dataset_version TEXT NOT NULL,
    from_page_id TEXT NOT NULL,
    to_page_id TEXT NOT NULL,
    relation_kind TEXT NOT NULL,
    source_edge_id TEXT NOT NULL,
    PRIMARY KEY (dataset_version, from_page_id, to_page_id, source_edge_id),
    FOREIGN KEY (dataset_version, from_page_id)
        REFERENCES knowledge_wiki_pages(dataset_version, page_id)
        ON DELETE CASCADE,
    FOREIGN KEY (dataset_version, to_page_id)
        REFERENCES knowledge_wiki_pages(dataset_version, page_id)
        ON DELETE CASCADE,
    FOREIGN KEY (dataset_version, source_edge_id)
        REFERENCES knowledge_graph_edges(dataset_version, edge_id)
        ON DELETE CASCADE
);

CREATE INDEX idx_knowledge_wiki_links_incoming
ON knowledge_wiki_links(dataset_version, to_page_id, relation_kind, from_page_id);

CREATE TABLE knowledge_error_book (
    entry_id TEXT NOT NULL,
    dataset_version TEXT NOT NULL
        REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    subject_kind TEXT NOT NULL
        CHECK (subject_kind IN (
            'artifact', 'graph_node', 'graph_edge', 'wiki_page', 'retrieval'
        )),
    subject_id TEXT NOT NULL,
    error_kind TEXT NOT NULL,
    diagnosis_ko TEXT NOT NULL,
    proposed_correction_ko TEXT,
    source_refs_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(source_refs_json)),
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'accepted', 'rejected', 'resolved')),
    detected_by TEXT NOT NULL
        CHECK (detected_by IN ('validator', 'human', 'llm')),
    reviewed_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (dataset_version, entry_id)
);

CREATE INDEX idx_knowledge_error_book_open
ON knowledge_error_book(dataset_version, status, subject_kind, created_at DESC);

CREATE TABLE knowledge_retrieval_runs (
    retrieval_id TEXT PRIMARY KEY,
    assistant_run_id TEXT
        REFERENCES assistant_runs(run_id) ON DELETE SET NULL,
    dataset_version TEXT NOT NULL,
    query_sha256 TEXT NOT NULL CHECK (length(query_sha256) = 64),
    intent TEXT NOT NULL
        CHECK (intent IN ('entity_lookup', 'relationship', 'comparison', 'overview')),
    route TEXT NOT NULL
        CHECK (route IN ('search_first', 'browse_first')),
    seed_node_count INTEGER NOT NULL CHECK (seed_node_count >= 0),
    graph_edge_count INTEGER NOT NULL CHECK (graph_edge_count >= 0),
    wiki_page_count INTEGER NOT NULL CHECK (wiki_page_count >= 0),
    source_document_count INTEGER NOT NULL CHECK (source_document_count >= 0),
    context_row_count INTEGER NOT NULL CHECK (context_row_count >= 0),
    context_bytes INTEGER NOT NULL CHECK (context_bytes >= 0),
    completed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_knowledge_retrieval_dataset
ON knowledge_retrieval_runs(dataset_version, completed_at DESC);

-- Compound triggers are reconciled by
-- schema/post_migration_triggers.sql after Wrangler records this migration.
