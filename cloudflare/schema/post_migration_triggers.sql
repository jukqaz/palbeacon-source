-- Wrangler appends its migration-ledger INSERT to each migration before
-- sending it to D1's remote /query endpoint. Keep compound trigger statements
-- in this idempotent post-migration file so each backend parses them intact.
DROP TRIGGER IF EXISTS reject_unverified_dataset_activation_insert;
DROP TRIGGER IF EXISTS reject_unverified_dataset_activation_update;
DROP TRIGGER IF EXISTS game_build_policy_after_dataset_insert;
DROP TRIGGER IF EXISTS game_build_policy_after_dataset_activation;
DROP TRIGGER IF EXISTS knowledge_wiki_pages_ai;
DROP TRIGGER IF EXISTS knowledge_wiki_pages_au;
DROP TRIGGER IF EXISTS knowledge_wiki_pages_ad;
DROP TRIGGER IF EXISTS knowledge_manifest_reject_direct_publish_insert;
DROP TRIGGER IF EXISTS knowledge_manifest_reject_invalid_publish_update;
DROP TRIGGER IF EXISTS knowledge_manifest_publish_after_dataset_update;
DROP TRIGGER IF EXISTS knowledge_manifest_publish_after_manifest_write;
DROP TRIGGER IF EXISTS knowledge_manifest_publish_after_validation;
DROP TRIGGER IF EXISTS knowledge_manifest_demote_on_error;
DROP TRIGGER IF EXISTS knowledge_manifest_demote_on_error_update;
DROP TRIGGER IF EXISTS knowledge_manifest_revalidate_after_error_review;

CREATE TRIGGER reject_unverified_dataset_activation_insert
BEFORE INSERT ON data_versions
WHEN NEW.activated_at IS NOT NULL AND NEW.verified <> 1
BEGIN
    SELECT RAISE(ABORT, 'cannot activate an unverified dataset');
END;

CREATE TRIGGER reject_unverified_dataset_activation_update
BEFORE UPDATE OF activated_at, verified ON data_versions
WHEN NEW.activated_at IS NOT NULL AND NEW.verified <> 1
BEGIN
    SELECT RAISE(ABORT, 'cannot activate an unverified dataset');
END;

-- Activating a newly extracted exact-build dataset also updates compatibility
-- without changing the application or redeploying the Worker.
CREATE TRIGGER game_build_policy_after_dataset_insert
AFTER INSERT ON data_versions
WHEN NEW.verified = 1
 AND NEW.activated_at IS NOT NULL
 AND NEW.dataset_scope = 'full_catalog'
BEGIN
    INSERT INTO game_build_policies (
        game_build_id,
        compatibility_state,
        dataset_version,
        projection_schema,
        notes,
        reviewed_at
    ) VALUES (
        CASE
            WHEN NEW.game_build_id LIKE 'steam:%' THEN substr(NEW.game_build_id, 7)
            ELSE NEW.game_build_id
        END,
        'compatible',
        NEW.dataset_version,
        'cloud-profile-projection-v1',
        'Automatically approved from an active verified dataset',
        CURRENT_TIMESTAMP
    )
    ON CONFLICT(game_build_id) DO UPDATE SET
        compatibility_state = 'compatible',
        dataset_version = excluded.dataset_version,
        projection_schema = excluded.projection_schema,
        notes = excluded.notes,
        reviewed_at = excluded.reviewed_at,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER game_build_policy_after_dataset_activation
AFTER UPDATE OF verified, activated_at ON data_versions
WHEN NEW.verified = 1
 AND NEW.activated_at IS NOT NULL
 AND NEW.dataset_scope = 'full_catalog'
BEGIN
    INSERT INTO game_build_policies (
        game_build_id,
        compatibility_state,
        dataset_version,
        projection_schema,
        notes,
        reviewed_at
    ) VALUES (
        CASE
            WHEN NEW.game_build_id LIKE 'steam:%' THEN substr(NEW.game_build_id, 7)
            ELSE NEW.game_build_id
        END,
        'compatible',
        NEW.dataset_version,
        'cloud-profile-projection-v1',
        'Automatically approved from an active verified dataset',
        CURRENT_TIMESTAMP
    )
    ON CONFLICT(game_build_id) DO UPDATE SET
        compatibility_state = 'compatible',
        dataset_version = excluded.dataset_version,
        projection_schema = excluded.projection_schema,
        notes = excluded.notes,
        reviewed_at = excluded.reviewed_at,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER knowledge_wiki_pages_ai
AFTER INSERT ON knowledge_wiki_pages
WHEN NEW.review_status = 'reviewed'
BEGIN
    INSERT INTO knowledge_wiki_fts(
        page_id, dataset_version, title_ko, summary_ko, body_markdown
    ) VALUES (
        NEW.page_id, NEW.dataset_version, NEW.title_ko, NEW.summary_ko,
        NEW.body_markdown
    );
END;

CREATE TRIGGER knowledge_wiki_pages_au
AFTER UPDATE ON knowledge_wiki_pages
BEGIN
    DELETE FROM knowledge_wiki_fts
    WHERE page_id = OLD.page_id AND dataset_version = OLD.dataset_version;
    INSERT INTO knowledge_wiki_fts(
        page_id, dataset_version, title_ko, summary_ko, body_markdown
    )
    SELECT
        NEW.page_id, NEW.dataset_version, NEW.title_ko, NEW.summary_ko,
        NEW.body_markdown
    WHERE NEW.review_status = 'reviewed';
END;

CREATE TRIGGER knowledge_wiki_pages_ad
AFTER DELETE ON knowledge_wiki_pages
BEGIN
    DELETE FROM knowledge_wiki_fts
    WHERE page_id = OLD.page_id AND dataset_version = OLD.dataset_version;
END;

-- Publication is an automatic transition from validated only. Operators
-- cannot insert a row already marked published or promote a candidate.
CREATE TRIGGER knowledge_manifest_reject_direct_publish_insert
BEFORE INSERT ON knowledge_dataset_manifests
WHEN NEW.publication_status = 'published'
BEGIN
    SELECT RAISE(ABORT, 'knowledge manifests must publish from validated state');
END;

CREATE TRIGGER knowledge_manifest_reject_invalid_publish_update
BEFORE UPDATE OF publication_status ON knowledge_dataset_manifests
WHEN NEW.publication_status = 'published'
 AND (
    OLD.publication_status NOT IN ('validated', 'published')
    OR NOT EXISTS (
        SELECT 1 FROM data_versions
        WHERE dataset_version = NEW.dataset_version
          AND verified = 1
          AND activated_at IS NOT NULL
    )
    OR EXISTS (
        SELECT 1 FROM knowledge_error_book
        WHERE dataset_version = NEW.dataset_version
          AND status IN ('open', 'accepted')
    )
 )
BEGIN
    SELECT RAISE(ABORT, 'knowledge manifest does not satisfy publication gate');
END;

-- Knowledge publication follows, but never precedes, catalog activation.
CREATE TRIGGER knowledge_manifest_publish_after_dataset_update
AFTER UPDATE OF verified, activated_at ON data_versions
WHEN NEW.verified = 1 AND NEW.activated_at IS NOT NULL
BEGIN
    UPDATE knowledge_dataset_manifests
    SET publication_status = 'published',
        published_at = COALESCE(published_at, CURRENT_TIMESTAMP)
    WHERE dataset_version = NEW.dataset_version
      AND publication_status = 'validated'
      AND NOT EXISTS (
        SELECT 1 FROM knowledge_error_book
        WHERE dataset_version = NEW.dataset_version
          AND status IN ('open', 'accepted')
      );
END;

CREATE TRIGGER knowledge_manifest_publish_after_manifest_write
AFTER INSERT ON knowledge_dataset_manifests
WHEN NEW.publication_status = 'validated'
BEGIN
    UPDATE knowledge_dataset_manifests
    SET publication_status = 'published',
        published_at = COALESCE(published_at, CURRENT_TIMESTAMP)
    WHERE dataset_version = NEW.dataset_version
      AND EXISTS (
        SELECT 1 FROM data_versions
        WHERE dataset_version = NEW.dataset_version
          AND verified = 1
          AND activated_at IS NOT NULL
      )
      AND NOT EXISTS (
        SELECT 1 FROM knowledge_error_book
        WHERE dataset_version = NEW.dataset_version
          AND status IN ('open', 'accepted')
      );
END;

CREATE TRIGGER knowledge_manifest_publish_after_validation
AFTER UPDATE OF publication_status ON knowledge_dataset_manifests
WHEN NEW.publication_status = 'validated'
BEGIN
    UPDATE knowledge_dataset_manifests
    SET publication_status = 'published',
        published_at = COALESCE(published_at, CURRENT_TIMESTAMP)
    WHERE dataset_version = NEW.dataset_version
      AND EXISTS (
        SELECT 1 FROM data_versions
        WHERE dataset_version = NEW.dataset_version
          AND verified = 1
          AND activated_at IS NOT NULL
      )
      AND NOT EXISTS (
        SELECT 1 FROM knowledge_error_book
        WHERE dataset_version = NEW.dataset_version
          AND status IN ('open', 'accepted')
      );
END;

CREATE TRIGGER knowledge_manifest_demote_on_error
AFTER INSERT ON knowledge_error_book
WHEN NEW.status IN ('open', 'accepted')
BEGIN
    UPDATE knowledge_dataset_manifests
    SET publication_status = 'candidate',
        validated_at = NULL,
        published_at = NULL
    WHERE dataset_version = NEW.dataset_version;
END;

CREATE TRIGGER knowledge_manifest_demote_on_error_update
AFTER UPDATE OF status ON knowledge_error_book
WHEN NEW.status IN ('open', 'accepted')
BEGIN
    UPDATE knowledge_dataset_manifests
    SET publication_status = 'candidate',
        validated_at = NULL,
        published_at = NULL
    WHERE dataset_version = NEW.dataset_version;
END;

CREATE TRIGGER knowledge_manifest_revalidate_after_error_review
AFTER UPDATE OF status ON knowledge_error_book
WHEN OLD.status IN ('open', 'accepted')
 AND NEW.status IN ('rejected', 'resolved')
BEGIN
    UPDATE knowledge_dataset_manifests
    SET publication_status = 'validated',
        validated_at = CURRENT_TIMESTAMP
    WHERE dataset_version = NEW.dataset_version
      AND NOT EXISTS (
        SELECT 1 FROM knowledge_error_book
        WHERE dataset_version = NEW.dataset_version
          AND status IN ('open', 'accepted')
      );
END;
