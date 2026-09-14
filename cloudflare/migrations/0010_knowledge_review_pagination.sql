-- Stable keyset pagination indexes for operator review queues.

CREATE INDEX idx_knowledge_error_review_page
ON knowledge_error_book(created_at DESC, dataset_version, entry_id);

CREATE INDEX idx_knowledge_error_review_filter_page
ON knowledge_error_book(dataset_version, status, created_at DESC, entry_id);

CREATE INDEX idx_knowledge_manifest_review_page
ON knowledge_dataset_manifests(
    publication_status, generated_at DESC, dataset_version, manifest_sha256
);

CREATE INDEX idx_knowledge_wiki_revision_review_page
ON knowledge_wiki_revisions(
    review_status, created_at, dataset_version, page_id, revision_number
);

CREATE INDEX idx_knowledge_wiki_revision_filter_page
ON knowledge_wiki_revisions(
    dataset_version, review_status, created_at, page_id, revision_number
);
