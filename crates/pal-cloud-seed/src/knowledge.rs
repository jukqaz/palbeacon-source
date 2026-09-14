use super::{Options, sql_document, sql_text};

pub(super) const COMPILER_VERSION: &str = "pal-knowledge-compiler-v1";

#[derive(Clone, Debug)]
pub(super) struct SourceArtifact {
    pub artifact_id: &'static str,
    pub artifact_kind: &'static str,
    pub logical_source_ref: String,
    pub sha256: String,
    pub byte_size: usize,
    pub evidence_quality: &'static str,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct KnowledgeProjectionCounts {
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub wiki_pages: usize,
    pub error_count: usize,
    pub missing_active_skill_edges: usize,
    pub missing_passive_edges: usize,
    pub missing_drop_species_edges: usize,
    pub missing_unlock_item_edges: usize,
}

pub(super) fn projection_sql(
    options: &Options,
    manifest_sha256: &str,
    artifacts: &[SourceArtifact],
) -> String {
    let dataset = sql_text(&options.dataset_version);
    let manifest = sql_text(manifest_sha256);
    let build = sql_text(&options.game_build_id);
    let compiler = sql_text(COMPILER_VERSION);
    let run_id = sql_text(&format!("knowledge-compile:{}", options.dataset_version));
    let mut rows = vec![format!(
        "INSERT INTO knowledge_dataset_manifests (
             dataset_version, manifest_sha256, compiler_version,
             graph_schema_version, wiki_schema_version, publication_status,
             source_artifact_count
         ) VALUES (
             {dataset}, {manifest}, {compiler}, 'property-graph-v1',
             'compiled-wiki-v1', 'candidate', {}
         );",
        artifacts.len()
    )];

    for artifact in artifacts {
        rows.push(format!(
            "INSERT INTO knowledge_source_artifacts (
                 dataset_version, artifact_id, artifact_kind, logical_source_ref,
                 sha256, byte_size, evidence_quality
             ) VALUES ({dataset}, {}, {}, {}, {}, {}, {});",
            sql_text(artifact.artifact_id),
            sql_text(artifact.artifact_kind),
            sql_text(&artifact.logical_source_ref),
            sql_text(&artifact.sha256),
            artifact.byte_size,
            sql_text(artifact.evidence_quality),
        ));
    }

    rows.push(format!(
        "INSERT INTO knowledge_pipeline_runs (
             run_id, dataset_version, pipeline_name, compiler_version, status,
             input_facets_json, output_facets_json, completed_at
         ) VALUES (
             {run_id}, {dataset}, 'exact-build-knowledge-compilation', {compiler},
             'completed',
             json_object('artifact_count', {}),
             json_object('graph_schema', 'property-graph-v1',
                         'wiki_schema', 'compiled-wiki-v1'),
             CURRENT_TIMESTAMP
         );",
        artifacts.len()
    ));

    rows.extend(graph_node_statements(&dataset, &manifest, &build));
    rows.extend(graph_gap_statements(&dataset));
    rows.extend(graph_edge_statements(&dataset, &manifest, &build));
    rows.extend(wiki_statements(&dataset, &manifest, &build, &compiler));

    for artifact in artifacts {
        for (output_kind, transform_kind) in [
            ("graph", "exact_projection"),
            ("wiki", "wiki_compilation"),
            ("search_index", "fts_indexing"),
        ] {
            rows.push(format!(
                "INSERT INTO knowledge_lineage_edges (
                     dataset_version, lineage_id, run_id, input_artifact_id,
                     output_kind, output_id, transform_kind
                 ) VALUES (
                     {dataset}, {}, {run_id}, {}, {}, {}, {}
                 );",
                sql_text(&format!("lineage:{}:{output_kind}", artifact.artifact_id)),
                sql_text(artifact.artifact_id),
                sql_text(output_kind),
                sql_text(&format!("{output_kind}:{}", options.dataset_version)),
                sql_text(transform_kind),
            ));
        }
    }

    rows.push(format!(
        "UPDATE knowledge_dataset_manifests
         SET publication_status=CASE
               WHEN (
                 SELECT COUNT(*) FROM knowledge_error_book
                 WHERE dataset_version={dataset} AND status IN ('open', 'accepted')
               )=0 THEN 'validated'
               ELSE 'candidate'
             END,
             graph_node_count=(
                 SELECT COUNT(*) FROM knowledge_graph_nodes
                 WHERE dataset_version={dataset}
             ),
             graph_edge_count=(
                 SELECT COUNT(*) FROM knowledge_graph_edges
                 WHERE dataset_version={dataset}
             ),
             wiki_page_count=(
                 SELECT COUNT(*) FROM knowledge_wiki_pages
                 WHERE dataset_version={dataset}
             ),
             validated_at=CASE
               WHEN (
                 SELECT COUNT(*) FROM knowledge_error_book
                 WHERE dataset_version={dataset} AND status IN ('open', 'accepted')
               )=0 THEN CURRENT_TIMESTAMP
               ELSE NULL
             END
         WHERE dataset_version={dataset};"
    ));

    sql_document(&rows)
}

fn graph_gap_statements(dataset: &str) -> Vec<String> {
    vec![
        format!(
            "INSERT INTO knowledge_error_book
             (entry_id, dataset_version, subject_kind, subject_id, error_kind,
              diagnosis_ko, source_refs_json, detected_by)
             SELECT 'error:missing-active-skill:' || species.species_id || ':' ||
                        json_extract(skill.value, '$.skill_id'),
                    species.dataset_version, 'graph_edge',
                    'learns-skill:' || species.species_id || ':' ||
                        json_extract(skill.value, '$.skill_id'),
                    'missing_graph_endpoint',
                    species.name_ko || '의 습득 스킬이 액티브 스킬 카탈로그에 없습니다.',
                    json_array(COALESCE(species.source_ref,
                                        'dataset:' || species.dataset_version)),
                    'validator'
             FROM catalog_species species
             CROSS JOIN json_each(species.learned_skills_json) skill
             WHERE species.dataset_version={dataset}
               AND NOT EXISTS (
                 SELECT 1 FROM knowledge_graph_nodes target
                 WHERE target.dataset_version=species.dataset_version
                   AND target.node_id='active-skill:' ||
                       json_extract(skill.value, '$.skill_id')
               );"
        ),
        format!(
            "INSERT INTO knowledge_error_book
             (entry_id, dataset_version, subject_kind, subject_id, error_kind,
              diagnosis_ko, source_refs_json, detected_by)
             SELECT 'error:missing-passive:' || species.species_id || ':' ||
                        passive.value,
                    species.dataset_version, 'graph_edge',
                    'guaranteed-passive:' || species.species_id || ':' ||
                        passive.value,
                    'missing_graph_endpoint',
                    species.name_ko || '의 확정 패시브가 패시브 카탈로그에 없습니다.',
                    json_array(COALESCE(species.source_ref,
                                        'dataset:' || species.dataset_version)),
                    'validator'
             FROM catalog_species species
             CROSS JOIN json_each(species.guaranteed_passives_json) passive
             WHERE species.dataset_version={dataset}
               AND NOT EXISTS (
                 SELECT 1 FROM knowledge_graph_nodes target
                 WHERE target.dataset_version=species.dataset_version
                   AND target.node_id='passive-skill:' || passive.value
               );"
        ),
        format!(
            "INSERT INTO knowledge_error_book
             (entry_id, dataset_version, subject_kind, subject_id, error_kind,
              diagnosis_ko, source_refs_json, detected_by)
             SELECT 'error:missing-drop-species:' || method.method_id,
                    method.dataset_version, 'graph_edge',
                    'drop:' || method.method_id, 'missing_graph_endpoint',
                    '드롭 원본의 팰이 정규화 팰 카탈로그에 없습니다.',
                    json_array(COALESCE(method.evidence_ref,
                                        'dataset:' || method.dataset_version)),
                    'validator'
             FROM acquisition_methods method
             WHERE method.dataset_version={dataset}
               AND method.method_kind='pal_drop'
               AND NOT EXISTS (
                 SELECT 1 FROM knowledge_graph_nodes source
                 WHERE source.dataset_version=method.dataset_version
                   AND source.node_id='species:' || method.source_entity_id
               );"
        ),
        format!(
            "INSERT INTO knowledge_error_book
             (entry_id, dataset_version, subject_kind, subject_id, error_kind,
              diagnosis_ko, source_refs_json, detected_by)
             SELECT 'error:missing-unlock-item:' || recipe.recipe_id,
                    recipe.dataset_version, 'graph_edge',
                    'recipe-unlock:' || recipe.recipe_id,
                    'missing_graph_endpoint',
                    '제작법의 해금 아이템이 아이템 카탈로그에 없습니다.',
                    json_array(COALESCE(recipe.source_ref,
                                        'dataset:' || recipe.dataset_version)),
                    'validator'
             FROM catalog_recipes recipe
             WHERE recipe.dataset_version={dataset}
               AND recipe.unlock_item_id IS NOT NULL
               AND NOT EXISTS (
                 SELECT 1 FROM knowledge_graph_nodes target
                 WHERE target.dataset_version=recipe.dataset_version
                   AND target.node_id='item:' || recipe.unlock_item_id
               );"
        ),
    ]
}

fn graph_node_statements(dataset: &str, manifest: &str, build: &str) -> Vec<String> {
    vec![
        format!(
            "INSERT INTO knowledge_graph_nodes
             (dataset_version, node_id, node_kind, label_ko, evidence_quality,
              evidence_refs_json, content_hash)
             SELECT cs.dataset_version, 'species:' || cs.species_id, 'species',
                    cs.name_ko, 'exact',
                    json_array('fact:species:' || cs.species_id,
                               COALESCE(cs.source_ref, 'dataset:' || cs.dataset_version)),
                    {manifest}
             FROM catalog_species cs WHERE cs.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_nodes
             (dataset_version, node_id, node_kind, label_ko, evidence_quality,
              evidence_refs_json, content_hash)
             SELECT skill.dataset_version, 'active-skill:' || skill.skill_id,
                    'active_skill', skill.name_ko, 'exact',
                    json_array('fact:active-skill:' || skill.skill_id,
                               COALESCE(skill.source_ref,
                                        'dataset:' || skill.dataset_version)),
                    {manifest}
             FROM catalog_active_skills skill
             WHERE skill.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_nodes
             (dataset_version, node_id, node_kind, label_ko, evidence_quality,
              evidence_refs_json, content_hash)
             SELECT passive.dataset_version, 'passive-skill:' || passive.passive_id,
                    'passive_skill', passive.name_ko, 'exact',
                    json_array('fact:passive-skill:' || passive.passive_id,
                               'artifact:passive-skills'),
                    {manifest}
             FROM catalog_passives passive
             WHERE passive.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_nodes
             (dataset_version, node_id, node_kind, label_ko, evidence_quality,
              evidence_refs_json, content_hash)
             SELECT item.dataset_version, 'item:' || item.item_id, 'item',
                    item.name_ko, 'exact',
                    json_array('fact:item:' || item.item_id,
                               COALESCE(item.source_ref,
                                        'dataset:' || item.dataset_version)),
                    {manifest}
             FROM catalog_items item WHERE item.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_nodes
             (dataset_version, node_id, node_kind, label_ko, evidence_quality,
              evidence_refs_json, content_hash)
             SELECT recipe.dataset_version, 'recipe:' || recipe.recipe_id, 'recipe',
                    output.name_ko || ' 제작법', 'exact',
                    json_array('fact:recipe:' || recipe.recipe_id,
                               COALESCE(recipe.source_ref,
                                        'dataset:' || recipe.dataset_version)),
                    {manifest}
             FROM catalog_recipes recipe
             JOIN catalog_items output
               ON output.dataset_version=recipe.dataset_version
              AND output.item_id=recipe.output_item_id
             WHERE recipe.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_nodes
             (dataset_version, node_id, node_kind, label_ko, evidence_quality,
              evidence_refs_json, content_hash)
             SELECT rule.dataset_version,
                    'breeding-rule:' || rule.parent_a_species_id || ':' ||
                        rule.parent_b_species_id || ':' || rule.child_species_id,
                    'breeding_rule',
                    parent_a.name_ko || ' × ' || parent_b.name_ko || ' → ' ||
                        child.name_ko,
                    'exact',
                    json_array(
                        'fact:breeding-rule:' || rule.parent_a_species_id || ':' ||
                            rule.parent_b_species_id || ':' || rule.child_species_id,
                        'game-files:steam:' || {build} || ':breeding'
                    ),
                    {manifest}
             FROM breeding_rules rule
             JOIN catalog_species parent_a
               ON parent_a.dataset_version=rule.dataset_version
              AND parent_a.species_id=rule.parent_a_species_id
             JOIN catalog_species parent_b
               ON parent_b.dataset_version=rule.dataset_version
              AND parent_b.species_id=rule.parent_b_species_id
             JOIN catalog_species child
               ON child.dataset_version=rule.dataset_version
              AND child.species_id=rule.child_species_id
             WHERE rule.dataset_version={dataset};"
        ),
    ]
}

fn graph_edge_statements(dataset: &str, manifest: &str, build: &str) -> Vec<String> {
    vec![
        format!(
            "INSERT INTO knowledge_graph_edges
             (dataset_version, edge_id, from_node_id, to_node_id, relation_kind,
              origin, evidence_quality, evidence_refs_json, attributes_json,
              content_hash)
             SELECT species.dataset_version,
                    'learns-skill:' || species.species_id || ':' ||
                        json_extract(skill.value, '$.skill_id'),
                    'species:' || species.species_id,
                    'active-skill:' || json_extract(skill.value, '$.skill_id'),
                    'learns_skill', 'exact_extract', 'exact',
                    json_array(
                        'relation:learns-skill:' || species.species_id || ':' ||
                            json_extract(skill.value, '$.skill_id'),
                        COALESCE(species.source_ref,
                                 'dataset:' || species.dataset_version)
                    ),
                    json_object(
                        'learn_level', json_extract(skill.value, '$.learn_level'),
                        'element_ko', json_extract(skill.value, '$.element_ko'),
                        'power', json_extract(skill.value, '$.power'),
                        'cool_time', json_extract(skill.value, '$.cool_time')
                    ),
                    {manifest}
             FROM catalog_species species
             CROSS JOIN json_each(species.learned_skills_json) skill
             JOIN knowledge_graph_nodes target
               ON target.dataset_version=species.dataset_version
              AND target.node_id='active-skill:' ||
                  json_extract(skill.value, '$.skill_id')
             WHERE species.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_edges
             (dataset_version, edge_id, from_node_id, to_node_id, relation_kind,
              origin, evidence_quality, evidence_refs_json, attributes_json,
              content_hash)
             SELECT species.dataset_version,
                    'guaranteed-passive:' || species.species_id || ':' ||
                        passive.value,
                    'species:' || species.species_id,
                    'passive-skill:' || passive.value,
                    'has_guaranteed_passive', 'exact_extract', 'exact',
                    json_array(
                        'relation:guaranteed-passive:' || species.species_id || ':' ||
                            passive.value,
                        COALESCE(species.source_ref,
                                 'dataset:' || species.dataset_version)
                    ),
                    '{{}}', {manifest}
             FROM catalog_species species
             CROSS JOIN json_each(species.guaranteed_passives_json) passive
             JOIN knowledge_graph_nodes target
               ON target.dataset_version=species.dataset_version
              AND target.node_id='passive-skill:' || passive.value
             WHERE species.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_edges
             (dataset_version, edge_id, from_node_id, to_node_id, relation_kind,
              origin, evidence_quality, evidence_refs_json, attributes_json,
              content_hash)
             SELECT method.dataset_version, 'drop:' || method.method_id,
                    'species:' || method.source_entity_id,
                    'item:' || method.item_id,
                    'drops_item', 'exact_extract', 'exact',
                    json_array('relation:drop:' || method.method_id,
                               COALESCE(method.evidence_ref,
                                        'dataset:' || method.dataset_version)),
                    json_object(
                        'quantity_min', method.quantity_min,
                        'quantity_max', method.quantity_max,
                        'probability', method.probability,
                        'requirements', json(method.requirements_json)
                    ),
                    {manifest}
             FROM acquisition_methods method
             JOIN knowledge_graph_nodes source
               ON source.dataset_version=method.dataset_version
              AND source.node_id='species:' || method.source_entity_id
             WHERE method.dataset_version={dataset}
               AND method.method_kind='pal_drop';"
        ),
        format!(
            "INSERT INTO knowledge_graph_edges
             (dataset_version, edge_id, from_node_id, to_node_id, relation_kind,
              origin, evidence_quality, evidence_refs_json, attributes_json,
              content_hash)
             SELECT recipe.dataset_version, 'recipe-output:' || recipe.recipe_id,
                    'recipe:' || recipe.recipe_id,
                    'item:' || recipe.output_item_id,
                    'produces_item', 'exact_extract', 'exact',
                    json_array('relation:recipe-output:' || recipe.recipe_id,
                               COALESCE(recipe.source_ref,
                                        'dataset:' || recipe.dataset_version)),
                    json_object('quantity', recipe.output_quantity,
                                'work_amount', recipe.work_amount),
                    {manifest}
             FROM catalog_recipes recipe
             WHERE recipe.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_edges
             (dataset_version, edge_id, from_node_id, to_node_id, relation_kind,
              origin, evidence_quality, evidence_refs_json, attributes_json,
              content_hash)
             SELECT recipe.dataset_version,
                    'recipe-ingredient:' || recipe.recipe_id || ':' ||
                        json_extract(ingredient.value, '$.item_id'),
                    'item:' || json_extract(ingredient.value, '$.item_id'),
                    'recipe:' || recipe.recipe_id,
                    'ingredient_for', 'exact_extract', 'exact',
                    json_array(
                        'relation:recipe-ingredient:' || recipe.recipe_id || ':' ||
                            json_extract(ingredient.value, '$.item_id'),
                        COALESCE(recipe.source_ref,
                                 'dataset:' || recipe.dataset_version)
                    ),
                    json_object(
                        'quantity', json_extract(ingredient.value, '$.quantity')
                    ),
                    {manifest}
             FROM catalog_recipes recipe,
                  json_each(recipe.ingredients_json) ingredient
             WHERE recipe.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_edges
             (dataset_version, edge_id, from_node_id, to_node_id, relation_kind,
              origin, evidence_quality, evidence_refs_json, attributes_json,
              content_hash)
             SELECT recipe.dataset_version, 'recipe-unlock:' || recipe.recipe_id,
                    'recipe:' || recipe.recipe_id,
                    'item:' || recipe.unlock_item_id,
                    'requires', 'exact_extract', 'exact',
                    json_array('relation:recipe-unlock:' || recipe.recipe_id,
                               COALESCE(recipe.source_ref,
                                        'dataset:' || recipe.dataset_version)),
                    json_object('requirement_kind', 'unlock_item'),
                    {manifest}
             FROM catalog_recipes recipe
             JOIN knowledge_graph_nodes target
               ON target.dataset_version=recipe.dataset_version
              AND target.node_id='item:' || recipe.unlock_item_id
             WHERE recipe.dataset_version={dataset}
               AND recipe.unlock_item_id IS NOT NULL;"
        ),
        format!(
            "INSERT INTO knowledge_graph_edges
             (dataset_version, edge_id, from_node_id, to_node_id, relation_kind,
              origin, evidence_quality, evidence_refs_json, attributes_json,
              content_hash)
             SELECT rule.dataset_version,
                    'breeding-parent-a:' || rule.parent_a_species_id || ':' ||
                        rule.parent_b_species_id || ':' || rule.child_species_id,
                    'species:' || rule.parent_a_species_id,
                    'breeding-rule:' || rule.parent_a_species_id || ':' ||
                        rule.parent_b_species_id || ':' || rule.child_species_id,
                    'breeding_parent', 'exact_extract', 'exact',
                    json_array(
                        'relation:breeding-parent-a:' ||
                            rule.parent_a_species_id || ':' ||
                            rule.parent_b_species_id || ':' || rule.child_species_id,
                        'game-files:steam:' || {build} || ':breeding'
                    ),
                    json_object('role', 'parent_a'), {manifest}
             FROM breeding_rules rule WHERE rule.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_edges
             (dataset_version, edge_id, from_node_id, to_node_id, relation_kind,
              origin, evidence_quality, evidence_refs_json, attributes_json,
              content_hash)
             SELECT rule.dataset_version,
                    'breeding-parent-b:' || rule.parent_a_species_id || ':' ||
                        rule.parent_b_species_id || ':' || rule.child_species_id,
                    'species:' || rule.parent_b_species_id,
                    'breeding-rule:' || rule.parent_a_species_id || ':' ||
                        rule.parent_b_species_id || ':' || rule.child_species_id,
                    'breeding_parent', 'exact_extract', 'exact',
                    json_array(
                        'relation:breeding-parent-b:' ||
                            rule.parent_a_species_id || ':' ||
                            rule.parent_b_species_id || ':' || rule.child_species_id,
                        'game-files:steam:' || {build} || ':breeding'
                    ),
                    json_object('role', 'parent_b'), {manifest}
             FROM breeding_rules rule WHERE rule.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_edges
             (dataset_version, edge_id, from_node_id, to_node_id, relation_kind,
              origin, evidence_quality, evidence_refs_json, attributes_json,
              content_hash)
             SELECT rule.dataset_version,
                    'breeding-result:' || rule.parent_a_species_id || ':' ||
                        rule.parent_b_species_id || ':' || rule.child_species_id,
                    'breeding-rule:' || rule.parent_a_species_id || ':' ||
                        rule.parent_b_species_id || ':' || rule.child_species_id,
                    'species:' || rule.child_species_id,
                    'breeds_into', 'exact_extract', 'exact',
                    json_array(
                        'relation:breeding-result:' ||
                            rule.parent_a_species_id || ':' ||
                            rule.parent_b_species_id || ':' || rule.child_species_id,
                        'game-files:steam:' || {build} || ':breeding'
                    ),
                    '{{}}', {manifest}
             FROM breeding_rules rule WHERE rule.dataset_version={dataset};"
        ),
    ]
}

fn wiki_statements(dataset: &str, manifest: &str, build: &str, compiler: &str) -> Vec<String> {
    vec![
        format!(
            "INSERT INTO knowledge_wiki_pages
             (dataset_version, page_id, page_kind, title_ko, summary_ko,
              body_markdown, related_node_ids_json, source_manifest_sha256,
              review_status)
             SELECT node.dataset_version, 'wiki:' || node.node_id, 'entity',
                    node.label_ko,
                    node.label_ko || '의 검증된 게임 데이터입니다.',
                    '# ' || node.label_ko || char(10) || char(10) ||
                    '게임 빌드 ' || {build} ||
                    '에서 추출하고 관계 무결성을 검사한 정보입니다.',
                    json_array(node.node_id), {manifest}, 'reviewed'
             FROM knowledge_graph_nodes node
             WHERE node.dataset_version={dataset}
               AND node.node_kind NOT IN ('wiki_page', 'breeding_rule');"
        ),
        format!(
            "INSERT INTO knowledge_wiki_claims
             (dataset_version, page_id, claim_id, claim_text_ko,
              evidence_quality, evidence_refs_json, related_node_ids_json)
             SELECT node.dataset_version, 'wiki:' || node.node_id,
                    'claim:' || node.node_id,
                    node.label_ko || ' 항목은 현재 게임 빌드의 검증된 데이터에 포함됩니다.',
                    node.evidence_quality,
                    json_array('fact:' || node.node_id),
                    json_array(node.node_id)
             FROM knowledge_graph_nodes node
             WHERE node.dataset_version={dataset}
               AND node.node_kind NOT IN ('wiki_page', 'breeding_rule');"
        ),
        format!(
            "INSERT INTO knowledge_graph_nodes
             (dataset_version, node_id, node_kind, label_ko, evidence_quality,
              evidence_refs_json, content_hash)
             SELECT page.dataset_version, 'wiki-page:' || substr(page.page_id, 6),
                    'wiki_page', page.title_ko, 'exact',
                    json_array('wiki-claim:' || substr(page.page_id, 6),
                               'manifest:' || page.source_manifest_sha256),
                    {manifest}
             FROM knowledge_wiki_pages page
             WHERE page.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_graph_edges
             (dataset_version, edge_id, from_node_id, to_node_id, relation_kind,
              origin, evidence_quality, evidence_refs_json, attributes_json,
              content_hash)
             SELECT page.dataset_version,
                    'wiki-reference:' || substr(page.page_id, 6),
                    'wiki-page:' || substr(page.page_id, 6),
                    substr(page.page_id, 6),
                    'references', 'exact_extract', 'exact',
                    json_array('wiki-claim:' || substr(page.page_id, 6)),
                    '{{}}', {manifest}
             FROM knowledge_wiki_pages page
             WHERE page.dataset_version={dataset};"
        ),
        format!(
            "INSERT INTO knowledge_wiki_links
             (dataset_version, from_page_id, to_page_id, relation_kind,
              source_edge_id)
             SELECT edge.dataset_version,
                    'wiki:' || edge.from_node_id,
                    'wiki:' || edge.to_node_id,
                    edge.relation_kind, edge.edge_id
             FROM knowledge_graph_edges edge
             JOIN knowledge_wiki_pages source_page
               ON source_page.dataset_version=edge.dataset_version
              AND source_page.page_id='wiki:' || edge.from_node_id
             JOIN knowledge_wiki_pages target_page
               ON target_page.dataset_version=edge.dataset_version
              AND target_page.page_id='wiki:' || edge.to_node_id
             WHERE edge.dataset_version={dataset}
               AND edge.relation_kind <> 'references';"
        ),
        format!(
            "INSERT INTO knowledge_wiki_revisions
             (dataset_version, page_id, revision_number, title_ko, summary_ko,
              body_markdown, claims_json, related_node_ids_json, change_origin,
              review_status, generator_id, reviewer_id, change_summary_ko)
             SELECT page.dataset_version, page.page_id, 1, page.title_ko,
                    page.summary_ko, page.body_markdown,
                    COALESCE((
                        SELECT json_group_array(json_object(
                            'claim_id', claim.claim_id,
                            'text_ko', claim.claim_text_ko,
                            'quality', claim.evidence_quality,
                            'evidence_refs', json(claim.evidence_refs_json),
                            'related_node_ids', json(claim.related_node_ids_json)
                        ))
                        FROM knowledge_wiki_claims claim
                        WHERE claim.dataset_version=page.dataset_version
                          AND claim.page_id=page.page_id
                    ), '[]'),
                    page.related_node_ids_json, 'exact_compiler', 'reviewed',
                    {compiler}, {compiler},
                    'exact-build 데이터에서 최초 컴파일'
             FROM knowledge_wiki_pages page
             WHERE page.dataset_version={dataset};"
        ),
    ]
}
