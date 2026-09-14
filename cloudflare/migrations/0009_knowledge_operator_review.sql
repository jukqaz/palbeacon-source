-- Optimistic review tokens for Cloudflare Access operator workflows.
--
-- Review mutations increment these counters and require the client-provided
-- previous value. This prevents a stale browser tab from silently replacing a
-- newer Error Book, manifest, or Wiki revision decision.

ALTER TABLE knowledge_error_book
ADD COLUMN review_version INTEGER NOT NULL DEFAULT 0
    CHECK (review_version >= 0);

ALTER TABLE knowledge_dataset_manifests
ADD COLUMN review_version INTEGER NOT NULL DEFAULT 0
    CHECK (review_version >= 0);

ALTER TABLE knowledge_wiki_revisions
ADD COLUMN review_version INTEGER NOT NULL DEFAULT 0
    CHECK (review_version >= 0);

ALTER TABLE knowledge_wiki_revisions
ADD COLUMN reviewed_at TEXT;
