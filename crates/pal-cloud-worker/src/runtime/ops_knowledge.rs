use serde::{Deserialize, Serialize};
use serde_json::json;
use wasm_bindgen::JsValue;
use worker::{D1Database, D1Result, Request, Response};

use crate::list_pagination::{StableListCursor, page_limit};

use super::{
    ApiError, PrincipalContext, ProtectedCapability, audit, json_response, read_json_body,
    valid_hex_id, valid_short_id,
};

const MAX_REVIEW_BODY_BYTES: usize = 32 * 1024;
const ERROR_LIST_SCOPE: &str = "knowledge-errors";
const MANIFEST_LIST_SCOPE: &str = "knowledge-manifests";
const WIKI_REVISION_LIST_SCOPE: &str = "knowledge-wiki-revisions";

#[derive(Debug, Default, Deserialize)]
struct ReviewListQuery {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    dataset_version: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
}

struct ParsedReviewList {
    limit: u32,
    dataset_version: Option<String>,
    status: Option<String>,
    cursor: Option<StableListCursor>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ErrorBookReviewRow {
    entry_id: String,
    dataset_version: String,
    subject_kind: String,
    subject_id: String,
    error_kind: String,
    diagnosis_ko: String,
    proposed_correction_ko: Option<String>,
    source_refs_json: String,
    status: String,
    detected_by: String,
    reviewed_at: Option<String>,
    created_at: String,
    review_version: i64,
}

#[derive(Debug, Deserialize)]
struct ErrorBookReviewRequest {
    dataset_version: String,
    entry_id: String,
    expected_status: String,
    expected_review_version: i64,
    status: String,
    #[serde(default)]
    proposed_correction_ko: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ManifestReviewRow {
    dataset_version: String,
    manifest_sha256: String,
    compiler_version: String,
    graph_schema_version: String,
    wiki_schema_version: String,
    publication_status: String,
    generated_at: String,
    validated_at: Option<String>,
    published_at: Option<String>,
    review_version: i64,
    dataset_verified: i64,
    dataset_active: i64,
    source_artifact_count: i64,
    actual_source_artifact_count: i64,
    graph_node_count: i64,
    actual_graph_node_count: i64,
    graph_edge_count: i64,
    actual_graph_edge_count: i64,
    wiki_page_count: i64,
    reviewed_wiki_page_count: i64,
    unresolved_error_count: i64,
    untrusted_source_count: i64,
    untrusted_graph_count: i64,
    untrusted_claim_count: i64,
    completed_pipeline_count: i64,
    ready_for_validation: i64,
}

#[derive(Debug, Deserialize)]
struct ManifestValidateRequest {
    dataset_version: String,
    expected_manifest_sha256: String,
    expected_review_version: i64,
}

#[derive(Debug, Deserialize, Serialize)]
struct WikiRevisionReviewRow {
    dataset_version: String,
    page_id: String,
    revision_number: i64,
    title_ko: String,
    summary_ko: String,
    body_markdown: String,
    claims_json: String,
    related_node_ids_json: String,
    change_origin: String,
    review_status: String,
    generator_id: Option<String>,
    reviewer_id: Option<String>,
    change_summary_ko: String,
    created_at: String,
    reviewed_at: Option<String>,
    review_version: i64,
}

#[derive(Debug, Deserialize)]
struct WikiRevisionReviewRequest {
    dataset_version: String,
    page_id: String,
    revision_number: i64,
    expected_review_version: i64,
    decision: String,
}

#[derive(Debug, Deserialize)]
struct WikiRevisionGateRow {
    review_status: String,
    review_version: i64,
    invalid_claim_count: i64,
}

pub(super) async fn errors_response(
    req: &Request,
    db: &D1Database,
    principal: &PrincipalContext,
) -> Result<Response, ApiError> {
    authorize_review(principal)?;
    let page = parse_list_query(
        req,
        ERROR_LIST_SCOPE,
        &["open", "accepted", "rejected", "resolved"],
    )?;
    let mut rows = db
        .prepare(
            "SELECT entry_id, dataset_version, subject_kind, subject_id, error_kind,
                    diagnosis_ko, proposed_correction_ko, source_refs_json, status,
                    detected_by, reviewed_at, created_at, review_version
             FROM knowledge_error_book
             WHERE (?1 IS NULL OR dataset_version=?1)
               AND (?2 IS NULL OR status=?2)
               AND (
                 ?3 IS NULL
                 OR created_at < ?3
                 OR (created_at = ?3 AND dataset_version > ?4)
                 OR (created_at = ?3 AND dataset_version = ?4 AND entry_id > ?5)
               )
             ORDER BY created_at DESC, dataset_version, entry_id
             LIMIT ?6",
        )
        .bind(&[
            optional_js_str(page.dataset_version.as_deref()),
            optional_js_str(page.status.as_deref()),
            optional_cursor_str(page.cursor.as_ref(), StableListCursor::sort_timestamp),
            optional_cursor_str(page.cursor.as_ref(), StableListCursor::row_dataset_version),
            optional_cursor_str(page.cursor.as_ref(), StableListCursor::row_key),
            JsValue::from_f64((page.limit + 1) as f64),
        ])
        .map_err(ApiError::internal)?
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<ErrorBookReviewRow>()
        .map_err(ApiError::internal)?;
    let has_more = rows.len() > page.limit as usize;
    if has_more {
        rows.truncate(page.limit as usize);
    }
    let next_cursor = if has_more {
        rows.last()
            .map(|row| {
                StableListCursor::new(
                    ERROR_LIST_SCOPE,
                    page.dataset_version.as_deref(),
                    page.status.as_deref(),
                    &row.created_at,
                    &row.dataset_version,
                    &row.entry_id,
                    None,
                )
                .and_then(|cursor| cursor.encode())
            })
            .transpose()
            .map_err(ApiError::internal)?
    } else {
        None
    };
    json_response(&json!({
        "items": rows,
        "count": rows.len(),
        "next_cursor": next_cursor
    }))
}

pub(super) async fn review_error_response(
    req: &mut Request,
    db: &D1Database,
    principal: &PrincipalContext,
    request_id: &str,
) -> Result<Response, ApiError> {
    authorize_review(principal)?;
    let input: ErrorBookReviewRequest = read_json_body(req, MAX_REVIEW_BODY_BYTES).await?;
    validate_error_review(&input)?;
    let correction = input
        .proposed_correction_ko
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let result = db
        .prepare(
            "UPDATE knowledge_error_book
             SET status=?1,
                 proposed_correction_ko=COALESCE(?2, proposed_correction_ko),
                 reviewed_at=CURRENT_TIMESTAMP,
                 review_version=review_version+1
             WHERE dataset_version=?3 AND entry_id=?4
               AND status=?5 AND review_version=?6",
        )
        .bind(&[
            JsValue::from_str(&input.status),
            optional_js_str(correction),
            JsValue::from_str(&input.dataset_version),
            JsValue::from_str(&input.entry_id),
            JsValue::from_str(&input.expected_status),
            JsValue::from_f64(input.expected_review_version as f64),
        ])
        .map_err(ApiError::internal)?
        .run()
        .await
        .map_err(ApiError::internal)?;
    let target_key = format!("{}:{}", input.dataset_version, input.entry_id);
    if d1_change_count(&result)? != 1 {
        audit(
            db,
            principal,
            "knowledge.error.review",
            "knowledge_error",
            Some(&target_key),
            "stale_conflict",
            request_id,
        )
        .await?;
        return Err(ApiError::conflict(
            "Error Book entry changed after it was loaded; refresh and retry",
        ));
    }
    audit(
        db,
        principal,
        "knowledge.error.review",
        "knowledge_error",
        Some(&target_key),
        &input.status,
        request_id,
    )
    .await?;
    let row = load_error(db, &input.dataset_version, &input.entry_id).await?;
    json_response(&json!({"ok": true, "item": row}))
}

pub(super) async fn manifests_response(
    req: &Request,
    db: &D1Database,
    principal: &PrincipalContext,
) -> Result<Response, ApiError> {
    authorize_review(principal)?;
    let page = parse_list_query(
        req,
        MANIFEST_LIST_SCOPE,
        &["candidate", "validated", "published", "retired"],
    )?;
    let mut rows = db
        .prepare(format!(
            "{} WHERE (?1 IS NULL OR dataset_version=?1)
               AND (
                 (?2 IS NULL AND publication_status IN ('candidate', 'validated'))
                 OR publication_status=?2
               )
               AND (
                 ?3 IS NULL
                 OR generated_at < ?3
                 OR (generated_at = ?3 AND dataset_version > ?4)
                 OR (
                   generated_at = ?3 AND dataset_version = ?4
                   AND manifest_sha256 > ?5
                 )
               )
             ORDER BY generated_at DESC, dataset_version, manifest_sha256
             LIMIT ?6",
            manifest_review_select()
        ))
        .bind(&[
            optional_js_str(page.dataset_version.as_deref()),
            optional_js_str(page.status.as_deref()),
            optional_cursor_str(page.cursor.as_ref(), StableListCursor::sort_timestamp),
            optional_cursor_str(page.cursor.as_ref(), StableListCursor::row_dataset_version),
            optional_cursor_str(page.cursor.as_ref(), StableListCursor::row_key),
            JsValue::from_f64((page.limit + 1) as f64),
        ])
        .map_err(ApiError::internal)?
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<ManifestReviewRow>()
        .map_err(ApiError::internal)?;
    let has_more = rows.len() > page.limit as usize;
    if has_more {
        rows.truncate(page.limit as usize);
    }
    let next_cursor = if has_more {
        rows.last()
            .map(|row| {
                StableListCursor::new(
                    MANIFEST_LIST_SCOPE,
                    page.dataset_version.as_deref(),
                    page.status.as_deref(),
                    &row.generated_at,
                    &row.dataset_version,
                    &row.manifest_sha256,
                    None,
                )
                .and_then(|cursor| cursor.encode())
            })
            .transpose()
            .map_err(ApiError::internal)?
    } else {
        None
    };
    json_response(&json!({
        "items": rows,
        "count": rows.len(),
        "next_cursor": next_cursor
    }))
}

pub(super) async fn validate_manifest_response(
    req: &mut Request,
    db: &D1Database,
    principal: &PrincipalContext,
    request_id: &str,
) -> Result<Response, ApiError> {
    authorize_review(principal)?;
    let input: ManifestValidateRequest = read_json_body(req, MAX_REVIEW_BODY_BYTES).await?;
    if !valid_short_id(&input.dataset_version)
        || !valid_hex_id(&input.expected_manifest_sha256, 64)
        || input.expected_review_version < 0
    {
        return Err(ApiError::bad_request(
            "manifest review precondition is invalid",
        ));
    }
    let target_key = input.dataset_version.clone();
    let current = load_manifest_review(
        db,
        &input.dataset_version,
        &input.expected_manifest_sha256,
        input.expected_review_version,
    )
    .await?
    .ok_or_else(|| {
        ApiError::conflict("Knowledge manifest changed after it was loaded; refresh and retry")
    })?;
    if current.publication_status != "candidate" {
        return Err(ApiError::conflict(
            "Only a candidate knowledge manifest can be validated",
        ));
    }
    if current.ready_for_validation != 1 {
        audit(
            db,
            principal,
            "knowledge.manifest.validate",
            "knowledge_manifest",
            Some(&target_key),
            "blocked",
            request_id,
        )
        .await?;
        return Err(ApiError::unprocessable(
            "Knowledge manifest has unresolved provenance, count, pipeline, or Error Book blockers",
        ));
    }

    let result = db
        .prepare(
            "UPDATE knowledge_dataset_manifests AS manifest
             SET publication_status='validated',
                 validated_at=CURRENT_TIMESTAMP,
                 review_version=review_version+1
             WHERE dataset_version=?1 AND manifest_sha256=?2
               AND publication_status='candidate' AND review_version=?3
               AND source_artifact_count > 0
               AND graph_node_count > 0
               AND wiki_page_count > 0
               AND EXISTS (
                 SELECT 1 FROM data_versions version
                 WHERE version.dataset_version=manifest.dataset_version
                   AND version.verified=1
               )
               AND source_artifact_count=(
                 SELECT COUNT(*) FROM knowledge_source_artifacts source
                 WHERE source.dataset_version=manifest.dataset_version
               )
               AND graph_node_count=(
                 SELECT COUNT(*) FROM knowledge_graph_nodes node
                 WHERE node.dataset_version=manifest.dataset_version
               )
               AND graph_edge_count=(
                 SELECT COUNT(*) FROM knowledge_graph_edges edge
                 WHERE edge.dataset_version=manifest.dataset_version
               )
               AND wiki_page_count=(
                 SELECT COUNT(*) FROM knowledge_wiki_pages page
                 WHERE page.dataset_version=manifest.dataset_version
                   AND page.review_status='reviewed'
               )
               AND EXISTS (
                 SELECT 1 FROM knowledge_pipeline_runs pipeline
                 WHERE pipeline.dataset_version=manifest.dataset_version
                   AND pipeline.status='completed'
               )
               AND NOT EXISTS (
                 SELECT 1 FROM knowledge_error_book error
                 WHERE error.dataset_version=manifest.dataset_version
                   AND error.status IN ('open', 'accepted')
               )
               AND NOT EXISTS (
                 SELECT 1 FROM knowledge_source_artifacts source
                 WHERE source.dataset_version=manifest.dataset_version
                   AND source.evidence_quality NOT IN ('exact', 'measured')
               )
               AND NOT EXISTS (
                 SELECT 1 FROM knowledge_graph_nodes node
                 WHERE node.dataset_version=manifest.dataset_version
                   AND node.evidence_quality NOT IN ('exact', 'measured')
               )
               AND NOT EXISTS (
                 SELECT 1 FROM knowledge_graph_edges edge
                 WHERE edge.dataset_version=manifest.dataset_version
                   AND edge.evidence_quality NOT IN ('exact', 'measured')
               )
               AND NOT EXISTS (
                 SELECT 1 FROM knowledge_wiki_claims claim
                 WHERE claim.dataset_version=manifest.dataset_version
                   AND claim.evidence_quality NOT IN ('exact', 'measured')
               )",
        )
        .bind(&[
            JsValue::from_str(&input.dataset_version),
            JsValue::from_str(&input.expected_manifest_sha256),
            JsValue::from_f64(input.expected_review_version as f64),
        ])
        .map_err(ApiError::internal)?
        .run()
        .await
        .map_err(ApiError::internal)?;
    if d1_change_count(&result)? != 1 {
        audit(
            db,
            principal,
            "knowledge.manifest.validate",
            "knowledge_manifest",
            Some(&target_key),
            "stale_conflict",
            request_id,
        )
        .await?;
        return Err(ApiError::conflict(
            "Knowledge manifest blockers or review version changed; refresh and retry",
        ));
    }
    audit(
        db,
        principal,
        "knowledge.manifest.validate",
        "knowledge_manifest",
        Some(&target_key),
        "validated",
        request_id,
    )
    .await?;
    let row = load_manifest_review(
        db,
        &input.dataset_version,
        &input.expected_manifest_sha256,
        input.expected_review_version + 1,
    )
    .await?
    .ok_or_else(|| ApiError::internal("validated knowledge manifest disappeared"))?;
    json_response(&json!({"ok": true, "item": row}))
}

pub(super) async fn wiki_revisions_response(
    req: &Request,
    db: &D1Database,
    principal: &PrincipalContext,
) -> Result<Response, ApiError> {
    authorize_review(principal)?;
    let page = parse_list_query(
        req,
        WIKI_REVISION_LIST_SCOPE,
        &["draft", "reviewed", "rejected", "superseded"],
    )?;
    let mut rows = db
        .prepare(
            "SELECT dataset_version, page_id, revision_number, title_ko, summary_ko,
                    body_markdown, claims_json, related_node_ids_json, change_origin,
                    review_status, generator_id, reviewer_id, change_summary_ko,
                    created_at, reviewed_at, review_version
             FROM knowledge_wiki_revisions
             WHERE (?1 IS NULL OR dataset_version=?1)
               AND (
                 (?2 IS NULL AND review_status='draft')
                 OR review_status=?2
               )
               AND (
                 ?3 IS NULL
                 OR created_at > ?3
                 OR (created_at = ?3 AND dataset_version > ?4)
                 OR (created_at = ?3 AND dataset_version = ?4 AND page_id > ?5)
                 OR (
                   created_at = ?3 AND dataset_version = ?4 AND page_id = ?5
                   AND revision_number > ?6
                 )
               )
             ORDER BY created_at, dataset_version, page_id, revision_number
             LIMIT ?7",
        )
        .bind(&[
            optional_js_str(page.dataset_version.as_deref()),
            optional_js_str(page.status.as_deref()),
            optional_cursor_str(page.cursor.as_ref(), StableListCursor::sort_timestamp),
            optional_cursor_str(page.cursor.as_ref(), StableListCursor::row_dataset_version),
            optional_cursor_str(page.cursor.as_ref(), StableListCursor::row_key),
            optional_cursor_ordinal(page.cursor.as_ref()),
            JsValue::from_f64((page.limit + 1) as f64),
        ])
        .map_err(ApiError::internal)?
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<WikiRevisionReviewRow>()
        .map_err(ApiError::internal)?;
    let has_more = rows.len() > page.limit as usize;
    if has_more {
        rows.truncate(page.limit as usize);
    }
    let next_cursor = if has_more {
        rows.last()
            .map(|row| {
                StableListCursor::new(
                    WIKI_REVISION_LIST_SCOPE,
                    page.dataset_version.as_deref(),
                    page.status.as_deref(),
                    &row.created_at,
                    &row.dataset_version,
                    &row.page_id,
                    Some(row.revision_number),
                )
                .and_then(|cursor| cursor.encode())
            })
            .transpose()
            .map_err(ApiError::internal)?
    } else {
        None
    };
    json_response(&json!({
        "items": rows,
        "count": rows.len(),
        "next_cursor": next_cursor
    }))
}

pub(super) async fn review_wiki_revision_response(
    req: &mut Request,
    db: &D1Database,
    principal: &PrincipalContext,
    request_id: &str,
) -> Result<Response, ApiError> {
    authorize_review(principal)?;
    let input: WikiRevisionReviewRequest = read_json_body(req, MAX_REVIEW_BODY_BYTES).await?;
    if !valid_short_id(&input.dataset_version)
        || !valid_short_id(&input.page_id)
        || input.revision_number <= 0
        || input.expected_review_version < 0
        || !matches!(input.decision.as_str(), "approve" | "reject")
    {
        return Err(ApiError::bad_request(
            "Wiki revision review request is invalid",
        ));
    }
    let review_status = if input.decision == "approve" {
        "reviewed"
    } else {
        "rejected"
    };
    let reviewer_id = principal.principal_hex();
    let target_key = format!(
        "{}:{}:{}",
        input.dataset_version, input.page_id, input.revision_number
    );
    let gate = load_wiki_revision_gate(
        db,
        &input.dataset_version,
        &input.page_id,
        input.revision_number,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("Wiki revision was not found"))?;
    if gate.review_status != "draft" || gate.review_version != input.expected_review_version {
        return Err(ApiError::conflict(
            "Wiki revision changed after it was loaded; refresh and retry",
        ));
    }
    if input.decision == "approve" && gate.invalid_claim_count != 0 {
        audit(
            db,
            principal,
            "knowledge.wiki_revision.review",
            "knowledge_wiki_revision",
            Some(&target_key),
            "blocked_invalid_claims",
            request_id,
        )
        .await?;
        return Err(ApiError::unprocessable(
            "Wiki revision claims must be an array of exact or measured evidence records",
        ));
    }
    let update_revision = db
        .prepare(
            "UPDATE knowledge_wiki_revisions
             SET review_status=?1, reviewer_id=?2, reviewed_at=CURRENT_TIMESTAMP,
                 review_version=review_version+1
             WHERE dataset_version=?3 AND page_id=?4 AND revision_number=?5
               AND review_status='draft' AND review_version=?6
               AND (
                 ?1='rejected'
                 OR (
                   json_type(claims_json)='array'
                   AND NOT EXISTS (
                     SELECT 1 FROM json_each(claims_json) claim
                     WHERE json_type(claim.value, '$.claim_id') IS NOT 'text'
                        OR length(trim(json_extract(claim.value, '$.claim_id')))=0
                        OR length(json_extract(claim.value, '$.claim_id'))>256
                        OR json_type(claim.value, '$.text_ko') IS NOT 'text'
                        OR length(trim(json_extract(claim.value, '$.text_ko')))=0
                        OR COALESCE(json_extract(claim.value, '$.quality'), '')
                           NOT IN ('exact', 'measured')
                        OR json_type(claim.value, '$.evidence_refs') IS NOT 'array'
                        OR json_type(claim.value, '$.related_node_ids') IS NOT 'array'
                   )
                 )
               )",
        )
        .bind(&[
            JsValue::from_str(review_status),
            JsValue::from_str(&reviewer_id),
            JsValue::from_str(&input.dataset_version),
            JsValue::from_str(&input.page_id),
            JsValue::from_f64(input.revision_number as f64),
            JsValue::from_f64(input.expected_review_version as f64),
        ])
        .map_err(ApiError::internal)?;

    let results = if input.decision == "approve" {
        let approval_version = input.expected_review_version + 1;
        db.batch(vec![
            update_revision,
            db.prepare(
                "UPDATE knowledge_wiki_revisions
                 SET review_status='superseded', review_version=review_version+1
                 WHERE dataset_version=?1 AND page_id=?2
                   AND revision_number<>?3 AND review_status='reviewed'
                   AND EXISTS (
                     SELECT 1 FROM knowledge_wiki_revisions approved
                     WHERE approved.dataset_version=?1 AND approved.page_id=?2
                       AND approved.revision_number=?3
                       AND approved.review_status='reviewed'
                       AND approved.reviewer_id=?4 AND approved.review_version=?5
                   )",
            )
            .bind(&[
                JsValue::from_str(&input.dataset_version),
                JsValue::from_str(&input.page_id),
                JsValue::from_f64(input.revision_number as f64),
                JsValue::from_str(&reviewer_id),
                JsValue::from_f64(approval_version as f64),
            ])
            .map_err(ApiError::internal)?,
            db.prepare(
                "DELETE FROM knowledge_wiki_claims
                 WHERE dataset_version=?1 AND page_id=?2
                   AND EXISTS (
                     SELECT 1 FROM knowledge_wiki_revisions approved
                     WHERE approved.dataset_version=?1 AND approved.page_id=?2
                       AND approved.revision_number=?3
                       AND approved.review_status='reviewed'
                       AND approved.reviewer_id=?4 AND approved.review_version=?5
                   )",
            )
            .bind(&[
                JsValue::from_str(&input.dataset_version),
                JsValue::from_str(&input.page_id),
                JsValue::from_f64(input.revision_number as f64),
                JsValue::from_str(&reviewer_id),
                JsValue::from_f64(approval_version as f64),
            ])
            .map_err(ApiError::internal)?,
            db.prepare(
                "INSERT INTO knowledge_wiki_claims (
                   dataset_version, page_id, claim_id, claim_text_ko,
                   evidence_quality, evidence_refs_json, related_node_ids_json
                 )
                 SELECT revision.dataset_version, revision.page_id,
                        json_extract(claim.value, '$.claim_id'),
                        json_extract(claim.value, '$.text_ko'),
                        json_extract(claim.value, '$.quality'),
                        json_extract(claim.value, '$.evidence_refs'),
                        json_extract(claim.value, '$.related_node_ids')
                 FROM knowledge_wiki_revisions revision,
                      json_each(revision.claims_json) claim
                 WHERE revision.dataset_version=?1 AND revision.page_id=?2
                   AND revision.revision_number=?3
                   AND revision.review_status='reviewed'
                   AND revision.reviewer_id=?4 AND revision.review_version=?5",
            )
            .bind(&[
                JsValue::from_str(&input.dataset_version),
                JsValue::from_str(&input.page_id),
                JsValue::from_f64(input.revision_number as f64),
                JsValue::from_str(&reviewer_id),
                JsValue::from_f64(approval_version as f64),
            ])
            .map_err(ApiError::internal)?,
            db.prepare(
                "UPDATE knowledge_wiki_pages AS page
                 SET title_ko=revision.title_ko,
                     summary_ko=revision.summary_ko,
                     body_markdown=revision.body_markdown,
                     related_node_ids_json=revision.related_node_ids_json,
                     review_status='reviewed',
                     updated_at=CURRENT_TIMESTAMP
                 FROM knowledge_wiki_revisions AS revision
                 WHERE page.dataset_version=?1 AND page.page_id=?2
                   AND revision.dataset_version=page.dataset_version
                   AND revision.page_id=page.page_id
                   AND revision.revision_number=?3
                   AND revision.review_status='reviewed'
                   AND revision.reviewer_id=?4 AND revision.review_version=?5",
            )
            .bind(&[
                JsValue::from_str(&input.dataset_version),
                JsValue::from_str(&input.page_id),
                JsValue::from_f64(input.revision_number as f64),
                JsValue::from_str(&reviewer_id),
                JsValue::from_f64(approval_version as f64),
            ])
            .map_err(ApiError::internal)?,
        ])
        .await
        .map_err(ApiError::internal)?
    } else {
        vec![update_revision.run().await.map_err(ApiError::internal)?]
    };

    if results
        .first()
        .ok_or_else(|| ApiError::internal("Wiki review update returned no result"))
        .and_then(d1_change_count)?
        != 1
    {
        audit(
            db,
            principal,
            "knowledge.wiki_revision.review",
            "knowledge_wiki_revision",
            Some(&target_key),
            "stale_conflict",
            request_id,
        )
        .await?;
        return Err(ApiError::conflict(
            "Wiki revision changed after it was loaded; refresh and retry",
        ));
    }
    audit(
        db,
        principal,
        "knowledge.wiki_revision.review",
        "knowledge_wiki_revision",
        Some(&target_key),
        review_status,
        request_id,
    )
    .await?;
    let row = load_wiki_revision(
        db,
        &input.dataset_version,
        &input.page_id,
        input.revision_number,
    )
    .await?;
    json_response(&json!({"ok": true, "item": row}))
}

fn authorize_review(principal: &PrincipalContext) -> Result<(), ApiError> {
    principal
        .authorize(ProtectedCapability::OperatorAudit)
        .map_err(|_| ApiError::forbidden("operator role required"))
}

fn parse_list_query(
    req: &Request,
    scope: &str,
    allowed_statuses: &[&str],
) -> Result<ParsedReviewList, ApiError> {
    let query = req
        .query::<ReviewListQuery>()
        .map_err(ApiError::bad_request)?;
    let limit = page_limit(query.limit).map_err(ApiError::bad_request)?;
    if query
        .dataset_version
        .as_deref()
        .is_some_and(|value| !valid_short_id(value))
    {
        return Err(ApiError::bad_request("dataset_version filter is invalid"));
    }
    if query
        .status
        .as_deref()
        .is_some_and(|value| !allowed_statuses.contains(&value))
    {
        return Err(ApiError::bad_request("status filter is invalid"));
    }
    let cursor = query
        .cursor
        .as_deref()
        .map(|encoded| {
            StableListCursor::decode_for(
                encoded,
                scope,
                query.dataset_version.as_deref(),
                query.status.as_deref(),
            )
        })
        .transpose()
        .map_err(ApiError::bad_request)?;
    Ok(ParsedReviewList {
        limit,
        dataset_version: query.dataset_version,
        status: query.status,
        cursor,
    })
}

fn validate_error_review(input: &ErrorBookReviewRequest) -> Result<(), ApiError> {
    if !valid_short_id(&input.dataset_version)
        || !valid_short_id(&input.entry_id)
        || input.expected_review_version < 0
        || input.expected_status == input.status
        || !matches!(
            (input.expected_status.as_str(), input.status.as_str()),
            ("open", "accepted" | "rejected" | "resolved")
                | ("accepted", "rejected" | "resolved")
                | ("rejected" | "resolved", "open")
        )
        || input
            .proposed_correction_ko
            .as_deref()
            .is_some_and(|value| {
                let value = value.trim();
                value.is_empty() || value.chars().count() > 2_000
            })
    {
        return Err(ApiError::bad_request(
            "Error Book review transition or precondition is invalid",
        ));
    }
    Ok(())
}

async fn load_error(
    db: &D1Database,
    dataset_version: &str,
    entry_id: &str,
) -> Result<ErrorBookReviewRow, ApiError> {
    db.prepare(
        "SELECT entry_id, dataset_version, subject_kind, subject_id, error_kind,
                diagnosis_ko, proposed_correction_ko, source_refs_json, status,
                detected_by, reviewed_at, created_at, review_version
         FROM knowledge_error_book
         WHERE dataset_version=?1 AND entry_id=?2",
    )
    .bind(&[
        JsValue::from_str(dataset_version),
        JsValue::from_str(entry_id),
    ])
    .map_err(ApiError::internal)?
    .first::<ErrorBookReviewRow>(None)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::internal("reviewed Error Book entry disappeared"))
}

async fn load_manifest_review(
    db: &D1Database,
    dataset_version: &str,
    manifest_sha256: &str,
    review_version: i64,
) -> Result<Option<ManifestReviewRow>, ApiError> {
    db.prepare(format!(
        "{} WHERE dataset_version=?1 AND manifest_sha256=?2 AND review_version=?3",
        manifest_review_select()
    ))
    .bind(&[
        JsValue::from_str(dataset_version),
        JsValue::from_str(manifest_sha256),
        JsValue::from_f64(review_version as f64),
    ])
    .map_err(ApiError::internal)?
    .first::<ManifestReviewRow>(None)
    .await
    .map_err(ApiError::internal)
}

async fn load_wiki_revision(
    db: &D1Database,
    dataset_version: &str,
    page_id: &str,
    revision_number: i64,
) -> Result<WikiRevisionReviewRow, ApiError> {
    db.prepare(
        "SELECT dataset_version, page_id, revision_number, title_ko, summary_ko,
                body_markdown, claims_json, related_node_ids_json, change_origin,
                review_status, generator_id, reviewer_id, change_summary_ko,
                created_at, reviewed_at, review_version
         FROM knowledge_wiki_revisions
         WHERE dataset_version=?1 AND page_id=?2 AND revision_number=?3",
    )
    .bind(&[
        JsValue::from_str(dataset_version),
        JsValue::from_str(page_id),
        JsValue::from_f64(revision_number as f64),
    ])
    .map_err(ApiError::internal)?
    .first::<WikiRevisionReviewRow>(None)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::internal("reviewed Wiki revision disappeared"))
}

async fn load_wiki_revision_gate(
    db: &D1Database,
    dataset_version: &str,
    page_id: &str,
    revision_number: i64,
) -> Result<Option<WikiRevisionGateRow>, ApiError> {
    db.prepare(
        "SELECT review_status, review_version,
                CASE WHEN json_type(claims_json) IS NOT 'array' THEN 1 ELSE (
                  SELECT COUNT(*) FROM json_each(claims_json) claim
                  WHERE json_type(claim.value, '$.claim_id') IS NOT 'text'
                     OR length(trim(json_extract(claim.value, '$.claim_id')))=0
                     OR length(json_extract(claim.value, '$.claim_id'))>256
                     OR json_type(claim.value, '$.text_ko') IS NOT 'text'
                     OR length(trim(json_extract(claim.value, '$.text_ko')))=0
                     OR COALESCE(json_extract(claim.value, '$.quality'), '')
                        NOT IN ('exact', 'measured')
                     OR json_type(claim.value, '$.evidence_refs') IS NOT 'array'
                     OR json_type(claim.value, '$.related_node_ids') IS NOT 'array'
                ) END AS invalid_claim_count
         FROM knowledge_wiki_revisions
         WHERE dataset_version=?1 AND page_id=?2 AND revision_number=?3",
    )
    .bind(&[
        JsValue::from_str(dataset_version),
        JsValue::from_str(page_id),
        JsValue::from_f64(revision_number as f64),
    ])
    .map_err(ApiError::internal)?
    .first::<WikiRevisionGateRow>(None)
    .await
    .map_err(ApiError::internal)
}

fn manifest_review_select() -> &'static str {
    "WITH review AS (
       SELECT manifest.dataset_version, manifest.manifest_sha256,
              manifest.compiler_version, manifest.graph_schema_version,
              manifest.wiki_schema_version, manifest.publication_status,
              manifest.generated_at, manifest.validated_at, manifest.published_at,
              manifest.review_version,
              version.verified AS dataset_verified,
              CASE WHEN version.activated_at IS NULL THEN 0 ELSE 1 END AS dataset_active,
              manifest.source_artifact_count,
              (SELECT COUNT(*) FROM knowledge_source_artifacts source
               WHERE source.dataset_version=manifest.dataset_version)
                AS actual_source_artifact_count,
              manifest.graph_node_count,
              (SELECT COUNT(*) FROM knowledge_graph_nodes node
               WHERE node.dataset_version=manifest.dataset_version)
                AS actual_graph_node_count,
              manifest.graph_edge_count,
              (SELECT COUNT(*) FROM knowledge_graph_edges edge
               WHERE edge.dataset_version=manifest.dataset_version)
                AS actual_graph_edge_count,
              manifest.wiki_page_count,
              (SELECT COUNT(*) FROM knowledge_wiki_pages page
               WHERE page.dataset_version=manifest.dataset_version
                 AND page.review_status='reviewed')
                AS reviewed_wiki_page_count,
              (SELECT COUNT(*) FROM knowledge_error_book error
               WHERE error.dataset_version=manifest.dataset_version
                 AND error.status IN ('open', 'accepted'))
                AS unresolved_error_count,
              (SELECT COUNT(*) FROM knowledge_source_artifacts source
               WHERE source.dataset_version=manifest.dataset_version
                 AND source.evidence_quality NOT IN ('exact', 'measured'))
                AS untrusted_source_count,
              ((SELECT COUNT(*) FROM knowledge_graph_nodes node
                WHERE node.dataset_version=manifest.dataset_version
                  AND node.evidence_quality NOT IN ('exact', 'measured'))
               + (SELECT COUNT(*) FROM knowledge_graph_edges edge
                  WHERE edge.dataset_version=manifest.dataset_version
                    AND edge.evidence_quality NOT IN ('exact', 'measured')))
                AS untrusted_graph_count,
              (SELECT COUNT(*) FROM knowledge_wiki_claims claim
               WHERE claim.dataset_version=manifest.dataset_version
                 AND claim.evidence_quality NOT IN ('exact', 'measured'))
                AS untrusted_claim_count,
              (SELECT COUNT(*) FROM knowledge_pipeline_runs pipeline
               WHERE pipeline.dataset_version=manifest.dataset_version
                 AND pipeline.status='completed')
                AS completed_pipeline_count
       FROM knowledge_dataset_manifests manifest
       JOIN data_versions version USING (dataset_version)
     )
     SELECT *,
       CASE WHEN publication_status='candidate'
                  AND dataset_verified=1
                  AND source_artifact_count > 0
                  AND graph_node_count > 0
                  AND wiki_page_count > 0
                  AND source_artifact_count=actual_source_artifact_count
                  AND graph_node_count=actual_graph_node_count
                  AND graph_edge_count=actual_graph_edge_count
                  AND wiki_page_count=reviewed_wiki_page_count
                  AND unresolved_error_count=0
                  AND untrusted_source_count=0
                  AND untrusted_graph_count=0
                  AND untrusted_claim_count=0
                  AND completed_pipeline_count > 0
            THEN 1 ELSE 0 END AS ready_for_validation
     FROM review"
}

fn d1_change_count(result: &D1Result) -> Result<usize, ApiError> {
    Ok(result
        .meta()
        .map_err(ApiError::internal)?
        .and_then(|meta| meta.changes.or(meta.rows_written))
        .unwrap_or(0))
}

fn optional_js_str(value: Option<&str>) -> JsValue {
    value.map(JsValue::from_str).unwrap_or(JsValue::NULL)
}

fn optional_cursor_str(
    cursor: Option<&StableListCursor>,
    field: fn(&StableListCursor) -> &str,
) -> JsValue {
    cursor
        .map(field)
        .map(JsValue::from_str)
        .unwrap_or(JsValue::NULL)
}

fn optional_cursor_ordinal(cursor: Option<&StableListCursor>) -> JsValue {
    cursor
        .and_then(StableListCursor::row_ordinal)
        .map(|value| JsValue::from_f64(value as f64))
        .unwrap_or(JsValue::NULL)
}
