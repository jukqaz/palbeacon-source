use rusqlite::{Connection, params};

const MIGRATIONS: &[&str] = &[
    include_str!("../../../cloudflare/migrations/0001_private_companion.sql"),
    include_str!("../../../cloudflare/migrations/0002_catalog_search.sql"),
    include_str!("../../../cloudflare/migrations/0003_catalog_species_detail.sql"),
    include_str!("../../../cloudflare/migrations/0004_catalog_skills.sql"),
    include_str!("../../../cloudflare/migrations/0005_catalog_items_detail.sql"),
    include_str!("../../../cloudflare/migrations/0006_game_build_compatibility.sql"),
    include_str!("../../../cloudflare/migrations/0007_knowledge_graph_wiki.sql"),
    include_str!("../../../cloudflare/migrations/0008_knowledge_operations.sql"),
    include_str!("../../../cloudflare/migrations/0009_knowledge_operator_review.sql"),
    include_str!("../../../cloudflare/migrations/0010_knowledge_review_pagination.sql"),
    include_str!("../../../cloudflare/migrations/0011_dataset_scope.sql"),
];
const POST_MIGRATION_TRIGGERS: &str =
    include_str!("../../../cloudflare/schema/post_migration_triggers.sql");

fn migrated() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    for migration in MIGRATIONS {
        connection.execute_batch(migration).unwrap();
    }
    connection.execute_batch(POST_MIGRATION_TRIGGERS).unwrap();
    connection
        .execute(
            "INSERT INTO data_versions (
                dataset_version, game_version, game_build_id, schema_version,
                source_hash, verified
             ) VALUES ('fixture', '1.0.2', 'steam:24425675', '7', ?1, 1)",
            ["1".repeat(64)],
        )
        .unwrap();
    connection
}

#[test]
fn exact_graph_enforces_endpoints_and_quarantines_model_suggestions() {
    let connection = migrated();
    for (node_id, kind, label) in [
        ("species:SkyDragon", "species", "페스키"),
        ("item:Crystal", "item", "수정"),
    ] {
        connection
            .execute(
                "INSERT INTO knowledge_graph_nodes (
                    dataset_version, node_id, node_kind, label_ko,
                    evidence_quality, evidence_refs_json, content_hash
                 ) VALUES ('fixture', ?1, ?2, ?3, 'exact', '[\"fixture\"]', ?4)",
                params![node_id, kind, label, "2".repeat(64)],
            )
            .unwrap();
    }

    connection
        .execute(
            "INSERT INTO knowledge_graph_edges (
                dataset_version, edge_id, from_node_id, to_node_id,
                relation_kind, origin, evidence_quality, evidence_refs_json,
                attributes_json, content_hash
             ) VALUES (
                'fixture', 'drop:SkyDragon:Crystal', 'species:SkyDragon',
                'item:Crystal', 'drops_item', 'exact_extract', 'exact',
                '[\"fixture\"]', '{}', ?1
             )",
            ["3".repeat(64)],
        )
        .unwrap();

    let missing_endpoint = connection.execute(
        "INSERT INTO knowledge_graph_edges (
            dataset_version, edge_id, from_node_id, to_node_id,
            relation_kind, origin, evidence_quality, evidence_refs_json,
            attributes_json, content_hash
         ) VALUES (
            'fixture', 'bad:endpoint', 'species:SkyDragon', 'item:Missing',
            'drops_item', 'exact_extract', 'exact', '[\"fixture\"]', '{}', ?1
         )",
        ["4".repeat(64)],
    );
    assert!(missing_endpoint.is_err());

    let model_edge = connection.execute(
        "INSERT INTO knowledge_graph_edges (
            dataset_version, edge_id, from_node_id, to_node_id,
            relation_kind, origin, evidence_quality, evidence_refs_json,
            attributes_json, content_hash
         ) VALUES (
            'fixture', 'bad:model', 'species:SkyDragon', 'item:Crystal',
            'related_to', 'llm_suggestion', 'model', '[\"fixture\"]', '{}', ?1
         )",
        ["5".repeat(64)],
    );
    assert!(model_edge.is_err());

    connection
        .execute(
            "INSERT INTO knowledge_edge_suggestions (
                suggestion_id, dataset_version, from_node_id, to_node_id,
                proposed_relation_kind, rationale, model_id, source_refs_json
             ) VALUES (
                'suggestion:one', 'fixture', 'species:SkyDragon', 'item:Crystal',
                'related_to', '문서에서 함께 언급됨', 'fixture-model',
                '[\"document:one\"]'
             )",
            [],
        )
        .unwrap();
}

#[test]
fn only_reviewed_wiki_pages_enter_full_text_search() {
    let connection = migrated();
    connection
        .execute(
            "INSERT INTO knowledge_wiki_pages (
                dataset_version, page_id, page_kind, title_ko, summary_ko,
                body_markdown, source_manifest_sha256, review_status
             ) VALUES (
                'fixture', 'wiki:draft', 'guide', '초안', '검토 전',
                '# 초안', ?1, 'draft'
             )",
            ["6".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_wiki_pages (
                dataset_version, page_id, page_kind, title_ko, summary_ko,
                body_markdown, source_manifest_sha256, review_status
             ) VALUES (
                'fixture', 'wiki:reviewed', 'guide', '수정 파밍',
                '검증된 획득 관계', '# 수정 파밍', ?1, 'reviewed'
             )",
            ["7".repeat(64)],
        )
        .unwrap();

    let indexed = connection
        .query_row(
            "SELECT COUNT(*) FROM knowledge_wiki_fts
             WHERE knowledge_wiki_fts MATCH '수정'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(indexed, 1);

    connection
        .execute(
            "UPDATE knowledge_wiki_pages
             SET review_status='superseded'
             WHERE dataset_version='fixture' AND page_id='wiki:reviewed'",
            [],
        )
        .unwrap();
    let remaining = connection
        .query_row("SELECT COUNT(*) FROM knowledge_wiki_fts", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    assert_eq!(remaining, 0);
}

#[test]
fn dataset_lineage_wiki_revisions_and_error_book_remain_auditable() {
    let connection = migrated();
    connection
        .execute(
            "INSERT INTO knowledge_dataset_manifests (
                dataset_version, manifest_sha256, compiler_version,
                graph_schema_version, wiki_schema_version, publication_status
             ) VALUES (
                'fixture', ?1, 'compiler-v1', 'graph-v1', 'wiki-v1', 'validated'
             )",
            ["8".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_source_artifacts (
                dataset_version, artifact_id, artifact_kind, logical_source_ref,
                sha256, byte_size, evidence_quality
             ) VALUES (
                'fixture', 'artifact:catalog', 'catalog', 'catalog:fixture',
                ?1, 100, 'exact'
             )",
            ["9".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_pipeline_runs (
                run_id, dataset_version, pipeline_name, compiler_version,
                status, completed_at
             ) VALUES (
                'run:compile', 'fixture', 'knowledge-compile', 'compiler-v1',
                'completed', CURRENT_TIMESTAMP
             )",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_lineage_edges (
                dataset_version, lineage_id, run_id, input_artifact_id,
                output_kind, output_id, transform_kind
             ) VALUES (
                'fixture', 'lineage:graph', 'run:compile', 'artifact:catalog',
                'graph', 'graph:fixture', 'exact_projection'
             )",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_wiki_pages (
                dataset_version, page_id, page_kind, title_ko, summary_ko,
                body_markdown, source_manifest_sha256, review_status
             ) VALUES (
                'fixture', 'wiki:pal', 'entity', '페스키', '검증된 팰 정보',
                '# 페스키', ?1, 'reviewed'
             )",
            ["8".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_wiki_revisions (
                dataset_version, page_id, revision_number, title_ko, summary_ko,
                body_markdown, change_origin, review_status, change_summary_ko
             ) VALUES (
                'fixture', 'wiki:pal', 1, '페스키', '검증된 팰 정보',
                '# 페스키', 'exact_compiler', 'reviewed', '최초 컴파일'
             )",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_error_book (
                entry_id, dataset_version, subject_kind, subject_id, error_kind,
                diagnosis_ko, detected_by
             ) VALUES (
                'error:one', 'fixture', 'wiki_page', 'wiki:pal',
                'missing_detail', '세부 설명 보강 필요', 'validator'
             )",
            [],
        )
        .unwrap();

    let audit_counts = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM knowledge_lineage_edges),
               (SELECT COUNT(*) FROM knowledge_wiki_revisions),
               (SELECT COUNT(*) FROM knowledge_error_book WHERE status='open')",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(audit_counts, (1, 1, 1));

    connection
        .execute(
            "UPDATE data_versions SET activated_at=CURRENT_TIMESTAMP
             WHERE dataset_version='fixture'",
            [],
        )
        .unwrap();
    let blocked_status: String = connection
        .query_row(
            "SELECT publication_status FROM knowledge_dataset_manifests
             WHERE dataset_version='fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(blocked_status, "candidate");

    connection
        .execute(
            "UPDATE knowledge_error_book SET status='resolved', reviewed_at=CURRENT_TIMESTAMP
             WHERE entry_id='error:one'",
            [],
        )
        .unwrap();
    let published_status: String = connection
        .query_row(
            "SELECT publication_status FROM knowledge_dataset_manifests
             WHERE dataset_version='fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(published_status, "published");
}

#[test]
fn publication_gate_rejects_direct_publish_and_demotes_reopened_errors() {
    let connection = migrated();
    connection
        .execute(
            "UPDATE data_versions SET activated_at=CURRENT_TIMESTAMP
             WHERE dataset_version='fixture'",
            [],
        )
        .unwrap();

    let direct_insert = connection.execute(
        "INSERT INTO knowledge_dataset_manifests (
            dataset_version, manifest_sha256, compiler_version,
            graph_schema_version, wiki_schema_version, publication_status
         ) VALUES ('fixture', ?1, 'compiler-v1', 'graph-v1', 'wiki-v1', 'published')",
        ["a".repeat(64)],
    );
    assert!(direct_insert.is_err());

    connection
        .execute(
            "INSERT INTO knowledge_dataset_manifests (
                dataset_version, manifest_sha256, compiler_version,
                graph_schema_version, wiki_schema_version, publication_status
             ) VALUES ('fixture', ?1, 'compiler-v1', 'graph-v1', 'wiki-v1', 'candidate')",
            ["b".repeat(64)],
        )
        .unwrap();
    let direct_update = connection.execute(
        "UPDATE knowledge_dataset_manifests SET publication_status='published'
         WHERE dataset_version='fixture'",
        [],
    );
    assert!(direct_update.is_err());

    connection
        .execute(
            "UPDATE knowledge_dataset_manifests SET publication_status='validated'
             WHERE dataset_version='fixture'",
            [],
        )
        .unwrap();
    let published: String = connection
        .query_row(
            "SELECT publication_status FROM knowledge_dataset_manifests
             WHERE dataset_version='fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(published, "published");

    connection
        .execute(
            "INSERT INTO knowledge_error_book (
                entry_id, dataset_version, subject_kind, subject_id, error_kind,
                diagnosis_ko, status, detected_by
             ) VALUES (
                'error:reopened', 'fixture', 'wiki_page', 'wiki:pal',
                'missing_detail', '검증 오류', 'resolved', 'validator'
             )",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE knowledge_error_book SET status='open'
             WHERE dataset_version='fixture' AND entry_id='error:reopened'",
            [],
        )
        .unwrap();
    let demoted: String = connection
        .query_row(
            "SELECT publication_status FROM knowledge_dataset_manifests
             WHERE dataset_version='fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(demoted, "candidate");
}

#[test]
fn error_book_entry_ids_are_scoped_to_dataset_version() {
    let connection = migrated();
    connection
        .execute(
            "INSERT INTO data_versions (
                dataset_version, game_version, game_build_id, schema_version,
                source_hash, verified
             ) VALUES ('fixture-next', '1.0.3', 'steam:next', '7', ?1, 1)",
            ["c".repeat(64)],
        )
        .unwrap();
    for dataset_version in ["fixture", "fixture-next"] {
        connection
            .execute(
                "INSERT INTO knowledge_error_book (
                    entry_id, dataset_version, subject_kind, subject_id,
                    error_kind, diagnosis_ko, detected_by
                 ) VALUES (
                    'error:same-relation', ?1, 'graph_edge', 'edge:one',
                    'missing_endpoint', '관계 끝점 누락', 'validator'
                 )",
                [dataset_version],
            )
            .unwrap();
    }
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM knowledge_error_book
             WHERE entry_id='error:same-relation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn operator_review_versions_reject_stale_error_and_wiki_decisions() {
    let connection = migrated();
    connection
        .execute(
            "INSERT INTO knowledge_error_book (
                entry_id, dataset_version, subject_kind, subject_id, error_kind,
                diagnosis_ko, detected_by
             ) VALUES (
                'error:review', 'fixture', 'wiki_page', 'wiki:review',
                'missing_detail', '검수 필요', 'validator'
             )",
            [],
        )
        .unwrap();
    let changed = connection
        .execute(
            "UPDATE knowledge_error_book
             SET status='resolved', reviewed_at=CURRENT_TIMESTAMP,
                 review_version=review_version+1
             WHERE dataset_version='fixture' AND entry_id='error:review'
               AND status='open' AND review_version=0",
            [],
        )
        .unwrap();
    assert_eq!(changed, 1);
    let stale = connection
        .execute(
            "UPDATE knowledge_error_book
             SET status='rejected', review_version=review_version+1
             WHERE dataset_version='fixture' AND entry_id='error:review'
               AND status='open' AND review_version=0",
            [],
        )
        .unwrap();
    assert_eq!(stale, 0);

    connection
        .execute(
            "INSERT INTO knowledge_wiki_pages (
                dataset_version, page_id, page_kind, title_ko, summary_ko,
                body_markdown, source_manifest_sha256, review_status
             ) VALUES (
                'fixture', 'wiki:review', 'guide', '초안', '초안 요약',
                '# 초안', ?1, 'draft'
             )",
            ["d".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_wiki_revisions (
                dataset_version, page_id, revision_number, title_ko, summary_ko,
                body_markdown, claims_json, change_origin, review_status,
                change_summary_ko
             ) VALUES (
                'fixture', 'wiki:review', 1, '승인 제목', '승인 요약',
                '# 승인 본문',
                '[{
                  \"claim_id\":\"claim:review\",
                  \"text_ko\":\"검증된 주장\",
                  \"quality\":\"exact\",
                  \"evidence_refs\":[\"fixture\"],
                  \"related_node_ids\":[\"concept:review\"]
                }]',
                'human_edit', 'draft', '검수 요청'
             )",
            [],
        )
        .unwrap();
    let approved = connection
        .execute(
            "UPDATE knowledge_wiki_revisions
             SET review_status='reviewed', reviewer_id='operator',
                 reviewed_at=CURRENT_TIMESTAMP, review_version=review_version+1
             WHERE dataset_version='fixture' AND page_id='wiki:review'
               AND revision_number=1 AND review_status='draft' AND review_version=0",
            [],
        )
        .unwrap();
    assert_eq!(approved, 1);
    connection
        .execute(
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
             WHERE revision.dataset_version='fixture'
               AND revision.page_id='wiki:review'
               AND revision.revision_number=1
               AND revision.review_status='reviewed'
               AND revision.reviewer_id='operator' AND revision.review_version=1",
            [],
        )
        .unwrap();
    let promoted = connection
        .execute(
            "UPDATE knowledge_wiki_pages AS page
             SET title_ko=revision.title_ko,
                 summary_ko=revision.summary_ko,
                 body_markdown=revision.body_markdown,
                 related_node_ids_json=revision.related_node_ids_json,
                 review_status='reviewed',
                 updated_at=CURRENT_TIMESTAMP
             FROM knowledge_wiki_revisions AS revision
             WHERE page.dataset_version='fixture' AND page.page_id='wiki:review'
               AND revision.dataset_version=page.dataset_version
               AND revision.page_id=page.page_id AND revision.revision_number=1
               AND revision.review_status='reviewed'
               AND revision.reviewer_id='operator' AND revision.review_version=1",
            [],
        )
        .unwrap();
    assert_eq!(promoted, 1);
    let page_title: String = connection
        .query_row(
            "SELECT title_ko FROM knowledge_wiki_pages
             WHERE dataset_version='fixture' AND page_id='wiki:review'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(page_title, "승인 제목");
    let claim: (String, String) = connection
        .query_row(
            "SELECT claim_text_ko, evidence_quality
             FROM knowledge_wiki_claims
             WHERE dataset_version='fixture' AND page_id='wiki:review'
               AND claim_id='claim:review'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(claim, ("검증된 주장".to_owned(), "exact".to_owned()));
    let stale_rejection = connection
        .execute(
            "UPDATE knowledge_wiki_revisions
             SET review_status='rejected', review_version=review_version+1
             WHERE dataset_version='fixture' AND page_id='wiki:review'
               AND revision_number=1 AND review_status='draft' AND review_version=0",
            [],
        )
        .unwrap();
    assert_eq!(stale_rejection, 0);
}

#[test]
fn manifest_validation_requires_exact_reviewed_complete_inputs() {
    let connection = migrated();
    connection
        .execute(
            "INSERT INTO knowledge_source_artifacts (
                dataset_version, artifact_id, artifact_kind, logical_source_ref,
                sha256, byte_size, evidence_quality
             ) VALUES (
                'fixture', 'artifact:review', 'catalog', 'fixture:catalog',
                ?1, 10, 'exact'
             )",
            ["e".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_graph_nodes (
                dataset_version, node_id, node_kind, label_ko,
                evidence_quality, evidence_refs_json, content_hash
             ) VALUES (
                'fixture', 'concept:review', 'concept', '검수',
                'exact', '[\"fixture\"]', ?1
             )",
            ["f".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_wiki_pages (
                dataset_version, page_id, page_kind, title_ko, summary_ko,
                body_markdown, source_manifest_sha256, review_status
             ) VALUES (
                'fixture', 'wiki:manifest-review', 'guide', '검수', '검수 요약',
                '# 검수', ?1, 'reviewed'
             )",
            ["1".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_pipeline_runs (
                run_id, dataset_version, pipeline_name, compiler_version,
                status, completed_at
             ) VALUES (
                'run:review', 'fixture', 'knowledge-compile', 'compiler-v1',
                'completed', CURRENT_TIMESTAMP
             )",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO knowledge_dataset_manifests (
                dataset_version, manifest_sha256, compiler_version,
                graph_schema_version, wiki_schema_version, publication_status,
                source_artifact_count, graph_node_count, graph_edge_count,
                wiki_page_count
             ) VALUES (
                'fixture', ?1, 'compiler-v1', 'graph-v1', 'wiki-v1',
                'candidate', 1, 1, 0, 1
             )",
            ["2".repeat(64)],
        )
        .unwrap();
    let changed = connection
        .execute(
            "UPDATE knowledge_dataset_manifests AS manifest
             SET publication_status='validated', validated_at=CURRENT_TIMESTAMP,
                 review_version=review_version+1
             WHERE dataset_version='fixture' AND review_version=0
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
               )",
            [],
        )
        .unwrap();
    assert_eq!(changed, 1);
    let state: (String, i64) = connection
        .query_row(
            "SELECT publication_status, review_version
             FROM knowledge_dataset_manifests WHERE dataset_version='fixture'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, ("validated".to_owned(), 1));
}

#[test]
fn error_book_keyset_pagination_reaches_every_filtered_row_once() {
    use std::collections::BTreeSet;

    let connection = migrated();
    for index in 0..137 {
        connection
            .execute(
                "INSERT INTO knowledge_error_book (
                    entry_id, dataset_version, subject_kind, subject_id,
                    error_kind, diagnosis_ko, status, detected_by, created_at
                 ) VALUES (
                    ?1, 'fixture', 'graph_edge', ?2, 'missing_endpoint',
                    '페이지 검수', ?3, 'validator', ?4
                 )",
                params![
                    format!("error:page:{index:03}"),
                    format!("edge:{index:03}"),
                    if index % 2 == 0 { "open" } else { "resolved" },
                    format!("2026-07-30 12:{:02}:00", index % 7),
                ],
            )
            .unwrap();
    }

    let mut cursor: Option<(String, String, String)> = None;
    let mut visited = BTreeSet::new();
    loop {
        let cursor_time = cursor.as_ref().map(|value| value.0.as_str());
        let cursor_dataset = cursor.as_ref().map(|value| value.1.as_str());
        let cursor_key = cursor.as_ref().map(|value| value.2.as_str());
        let mut statement = connection
            .prepare(
                "SELECT created_at, dataset_version, entry_id
                 FROM knowledge_error_book
                 WHERE dataset_version=?1 AND status=?2
                   AND (
                     ?3 IS NULL
                     OR created_at < ?3
                     OR (created_at = ?3 AND dataset_version > ?4)
                     OR (
                       created_at = ?3 AND dataset_version = ?4
                       AND entry_id > ?5
                     )
                   )
                 ORDER BY created_at DESC, dataset_version, entry_id
                 LIMIT 17",
            )
            .unwrap();
        let rows = statement
            .query_map(
                params!["fixture", "open", cursor_time, cursor_dataset, cursor_key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        if rows.is_empty() {
            break;
        }
        for (_, _, entry_id) in &rows {
            assert!(visited.insert(entry_id.clone()), "duplicate {entry_id}");
        }
        cursor = rows.last().cloned();
    }

    assert_eq!(visited.len(), 69);
    assert!(visited.contains("error:page:000"));
    assert!(visited.contains("error:page:136"));
}
