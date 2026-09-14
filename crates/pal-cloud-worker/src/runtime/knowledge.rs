use std::collections::{BTreeMap, BTreeSet};

use pal_companion_service::{
    AssistantEvidence, AssistantEvidenceKind, KnowledgeQueryIntent, KnowledgeRetrievalPlan,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wasm_bindgen::JsValue;
use worker::{D1Database, Method, Request, Response};

use super::{
    ApiError, KnowledgeRow, digest_like_id, fts_query, json_response, read_json_body, truncate_utf8,
};

const MAX_PUBLIC_QUERY_CHARS: usize = 120;
const MAX_PUBLIC_JSON_BODY_BYTES: usize = 16 * 1024;
const MAX_PUBLIC_RESULTS: u32 = 30;
const MAX_ASSISTANT_KNOWLEDGE_ROWS: usize = 32;
const MAX_GRAPH_EDGES_PER_HOP: usize = 96;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WikiSearchRequest {
    query: String,
    #[serde(default = "default_wiki_limit")]
    limit: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WikiReadRequest {
    page_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelatedRequest {
    node_id: String,
    #[serde(default = "default_related_limit")]
    limit: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WikiPageRow {
    page_id: String,
    page_kind: String,
    title_ko: String,
    summary_ko: String,
    body_markdown: String,
    related_node_ids_json: String,
    source_manifest_sha256: String,
    review_status: String,
    evidence_quality: String,
    evidence_refs_json: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WikiLinkRow {
    direction: String,
    relation_kind: String,
    page_id: String,
    title_ko: String,
    source_edge_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GraphNodeRow {
    node_id: String,
    node_kind: String,
    label_ko: String,
    evidence_quality: String,
    evidence_refs_json: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GraphEdgeRow {
    edge_id: String,
    from_node_id: String,
    from_label_ko: String,
    to_node_id: String,
    to_label_ko: String,
    relation_kind: String,
    evidence_quality: String,
    evidence_refs_json: String,
    attributes_json: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct KnowledgeStatusRow {
    dataset_version: String,
    game_build_id: String,
    manifest_sha256: String,
    compiler_version: String,
    graph_schema_version: String,
    wiki_schema_version: String,
    publication_status: String,
    source_artifact_count: i64,
    graph_node_count: i64,
    graph_edge_count: i64,
    wiki_page_count: i64,
    open_error_count: i64,
}

#[derive(Clone, Debug, Default)]
pub(super) struct RetrievalMetrics {
    pub query_sha256: String,
    pub intent: String,
    pub route: String,
    pub seed_node_count: usize,
    pub graph_edge_count: usize,
    pub wiki_page_count: usize,
    pub source_document_count: usize,
    pub context_row_count: usize,
    pub context_bytes: usize,
}

pub(super) struct RetrievedKnowledge {
    pub evidence: Vec<AssistantEvidence>,
    pub metrics: RetrievalMetrics,
}

pub(super) async fn status_response(db: &D1Database) -> Result<Response, ApiError> {
    let row = db
        .prepare(
            "SELECT dv.dataset_version, dv.game_build_id, manifest.manifest_sha256,
                    manifest.compiler_version, manifest.graph_schema_version,
                    manifest.wiki_schema_version, manifest.publication_status,
                    manifest.source_artifact_count, manifest.graph_node_count,
                    manifest.graph_edge_count, manifest.wiki_page_count,
                    (SELECT COUNT(*) FROM knowledge_error_book errors
                     WHERE errors.dataset_version=dv.dataset_version
                       AND errors.status IN ('open', 'accepted')) AS open_error_count
             FROM data_versions dv
             JOIN knowledge_dataset_manifests manifest
               ON manifest.dataset_version=dv.dataset_version
              WHERE dv.verified=1 AND dv.activated_at IS NOT NULL
              ORDER BY CASE dv.dataset_scope
                         WHEN 'public_metadata' THEN 0 ELSE 1
                       END,
                       dv.activated_at DESC
              LIMIT 1",
        )
        .first::<KnowledgeStatusRow>(None)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::unavailable("활성 지식 데이터셋이 없습니다"))?;
    json_response(&json!({
        "dataset": row,
        "retrieval": {
            "modes": ["wiki_fts", "entity_alias", "bounded_graph"],
            "semantic_search": "planned"
        }
    }))
}

pub(super) async fn wiki_search_response(
    req: &mut Request,
    db: &D1Database,
) -> Result<Response, ApiError> {
    let input: WikiSearchRequest = if req.method() == Method::Get {
        req.query().map_err(ApiError::bad_request)?
    } else {
        read_json_body(req, MAX_PUBLIC_JSON_BODY_BYTES).await?
    };
    let query = validated_query(&input.query)?;
    let limit = input.limit.clamp(1, MAX_PUBLIC_RESULTS);
    let pages = search_wiki(db, None, query, limit as usize).await?;
    json_response(&json!({
        "query": query,
        "count": pages.len(),
        "pages": pages.into_iter().map(wiki_search_item_json).collect::<Vec<_>>()
    }))
}

pub(super) async fn wiki_read_response(
    req: &mut Request,
    db: &D1Database,
) -> Result<Response, ApiError> {
    let input: WikiReadRequest = if req.method() == Method::Get {
        req.query().map_err(ApiError::bad_request)?
    } else {
        read_json_body(req, MAX_PUBLIC_JSON_BODY_BYTES).await?
    };
    if !valid_internal_id(&input.page_id) {
        return Err(ApiError::bad_request(
            "위키 페이지 식별자가 올바르지 않습니다",
        ));
    }
    let page = db
        .prepare(
            "SELECT page.page_id, page.page_kind, page.title_ko, page.summary_ko,
                    page.body_markdown, page.related_node_ids_json,
                    page.source_manifest_sha256, page.review_status,
                    CASE
                      WHEN EXISTS (
                        SELECT 1 FROM knowledge_wiki_claims claim
                        WHERE claim.dataset_version=page.dataset_version
                          AND claim.page_id=page.page_id
                          AND claim.evidence_quality='unknown'
                      ) THEN 'unknown'
                      WHEN EXISTS (
                        SELECT 1 FROM knowledge_wiki_claims claim
                        WHERE claim.dataset_version=page.dataset_version
                          AND claim.page_id=page.page_id
                          AND claim.evidence_quality='model'
                      ) THEN 'model'
                      WHEN EXISTS (
                        SELECT 1 FROM knowledge_wiki_claims claim
                        WHERE claim.dataset_version=page.dataset_version
                          AND claim.page_id=page.page_id
                          AND claim.evidence_quality='measured'
                      ) THEN 'measured'
                      ELSE 'exact'
                    END AS evidence_quality,
                    COALESCE((
                      SELECT json_group_array(source.value)
                      FROM knowledge_wiki_claims claim,
                           json_each(claim.evidence_refs_json) source
                      WHERE claim.dataset_version=page.dataset_version
                        AND claim.page_id=page.page_id
                    ), '[]') AS evidence_refs_json
             FROM knowledge_wiki_pages page
             JOIN data_versions dv USING (dataset_version)
             JOIN knowledge_dataset_manifests manifest USING (dataset_version)
             WHERE page.page_id=?1 AND page.review_status='reviewed'
               AND dv.verified=1 AND dv.activated_at IS NOT NULL
               AND manifest.publication_status='published'
              ORDER BY CASE dv.dataset_scope
                         WHEN 'public_metadata' THEN 0 ELSE 1
                       END,
                       dv.activated_at DESC
              LIMIT 1",
        )
        .bind(&[JsValue::from_str(&input.page_id)])
        .map_err(ApiError::internal)?
        .first::<WikiPageRow>(None)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("검토된 위키 페이지를 찾을 수 없습니다"))?;
    let links = load_wiki_links(db, &input.page_id).await?;
    json_response(&json!({
        "page": wiki_page_json(page),
        "links": links
    }))
}

pub(super) async fn related_response(
    req: &mut Request,
    db: &D1Database,
) -> Result<Response, ApiError> {
    let input: RelatedRequest = if req.method() == Method::Get {
        req.query().map_err(ApiError::bad_request)?
    } else {
        read_json_body(req, MAX_PUBLIC_JSON_BODY_BYTES).await?
    };
    if !valid_internal_id(&input.node_id) {
        return Err(ApiError::bad_request(
            "그래프 항목 식별자가 올바르지 않습니다",
        ));
    }
    let dataset_version = active_dataset_version(db).await?;
    let seed = load_node(db, &dataset_version, &input.node_id)
        .await?
        .ok_or_else(|| ApiError::not_found("그래프 항목을 찾을 수 없습니다"))?;
    let edges = load_edges_for_nodes(
        db,
        &dataset_version,
        std::slice::from_ref(&input.node_id),
        input.limit.clamp(1, MAX_PUBLIC_RESULTS) as usize,
    )
    .await?;
    json_response(&json!({
        "item": graph_node_json(seed),
        "relations": edges.into_iter().map(graph_edge_json).collect::<Vec<_>>()
    }))
}

pub(super) fn question_uses_personal_context(question: &str) -> bool {
    let normalized = normalize_korean_lookup(question);
    [
        "내가",
        "나는",
        "보유",
        "가지고",
        "인벤",
        "창고",
        "내팰",
        "내캐릭터",
        "추천",
        "가능한",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

pub(super) async fn manifest_sha256(
    db: &D1Database,
    dataset_version: &str,
) -> Result<String, ApiError> {
    #[derive(Debug, Deserialize)]
    struct ManifestRow {
        manifest_sha256: String,
    }

    let row = db
        .prepare(
            "SELECT COALESCE(manifest.manifest_sha256, dv.source_hash)
                    AS manifest_sha256
             FROM data_versions dv
             LEFT JOIN knowledge_dataset_manifests manifest
               ON manifest.dataset_version=dv.dataset_version
             WHERE dv.dataset_version=?1
               AND length(COALESCE(manifest.manifest_sha256, dv.source_hash))=64
             LIMIT 1",
        )
        .bind(&[JsValue::from_str(dataset_version)])
        .map_err(ApiError::internal)?
        .first::<ManifestRow>(None)
        .await
        .map_err(ApiError::internal)?;
    Ok(row
        .map(|row| row.manifest_sha256)
        .unwrap_or_else(|| digest_like_id(dataset_version)))
}

pub(super) async fn retrieve_for_assistant(
    db: &D1Database,
    dataset_version: &str,
    question: &str,
    plan: &KnowledgeRetrievalPlan,
) -> Result<RetrievedKnowledge, ApiError> {
    let query = validated_assistant_query(question)?;
    if !dataset_is_published(db, dataset_version).await? {
        return Ok(RetrievedKnowledge {
            evidence: Vec::new(),
            metrics: RetrievalMetrics {
                query_sha256: digest_like_id(query),
                intent: intent_name(plan.intent).to_owned(),
                route: if plan.search_first {
                    "search_first"
                } else {
                    "browse_first"
                }
                .to_owned(),
                ..RetrievalMetrics::default()
            },
        });
    }
    let mut wiki_pages =
        search_wiki(db, Some(dataset_version), query, plan.maximum_wiki_pages).await?;
    let mut seed_ids = resolve_seed_nodes(
        db,
        dataset_version,
        query,
        plan.maximum_seed_nodes,
        &wiki_pages,
    )
    .await?;
    seed_ids.truncate(plan.maximum_seed_nodes);

    let mut edge_rows = Vec::new();
    let mut seen_edges = BTreeSet::new();
    let mut seen_nodes = seed_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut frontier = seed_ids.clone();
    for _ in 0..plan.maximum_graph_hops {
        if frontier.is_empty() || seen_nodes.len() >= plan.maximum_graph_nodes {
            break;
        }
        let rows =
            load_edges_for_nodes(db, dataset_version, &frontier, MAX_GRAPH_EDGES_PER_HOP).await?;
        let mut next = BTreeSet::new();
        for edge in rows {
            if !seen_edges.insert(edge.edge_id.clone()) {
                continue;
            }
            for node_id in [&edge.from_node_id, &edge.to_node_id] {
                if seen_nodes.len() >= plan.maximum_graph_nodes {
                    break;
                }
                if seen_nodes.insert(node_id.clone()) {
                    next.insert(node_id.clone());
                }
            }
            edge_rows.push(edge);
        }
        frontier = next.into_iter().collect();
    }

    if wiki_pages.is_empty() && !seed_ids.is_empty() {
        wiki_pages =
            load_entity_wiki_pages(db, dataset_version, &seed_ids, plan.maximum_wiki_pages).await?;
    }

    let document_rows =
        search_source_documents(db, dataset_version, query, plan.maximum_source_documents).await?;
    let mut evidence = Vec::new();
    for edge in &edge_rows {
        evidence.push(AssistantEvidence {
            evidence_id: bounded_evidence_id("kg-edge", &edge.edge_id),
            kind: AssistantEvidenceKind::KnowledgeGraph,
            title: format!("{} → {}", edge.from_label_ko, edge.to_label_ko),
            detail: graph_edge_detail_ko(edge),
            quality: edge.evidence_quality.clone(),
            source_ids: parse_string_array(&edge.evidence_refs_json),
        });
    }
    for page in &wiki_pages {
        evidence.push(AssistantEvidence {
            evidence_id: bounded_evidence_id("wiki", &page.page_id),
            kind: AssistantEvidenceKind::WikiPage,
            title: page.title_ko.clone(),
            detail: truncate_utf8(&format!("{} {}", page.summary_ko, page.body_markdown), 900),
            quality: page.evidence_quality.clone(),
            source_ids: parse_string_array(&page.evidence_refs_json),
        });
    }
    for row in &document_rows {
        evidence.push(AssistantEvidence {
            evidence_id: bounded_evidence_id("document", &row.document_id),
            kind: AssistantEvidenceKind::OfficialDocument,
            title: row.title.clone(),
            detail: truncate_utf8(&row.content, 900),
            quality: row.confidence.clone(),
            source_ids: row
                .source_url
                .iter()
                .map(|source| bounded_source_id(source))
                .collect(),
        });
    }
    evidence.truncate(MAX_ASSISTANT_KNOWLEDGE_ROWS);

    let context_bytes = evidence
        .iter()
        .map(|row| row.title.len() + row.detail.len())
        .sum();
    Ok(RetrievedKnowledge {
        metrics: RetrievalMetrics {
            query_sha256: digest_like_id(query),
            intent: intent_name(plan.intent).to_owned(),
            route: if plan.search_first {
                "search_first"
            } else {
                "browse_first"
            }
            .to_owned(),
            seed_node_count: seed_ids.len(),
            graph_edge_count: edge_rows.len(),
            wiki_page_count: wiki_pages.len(),
            source_document_count: document_rows.len(),
            context_row_count: evidence.len(),
            context_bytes,
        },
        evidence,
    })
}

pub(super) async fn record_retrieval(
    db: &D1Database,
    retrieval_id: &str,
    assistant_run_id: &str,
    dataset_version: &str,
    metrics: &RetrievalMetrics,
) -> Result<(), ApiError> {
    db.prepare(
        "INSERT INTO knowledge_retrieval_runs (
             retrieval_id, assistant_run_id, dataset_version, query_sha256,
             intent, route, seed_node_count, graph_edge_count, wiki_page_count,
             source_document_count, context_row_count, context_bytes
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )
    .bind(&[
        JsValue::from_str(retrieval_id),
        JsValue::from_str(assistant_run_id),
        JsValue::from_str(dataset_version),
        JsValue::from_str(&metrics.query_sha256),
        JsValue::from_str(&metrics.intent),
        JsValue::from_str(&metrics.route),
        JsValue::from_f64(metrics.seed_node_count as f64),
        JsValue::from_f64(metrics.graph_edge_count as f64),
        JsValue::from_f64(metrics.wiki_page_count as f64),
        JsValue::from_f64(metrics.source_document_count as f64),
        JsValue::from_f64(metrics.context_row_count as f64),
        JsValue::from_f64(metrics.context_bytes as f64),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn active_dataset_version(db: &D1Database) -> Result<String, ApiError> {
    #[derive(Debug, Deserialize)]
    struct ActiveRow {
        dataset_version: String,
    }

    db.prepare(
        "SELECT dataset_version FROM data_versions
         JOIN knowledge_dataset_manifests manifest USING (dataset_version)
         WHERE verified=1 AND activated_at IS NOT NULL
           AND manifest.publication_status='published'
         ORDER BY CASE dataset_scope
                    WHEN 'public_metadata' THEN 0 ELSE 1
                  END,
                  activated_at DESC
         LIMIT 1",
    )
    .first::<ActiveRow>(None)
    .await
    .map_err(ApiError::internal)?
    .map(|row| row.dataset_version)
    .ok_or_else(|| ApiError::unavailable("활성 데이터셋이 없습니다"))
}

async fn dataset_is_published(db: &D1Database, dataset_version: &str) -> Result<bool, ApiError> {
    #[derive(Debug, Deserialize)]
    struct CountRow {
        count: i64,
    }

    let row = db
        .prepare(
            "SELECT COUNT(*) AS count FROM knowledge_dataset_manifests
             WHERE dataset_version=?1 AND publication_status='published'",
        )
        .bind(&[JsValue::from_str(dataset_version)])
        .map_err(ApiError::internal)?
        .first::<CountRow>(None)
        .await
        .map_err(ApiError::internal)?;
    Ok(row.is_some_and(|row| row.count == 1))
}

async fn search_wiki(
    db: &D1Database,
    dataset_version: Option<&str>,
    query: &str,
    limit: usize,
) -> Result<Vec<WikiPageRow>, ApiError> {
    let dataset = match dataset_version {
        Some(value) => value.to_owned(),
        None => active_dataset_version(db).await?,
    };
    let fts = fts_query(query);
    if fts.is_empty() {
        return Ok(Vec::new());
    }
    let result = db
        .prepare(
            "SELECT page.page_id, page.page_kind, page.title_ko, page.summary_ko,
                    page.body_markdown, page.related_node_ids_json,
                    page.source_manifest_sha256, page.review_status,
                    CASE
                      WHEN EXISTS (
                        SELECT 1 FROM knowledge_wiki_claims claim
                        WHERE claim.dataset_version=page.dataset_version
                          AND claim.page_id=page.page_id
                          AND claim.evidence_quality='unknown'
                      ) THEN 'unknown'
                      WHEN EXISTS (
                        SELECT 1 FROM knowledge_wiki_claims claim
                        WHERE claim.dataset_version=page.dataset_version
                          AND claim.page_id=page.page_id
                          AND claim.evidence_quality='model'
                      ) THEN 'model'
                      WHEN EXISTS (
                        SELECT 1 FROM knowledge_wiki_claims claim
                        WHERE claim.dataset_version=page.dataset_version
                          AND claim.page_id=page.page_id
                          AND claim.evidence_quality='measured'
                      ) THEN 'measured'
                      ELSE 'exact'
                    END AS evidence_quality,
                    COALESCE((
                      SELECT json_group_array(source.value)
                      FROM knowledge_wiki_claims claim,
                           json_each(claim.evidence_refs_json) source
                      WHERE claim.dataset_version=page.dataset_version
                        AND claim.page_id=page.page_id
                    ), '[]') AS evidence_refs_json
             FROM knowledge_wiki_fts fts
             JOIN knowledge_wiki_pages page
               ON page.dataset_version=fts.dataset_version
              AND page.page_id=fts.page_id
             WHERE knowledge_wiki_fts MATCH ?1
               AND page.dataset_version=?2 AND page.review_status='reviewed'
             ORDER BY bm25(knowledge_wiki_fts), page.title_ko
             LIMIT ?3",
        )
        .bind(&[
            JsValue::from_str(&fts),
            JsValue::from_str(&dataset),
            JsValue::from_f64(limit.clamp(1, MAX_PUBLIC_RESULTS as usize) as f64),
        ])
        .map_err(ApiError::internal)?
        .all()
        .await;
    match result {
        Ok(rows) => rows.results::<WikiPageRow>().map_err(ApiError::internal),
        Err(_) => search_wiki_like(db, &dataset, query, limit).await,
    }
}

async fn search_wiki_like(
    db: &D1Database,
    dataset_version: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<WikiPageRow>, ApiError> {
    let pattern = format!("%{}%", query.trim());
    db.prepare(
        "SELECT page.page_id, page.page_kind, page.title_ko, page.summary_ko,
                page.body_markdown, page.related_node_ids_json,
                page.source_manifest_sha256, page.review_status,
                'exact' AS evidence_quality,
                COALESCE((
                  SELECT json_group_array(source.value)
                  FROM knowledge_wiki_claims claim,
                       json_each(claim.evidence_refs_json) source
                  WHERE claim.dataset_version=page.dataset_version
                    AND claim.page_id=page.page_id
                ), '[]') AS evidence_refs_json
         FROM knowledge_wiki_pages page
         WHERE page.dataset_version=?1 AND page.review_status='reviewed'
           AND (page.title_ko LIKE ?2 OR page.summary_ko LIKE ?2)
         ORDER BY page.title_ko LIMIT ?3",
    )
    .bind(&[
        JsValue::from_str(dataset_version),
        JsValue::from_str(&pattern),
        JsValue::from_f64(limit.clamp(1, MAX_PUBLIC_RESULTS as usize) as f64),
    ])
    .map_err(ApiError::internal)?
    .all()
    .await
    .map_err(ApiError::internal)?
    .results::<WikiPageRow>()
    .map_err(ApiError::internal)
}

async fn resolve_seed_nodes(
    db: &D1Database,
    dataset_version: &str,
    question: &str,
    limit: usize,
    wiki_pages: &[WikiPageRow],
) -> Result<Vec<String>, ApiError> {
    let mut seeds = wiki_pages
        .iter()
        .flat_map(|page| parse_string_array(&page.related_node_ids_json))
        .filter(|node_id| !node_id.starts_with("wiki-page:"))
        .collect::<BTreeSet<_>>();
    let normalized = normalize_korean_lookup(question);
    let rows = db
        .prepare(
            "SELECT DISTINCT
               CASE alias.entity_kind
                 WHEN 'species' THEN 'species:' || alias.entity_id
                 WHEN 'active_skill' THEN 'active-skill:' || alias.entity_id
                 WHEN 'passive' THEN 'passive-skill:' || alias.entity_id
                 WHEN 'item' THEN 'item:' || alias.entity_id
               END AS node_id
             FROM catalog_aliases alias
             JOIN knowledge_graph_nodes node
               ON node.dataset_version=alias.dataset_version
              AND node.node_id=CASE alias.entity_kind
                 WHEN 'species' THEN 'species:' || alias.entity_id
                 WHEN 'active_skill' THEN 'active-skill:' || alias.entity_id
                 WHEN 'passive' THEN 'passive-skill:' || alias.entity_id
                 WHEN 'item' THEN 'item:' || alias.entity_id
               END
             WHERE alias.dataset_version=?1
               AND length(alias.normalized_alias) >= 2
               AND instr(?2, lower(replace(alias.normalized_alias, ' ', ''))) > 0
             ORDER BY length(alias.normalized_alias) DESC, node_id
             LIMIT ?3",
        )
        .bind(&[
            JsValue::from_str(dataset_version),
            JsValue::from_str(&normalized),
            JsValue::from_f64(limit.clamp(1, 16) as f64),
        ])
        .map_err(ApiError::internal)?
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<SeedNodeRow>()
        .map_err(ApiError::internal)?;
    for row in rows {
        seeds.insert(row.node_id);
    }
    Ok(seeds.into_iter().take(limit).collect())
}

#[derive(Debug, Deserialize)]
struct SeedNodeRow {
    node_id: String,
}

async fn load_node(
    db: &D1Database,
    dataset_version: &str,
    node_id: &str,
) -> Result<Option<GraphNodeRow>, ApiError> {
    db.prepare(
        "SELECT node_id, node_kind, label_ko, evidence_quality, evidence_refs_json
         FROM knowledge_graph_nodes
         WHERE dataset_version=?1 AND node_id=?2 LIMIT 1",
    )
    .bind(&[
        JsValue::from_str(dataset_version),
        JsValue::from_str(node_id),
    ])
    .map_err(ApiError::internal)?
    .first::<GraphNodeRow>(None)
    .await
    .map_err(ApiError::internal)
}

async fn load_edges_for_nodes(
    db: &D1Database,
    dataset_version: &str,
    node_ids: &[String],
    limit: usize,
) -> Result<Vec<GraphEdgeRow>, ApiError> {
    if node_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (0..node_ids.len())
        .map(|index| format!("?{}", index + 2))
        .collect::<Vec<_>>()
        .join(", ");
    let limit_parameter = node_ids.len() + 2;
    let sql = format!(
        "SELECT edge.edge_id, edge.from_node_id, source.label_ko AS from_label_ko,
                edge.to_node_id, target.label_ko AS to_label_ko,
                edge.relation_kind, edge.evidence_quality,
                edge.evidence_refs_json, edge.attributes_json
         FROM knowledge_graph_edges edge
         JOIN knowledge_graph_nodes source
           ON source.dataset_version=edge.dataset_version
          AND source.node_id=edge.from_node_id
         JOIN knowledge_graph_nodes target
           ON target.dataset_version=edge.dataset_version
          AND target.node_id=edge.to_node_id
         WHERE edge.dataset_version=?1
           AND (edge.from_node_id IN ({placeholders})
                OR edge.to_node_id IN ({placeholders}))
           AND edge.relation_kind <> 'references'
         ORDER BY edge.relation_kind, edge.edge_id
         LIMIT ?{limit_parameter}"
    );
    let mut bindings = Vec::with_capacity(node_ids.len() + 2);
    bindings.push(JsValue::from_str(dataset_version));
    for node_id in node_ids {
        bindings.push(JsValue::from_str(node_id));
    }
    bindings.push(JsValue::from_f64(
        limit.clamp(1, MAX_GRAPH_EDGES_PER_HOP) as f64
    ));
    db.prepare(&sql)
        .bind(&bindings)
        .map_err(ApiError::internal)?
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<GraphEdgeRow>()
        .map_err(ApiError::internal)
}

async fn load_entity_wiki_pages(
    db: &D1Database,
    dataset_version: &str,
    node_ids: &[String],
    limit: usize,
) -> Result<Vec<WikiPageRow>, ApiError> {
    if node_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (0..node_ids.len())
        .map(|index| format!("?{}", index + 2))
        .collect::<Vec<_>>()
        .join(", ");
    let limit_parameter = node_ids.len() + 2;
    let sql = format!(
        "SELECT page.page_id, page.page_kind, page.title_ko, page.summary_ko,
                page.body_markdown, page.related_node_ids_json,
                page.source_manifest_sha256, page.review_status,
                'exact' AS evidence_quality,
                COALESCE((
                  SELECT json_group_array(source.value)
                  FROM knowledge_wiki_claims claim,
                       json_each(claim.evidence_refs_json) source
                  WHERE claim.dataset_version=page.dataset_version
                    AND claim.page_id=page.page_id
                ), '[]') AS evidence_refs_json
         FROM knowledge_wiki_pages page
         WHERE page.dataset_version=?1 AND page.review_status='reviewed'
           AND page.page_id IN ({placeholders})
         ORDER BY page.title_ko LIMIT ?{limit_parameter}"
    );
    let mut bindings = Vec::with_capacity(node_ids.len() + 2);
    bindings.push(JsValue::from_str(dataset_version));
    for node_id in node_ids {
        bindings.push(JsValue::from_str(&format!("wiki:{node_id}")));
    }
    bindings.push(JsValue::from_f64(limit.clamp(1, 16) as f64));
    db.prepare(&sql)
        .bind(&bindings)
        .map_err(ApiError::internal)?
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<WikiPageRow>()
        .map_err(ApiError::internal)
}

async fn search_source_documents(
    db: &D1Database,
    dataset_version: &str,
    question: &str,
    limit: usize,
) -> Result<Vec<KnowledgeRow>, ApiError> {
    let fts = fts_query(question);
    if fts.is_empty() {
        return Ok(Vec::new());
    }
    let result = db
        .prepare(
            "SELECT document.document_id, document.title, document.content,
                    document.source_url, document.confidence
             FROM knowledge_fts fts
             JOIN knowledge_documents document
               ON document.document_id=fts.document_id
              AND document.dataset_version=fts.dataset_version
             WHERE knowledge_fts MATCH ?1 AND document.dataset_version=?2
             ORDER BY bm25(knowledge_fts) LIMIT ?3",
        )
        .bind(&[
            JsValue::from_str(&fts),
            JsValue::from_str(dataset_version),
            JsValue::from_f64(limit.clamp(1, 8) as f64),
        ])
        .map_err(ApiError::internal)?
        .all()
        .await;
    match result {
        Ok(rows) => rows.results::<KnowledgeRow>().map_err(ApiError::internal),
        Err(_) => Ok(Vec::new()),
    }
}

async fn load_wiki_links(db: &D1Database, page_id: &str) -> Result<Vec<WikiLinkRow>, ApiError> {
    let dataset = active_dataset_version(db).await?;
    db.prepare(
        "SELECT 'outgoing' AS direction, link.relation_kind,
                target.page_id, target.title_ko, link.source_edge_id
         FROM knowledge_wiki_links link
         JOIN knowledge_wiki_pages target
           ON target.dataset_version=link.dataset_version
          AND target.page_id=link.to_page_id
         WHERE link.dataset_version=?1 AND link.from_page_id=?2
         UNION ALL
         SELECT 'incoming' AS direction, link.relation_kind,
                source.page_id, source.title_ko, link.source_edge_id
         FROM knowledge_wiki_links link
         JOIN knowledge_wiki_pages source
           ON source.dataset_version=link.dataset_version
          AND source.page_id=link.from_page_id
         WHERE link.dataset_version=?1 AND link.to_page_id=?2
         ORDER BY relation_kind, title_ko LIMIT 12",
    )
    .bind(&[JsValue::from_str(&dataset), JsValue::from_str(page_id)])
    .map_err(ApiError::internal)?
    .all()
    .await
    .map_err(ApiError::internal)?
    .results::<WikiLinkRow>()
    .map_err(ApiError::internal)
}

fn graph_edge_detail_ko(edge: &GraphEdgeRow) -> String {
    let relation = relation_label_ko(&edge.relation_kind);
    let attributes = attributes_ko(&edge.attributes_json);
    if attributes.is_empty() {
        format!(
            "{}에서 {}로 이어지는 {} 관계입니다.",
            edge.from_label_ko, edge.to_label_ko, relation
        )
    } else {
        format!(
            "{}에서 {}로 이어지는 {} 관계입니다. {}",
            edge.from_label_ko, edge.to_label_ko, relation, attributes
        )
    }
}

fn attributes_ko(attributes_json: &str) -> String {
    let Ok(Value::Object(values)) = serde_json::from_str(attributes_json) else {
        return String::new();
    };
    let labels = BTreeMap::from([
        ("learn_level", "습득 레벨"),
        ("quantity", "수량"),
        ("quantity_min", "최소 수량"),
        ("quantity_max", "최대 수량"),
        ("probability", "확률"),
        ("work_amount", "작업량"),
        ("role", "역할"),
        ("requirement_kind", "필요 조건"),
    ]);
    values
        .iter()
        .filter_map(|(key, value)| {
            labels
                .get(key.as_str())
                .filter(|_| !value.is_null())
                .map(|label| format!("{label}: {}", scalar_value_ko(value)))
        })
        .take(6)
        .collect::<Vec<_>>()
        .join(", ")
}

fn scalar_value_ko(value: &Value) -> String {
    match value {
        Value::String(value) => match value.as_str() {
            "parent_a" => "첫 번째 부모".to_owned(),
            "parent_b" => "두 번째 부모".to_owned(),
            "unlock_item" => "해금 아이템".to_owned(),
            _ => value.clone(),
        },
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => if *value { "예" } else { "아니요" }.to_owned(),
        _ => value.to_string(),
    }
}

fn relation_label_ko(relation: &str) -> &'static str {
    match relation {
        "learns_skill" => "습득 스킬",
        "has_guaranteed_passive" => "확정 패시브",
        "drops_item" => "드롭",
        "ingredient_for" => "제작 재료",
        "produces_item" => "제작 결과",
        "sold_by" => "판매",
        "unlocks" => "해금",
        "located_at" => "위치",
        "requires" => "필요 조건",
        "breeding_parent" => "교배 부모",
        "breeds_into" => "교배 결과",
        "references" => "근거",
        _ => "관련",
    }
}

fn wiki_page_json(page: WikiPageRow) -> Value {
    json!({
        "page_id": page.page_id,
        "kind": page.page_kind,
        "title": page.title_ko,
        "summary": page.summary_ko,
        "body_markdown": page.body_markdown,
        "related_items": parse_string_array(&page.related_node_ids_json),
        "manifest_sha256": page.source_manifest_sha256,
        "review_status": page.review_status,
        "quality": page.evidence_quality,
        "evidence": parse_string_array(&page.evidence_refs_json)
    })
}

fn wiki_search_item_json(page: WikiPageRow) -> Value {
    json!({
        "page_id": page.page_id,
        "kind": page.page_kind,
        "title": page.title_ko,
        "summary": page.summary_ko,
        "related_items": parse_string_array(&page.related_node_ids_json),
        "manifest_sha256": page.source_manifest_sha256,
        "review_status": page.review_status,
        "quality": page.evidence_quality
    })
}

fn graph_node_json(node: GraphNodeRow) -> Value {
    json!({
        "node_id": node.node_id,
        "kind": node.node_kind,
        "name": node.label_ko,
        "quality": node.evidence_quality,
        "evidence": parse_string_array(&node.evidence_refs_json)
    })
}

fn graph_edge_json(edge: GraphEdgeRow) -> Value {
    let description = graph_edge_detail_ko(&edge);
    json!({
        "relation": relation_label_ko(&edge.relation_kind),
        "direction": "canonical",
        "from": {
            "node_id": edge.from_node_id,
            "name": edge.from_label_ko
        },
        "to": {
            "node_id": edge.to_node_id,
            "name": edge.to_label_ko
        },
        "description": description,
        "quality": edge.evidence_quality,
        "evidence": parse_string_array(&edge.evidence_refs_json)
    })
}

fn parse_string_array(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<Value>>(value)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(bounded_source_id))
        .take(32)
        .collect()
}

fn validated_query(query: &str) -> Result<&str, ApiError> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > MAX_PUBLIC_QUERY_CHARS {
        return Err(ApiError::bad_request(
            "검색어는 1자 이상 120자 이하여야 합니다",
        ));
    }
    Ok(query)
}

fn validated_assistant_query(query: &str) -> Result<&str, ApiError> {
    let query = query.trim();
    if query.is_empty() || query.len() > pal_companion_service::MAX_QUESTION_BYTES {
        return Err(ApiError::bad_request(
            "도우미 질문 크기가 올바르지 않습니다",
        ));
    }
    Ok(query)
}

fn normalize_korean_lookup(value: &str) -> String {
    value
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn intent_name(intent: KnowledgeQueryIntent) -> &'static str {
    match intent {
        KnowledgeQueryIntent::EntityLookup => "entity_lookup",
        KnowledgeQueryIntent::Relationship => "relationship",
        KnowledgeQueryIntent::Comparison => "comparison",
        KnowledgeQueryIntent::Overview => "overview",
    }
}

fn bounded_evidence_id(prefix: &str, value: &str) -> String {
    let candidate = format!("{prefix}:{value}");
    if candidate.len() <= 128 && valid_internal_id(&candidate) {
        candidate
    } else {
        format!("{prefix}:{}", digest_like_id(&candidate))
    }
}

fn bounded_source_id(value: &str) -> String {
    if value.len() <= 128 && valid_internal_id(value) {
        value.to_owned()
    } else {
        format!("source:{}", digest_like_id(value))
    }
}

fn valid_internal_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.is_ascii()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn default_wiki_limit() -> u32 {
    12
}

fn default_related_limit() -> u32 {
    24
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personal_context_is_only_loaded_for_personal_questions() {
        assert!(question_uses_personal_context(
            "내가 가진 팰로 무엇을 만들 수 있어?"
        ));
        assert!(!question_uses_personal_context(
            "페스키의 드롭 아이템은 뭐야?"
        ));
    }

    #[test]
    fn graph_attributes_are_presented_in_korean() {
        let text =
            attributes_ko(r#"{"quantity":2,"work_amount":100,"role":"parent_a","ignored":true}"#);
        assert!(text.contains("수량: 2"));
        assert!(text.contains("작업량: 100"));
        assert!(text.contains("역할: 첫 번째 부모"));
        assert!(!text.contains("ignored"));
    }

    #[test]
    fn evidence_ids_are_bounded() {
        let id = bounded_evidence_id("kg-edge", &"x".repeat(300));
        assert!(id.len() <= 128);
        assert!(valid_internal_id(&id));
    }
}
