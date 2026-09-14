use rusqlite::Connection;

const MIGRATION: &str = include_str!("../../../cloudflare/migrations/0001_private_companion.sql");
const CATALOG_SEARCH_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0002_catalog_search.sql");
const CATALOG_DETAIL_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0003_catalog_species_detail.sql");
const CATALOG_SKILLS_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0004_catalog_skills.sql");
const CATALOG_ITEMS_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0005_catalog_items_detail.sql");
const GAME_BUILD_COMPATIBILITY_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0006_game_build_compatibility.sql");
const KNOWLEDGE_GRAPH_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0007_knowledge_graph_wiki.sql");
const KNOWLEDGE_OPERATIONS_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0008_knowledge_operations.sql");
const KNOWLEDGE_OPERATOR_REVIEW_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0009_knowledge_operator_review.sql");
const KNOWLEDGE_REVIEW_PAGINATION_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0010_knowledge_review_pagination.sql");
const DATASET_SCOPE_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0011_dataset_scope.sql");
const POST_MIGRATION_TRIGGERS: &str =
    include_str!("../../../cloudflare/schema/post_migration_triggers.sql");

fn migrated_connection() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(MIGRATION).unwrap();
    connection.execute_batch(CATALOG_SEARCH_MIGRATION).unwrap();
    connection.execute_batch(CATALOG_DETAIL_MIGRATION).unwrap();
    connection.execute_batch(CATALOG_SKILLS_MIGRATION).unwrap();
    connection.execute_batch(CATALOG_ITEMS_MIGRATION).unwrap();
    connection
        .execute_batch(GAME_BUILD_COMPATIBILITY_MIGRATION)
        .unwrap();
    connection.execute_batch(KNOWLEDGE_GRAPH_MIGRATION).unwrap();
    connection
        .execute_batch(KNOWLEDGE_OPERATIONS_MIGRATION)
        .unwrap();
    connection
        .execute_batch(KNOWLEDGE_OPERATOR_REVIEW_MIGRATION)
        .unwrap();
    connection
        .execute_batch(KNOWLEDGE_REVIEW_PAGINATION_MIGRATION)
        .unwrap();
    connection.execute_batch(DATASET_SCOPE_MIGRATION).unwrap();
    connection.execute_batch(POST_MIGRATION_TRIGGERS).unwrap();
    connection
}

#[test]
fn phase_one_schema_applies_to_empty_sqlite() {
    let connection = migrated_connection();

    let expected = [
        "principals",
        "owner_bindings",
        "service_agents",
        "sync_runs",
        "projection_commits",
        "character_profiles",
        "inventory_slots",
        "equipment_slots",
        "owned_pals",
        "owned_pal_passives",
        "owned_pal_skills",
        "data_versions",
        "catalog_species",
        "catalog_aliases",
        "catalog_passives",
        "catalog_active_skills",
        "breeding_rules",
        "catalog_items",
        "catalog_recipes",
        "acquisition_methods",
        "knowledge_documents",
        "knowledge_fts",
        "assistant_runs",
        "audit_events",
        "game_build_policies",
        "knowledge_graph_nodes",
        "knowledge_graph_edges",
        "knowledge_edge_suggestions",
        "knowledge_wiki_pages",
        "knowledge_wiki_claims",
        "knowledge_wiki_fts",
        "knowledge_dataset_manifests",
        "knowledge_source_artifacts",
        "knowledge_pipeline_runs",
        "knowledge_lineage_edges",
        "knowledge_wiki_revisions",
        "knowledge_wiki_links",
        "knowledge_error_book",
        "knowledge_retrieval_runs",
    ];
    for table in expected {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "missing table {table}");
    }
}

#[test]
fn operator_review_keyset_indexes_are_present() {
    let connection = migrated_connection();
    for index in [
        "idx_knowledge_error_review_page",
        "idx_knowledge_error_review_filter_page",
        "idx_knowledge_manifest_review_page",
        "idx_knowledge_wiki_revision_review_page",
        "idx_knowledge_wiki_revision_filter_page",
    ] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='index' AND name=?1",
                [index],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "missing pagination index {index}");
    }
}

#[test]
fn activating_verified_dataset_approves_build_without_application_rebuild() {
    let connection = migrated_connection();
    connection
        .execute(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, verified) \
             VALUES ('data-v1', '1.0.1', 'steam:24181527', '6', 'fixture', 1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE data_versions SET activated_at=CURRENT_TIMESTAMP \
             WHERE dataset_version='data-v1'",
            [],
        )
        .unwrap();

    let policy = connection
        .query_row(
            "SELECT game_build_id, compatibility_state, dataset_version \
             FROM game_build_policies WHERE game_build_id='24181527'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        policy,
        (
            "24181527".to_owned(),
            "compatible".to_owned(),
            "data-v1".to_owned()
        )
    );
}

#[test]
fn activating_public_metadata_does_not_approve_personal_projection_build() {
    let connection = migrated_connection();
    connection
        .execute(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, \
              dataset_scope, verified, activated_at) \
             VALUES ('public-meta-v1', '1.0.1', 'steam:24181527', '11', 'fixture', \
                     'public_metadata', 1, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();

    let policy_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM game_build_policies WHERE game_build_id='24181527'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(policy_count, 0);

    let scope: String = connection
        .query_row(
            "SELECT dataset_scope FROM data_versions \
             WHERE dataset_version='public-meta-v1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(scope, "public_metadata");
}

#[test]
fn full_catalog_and_public_metadata_have_independent_active_pointers() {
    let connection = migrated_connection();
    connection
        .execute_batch(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, \
              dataset_scope, verified, activated_at) \
             VALUES ('catalog-v1', '1.0.1', 'steam:24181527', '11', 'catalog', \
                     'full_catalog', 1, CURRENT_TIMESTAMP);
             INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, \
              dataset_scope, verified, activated_at) \
             VALUES ('public-meta-v1', '1.0.1', 'steam:24181527', '11', 'metadata', \
                     'public_metadata', 1, CURRENT_TIMESTAMP);",
        )
        .unwrap();

    let active_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM data_versions \
             WHERE verified=1 AND activated_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active_count, 2);
}

#[test]
fn observing_new_build_preserves_remote_compatibility_decision() {
    let connection = migrated_connection();
    connection
        .execute_batch(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, verified) \
             VALUES ('data-v1', '1.0.1', '24181527', '6', 'fixture', 1);
             INSERT INTO game_build_policies \
             (game_build_id, compatibility_state, dataset_version, observation_count) \
             VALUES ('new-build', 'compatible', 'data-v1', 1);
             INSERT INTO game_build_policies \
             (game_build_id, compatibility_state, projection_schema, observation_count, \
              last_parser_version, last_requested_dataset_version) \
             VALUES ('new-build', 'observed', 'cloud-profile-projection-v1', 1, \
                     'parser-v2', 'data-v1') \
             ON CONFLICT(game_build_id) DO UPDATE SET \
               observation_count=game_build_policies.observation_count+1, \
               last_parser_version=excluded.last_parser_version, \
               last_requested_dataset_version=excluded.last_requested_dataset_version, \
               last_seen_at=CURRENT_TIMESTAMP, updated_at=CURRENT_TIMESTAMP;",
        )
        .unwrap();

    let policy = connection
        .query_row(
            "SELECT compatibility_state, dataset_version, observation_count, \
             last_parser_version FROM game_build_policies WHERE game_build_id='new-build'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        policy,
        (
            "compatible".to_owned(),
            "data-v1".to_owned(),
            2,
            "parser-v2".to_owned()
        )
    );
}

#[test]
fn active_skills_and_aliases_are_queryable() {
    let connection = migrated_connection();
    connection
        .execute_batch(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, verified) \
             VALUES ('data', 'unverified', 'unverified', '4', 'fixture', 0);
             INSERT INTO catalog_active_skills \
             (dataset_version, skill_id, name_ko, description_ko, element, skill_kind, \
              power, min_range, max_range, cool_time, effects_json) \
             VALUES ('data', 'DragonWave', '용의 파동', '용의 힘을 방출한다', 'Dragon', \
                     'Shot', 80, 0, 200, 4.0, '[{\"type\":\"Burn\",\"value\":50}]');
             INSERT INTO catalog_aliases \
             (dataset_version, entity_kind, entity_id, language, alias, normalized_alias) \
             VALUES ('data', 'active_skill', 'DragonWave', 'ko', '용 파동', '용파동');",
        )
        .unwrap();

    let row = connection
        .query_row(
            "SELECT skill.name_ko, skill.power, skill.cool_time, alias.alias \
             FROM catalog_active_skills skill \
             JOIN catalog_aliases alias \
               ON alias.dataset_version=skill.dataset_version \
              AND alias.entity_kind='active_skill' AND alias.entity_id=skill.skill_id \
             WHERE skill.dataset_version='data' AND alias.normalized_alias='용파동'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row, ("용의 파동".to_owned(), 80, 4.0, "용 파동".to_owned()));
}

#[test]
fn item_catalog_keeps_recipe_and_pal_drop_context() {
    let connection = migrated_connection();
    connection
        .execute_batch(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, verified) \
             VALUES ('data', 'steam-build-24181527', '24181527', '5', 'fixture', 0);
             INSERT INTO catalog_species \
             (dataset_version, species_id, name_ko) \
             VALUES ('data', 'GrassMammoth', '그린모스');
             INSERT INTO catalog_items \
             (dataset_version, item_id, name_ko, category, price, description_ko, \
              subcategory, weight_milli, maximum_stack_count, rarity, rank, icon_name, \
              legal_in_game, localization_fallback) \
             VALUES ('data', 'PalOil', '고급 팰 기름', 'Material', 300, '고품질 기름', \
                     'Material', 200, 9999, 1, 1, 'PalOil', 1, 0);
             INSERT INTO catalog_recipes \
             (dataset_version, recipe_id, output_item_id, output_quantity, ingredients_json, \
              work_amount) \
             VALUES ('data', 'PalOil', 'PalOil', 1, \
                     '[{\"item_id\":\"BerrySeeds\",\"name_ko\":\"빨간 열매 씨\",\"quantity\":2}]', \
                     100);
             INSERT INTO acquisition_methods \
             (dataset_version, method_id, item_id, method_kind, source_entity_id, \
              quantity_min, quantity_max, probability, requirements_json) \
             VALUES ('data', 'pal-drop:GrassMammoth:1', 'PalOil', 'pal_drop', \
                     'GrassMammoth', 5, 10, 1.0, \
                     '{\"level\":1,\"probability_ppm\":1000000}');",
        )
        .unwrap();

    let row = connection
        .query_row(
            "SELECT ci.name_ko, \
             (SELECT json_group_array(json_object(\
               'recipe_id', cr.recipe_id, 'ingredients', json(cr.ingredients_json))) \
              FROM catalog_recipes cr \
              WHERE cr.dataset_version=ci.dataset_version \
                AND cr.output_item_id=ci.item_id), \
             (SELECT json_group_array(json_object(\
               'pal_id', am.source_entity_id, 'pal_name_ko', cs.name_ko, \
               'probability_ppm', CAST(ROUND(am.probability * 1000000) AS INTEGER))) \
              FROM acquisition_methods am \
              LEFT JOIN catalog_species cs \
                ON cs.dataset_version=am.dataset_version \
               AND cs.species_id=am.source_entity_id \
              WHERE am.dataset_version=ci.dataset_version \
                AND am.item_id=ci.item_id AND am.method_kind='pal_drop') \
             FROM catalog_items ci \
             WHERE ci.dataset_version='data' AND ci.item_id='PalOil'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "고급 팰 기름");
    let recipes: serde_json::Value = serde_json::from_str(&row.1).unwrap();
    let drops: serde_json::Value = serde_json::from_str(&row.2).unwrap();
    assert_eq!(recipes[0]["ingredients"][0]["name_ko"], "빨간 열매 씨");
    assert_eq!(drops[0]["pal_name_ko"], "그린모스");
    assert_eq!(drops[0]["probability_ppm"], 1_000_000);
}

#[test]
fn catalog_detail_columns_are_additive_and_queryable() {
    let connection = migrated_connection();
    connection
        .execute_batch(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, verified) \
             VALUES ('data', 'unverified', 'unverified', '1', 'fixture', 0);
             INSERT INTO catalog_species \
             (dataset_version, species_id, name_ko, paldex_number, rarity, run_speed, \
              transport_speed, food_amount, nocturnal, learned_skills_json, \
              guaranteed_passives_json) \
             VALUES ('data', 'SkyDragon', '페스키', 95, 8, 900, 450, 7, 0, \
                     '[{\"skill_id\":\"DragonWave\",\"learn_level\":1}]', \
                     '[\"Rare\"]');",
        )
        .unwrap();

    let row = connection
        .query_row(
            "SELECT paldex_number, rarity, run_speed, transport_speed, food_amount, \
                    nocturnal, learned_skills_json, guaranteed_passives_json \
             FROM catalog_species WHERE dataset_version='data' AND species_id='SkyDragon'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        (row.0, row.1, row.2, row.3, row.4, row.5),
        (95, 8, 900, 450, 7, 0)
    );
    assert!(row.6.contains("DragonWave"));
    assert_eq!(row.7, "[\"Rare\"]");
}

#[test]
fn unverified_catalog_cannot_be_activated() {
    let connection = migrated_connection();
    connection
        .execute(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, verified) \
             VALUES ('candidate', 'unverified', 'unverified', '3', 'fixture', 0)",
            [],
        )
        .unwrap();
    let activation = connection.execute(
        "UPDATE data_versions SET activated_at=CURRENT_TIMESTAMP \
         WHERE dataset_version='candidate'",
        [],
    );
    assert!(activation.is_err());
    let activated: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM data_versions WHERE activated_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(activated, 0);
}

#[test]
fn only_one_verified_catalog_can_be_active() {
    let connection = migrated_connection();
    connection
        .execute_batch(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, \
              verified, activated_at) \
             VALUES ('active-a', '1.0.1', 'build-a', '3', 'a', 1, CURRENT_TIMESTAMP);
             INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, verified) \
             VALUES ('active-b', '1.0.1', 'build-b', '3', 'b', 1);",
        )
        .unwrap();
    let activation = connection.execute(
        "UPDATE data_versions SET activated_at=CURRENT_TIMESTAMP \
         WHERE dataset_version='active-b'",
        [],
    );
    assert!(activation.is_err());
}

#[test]
fn one_owner_can_bind_to_only_one_principal() {
    let connection = migrated_connection();
    connection
        .execute(
            "INSERT INTO principals (principal_id, access_subject, email, role) \
             VALUES ('p1', 's1', 'one@example.com', 'operator'), \
                    ('p2', 's2', 'two@example.com', 'member')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO owner_bindings \
             (owner_subject, principal_id, world_id, bound_by_principal_id) \
             VALUES ('owner-a', 'p1', 'world', 'p1')",
            [],
        )
        .unwrap();
    let result = connection.execute(
        "INSERT INTO owner_bindings \
         (owner_subject, principal_id, world_id, bound_by_principal_id) \
         VALUES ('owner-a', 'p2', 'world', 'p1')",
        [],
    );
    assert!(result.is_err());
}

#[test]
fn active_projection_is_unique_per_owner_and_world() {
    let connection = migrated_connection();
    connection
        .execute(
            "INSERT INTO service_agents (agent_id, world_id, access_common_name) \
             VALUES ('agent', 'world', 'agent-token')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO sync_runs \
             (sync_id, agent_id, world_id, game_build_id, parser_version, projection_schema, status) \
             VALUES ('sync-1', 'agent', 'world', '24181527', 'v1', 'v1', 'activated'), \
                    ('sync-2', 'agent', 'world', '24181527', 'v1', 'v1', 'validated')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO projection_commits \
             (projection_id, owner_subject, world_id, sync_id, game_build_id, \
              dataset_version, projected_at, is_active) \
             VALUES ('projection-1', 'owner', 'world', 'sync-1', '24181527', \
                     'data', CURRENT_TIMESTAMP, 1)",
            [],
        )
        .unwrap();
    let result = connection.execute(
        "INSERT INTO projection_commits \
         (projection_id, owner_subject, world_id, sync_id, game_build_id, \
          dataset_version, projected_at, is_active) \
         VALUES ('projection-2', 'owner', 'world', 'sync-2', '24181527', \
                 'data', CURRENT_TIMESTAMP, 1)",
        [],
    );
    assert!(result.is_err());
}

#[test]
fn inventory_slots_keep_same_slot_number_in_different_containers() {
    let connection = migrated_connection();
    connection
        .execute_batch(
            "INSERT INTO service_agents (agent_id, world_id, access_common_name) \
             VALUES ('agent', 'world', 'agent-token');
             INSERT INTO sync_runs \
             (sync_id, agent_id, world_id, game_build_id, parser_version, projection_schema, status) \
             VALUES ('sync', 'agent', 'world', '24181527', 'v1', 'v1', 'activated');
             INSERT INTO projection_commits \
             (projection_id, owner_subject, world_id, sync_id, game_build_id, \
              dataset_version, projected_at, is_active) \
             VALUES ('projection', 'owner', 'world', 'sync', '24181527', \
                     'data', CURRENT_TIMESTAMP, 1);",
        )
        .unwrap();

    connection
        .execute(
            "INSERT INTO inventory_slots \
             (projection_id, container_kind, container_ordinal, slot_index, item_id, quantity) \
             VALUES ('projection', 'storage', 0, 0, 'PalOil', 10), \
                    ('projection', 'storage', 1, 0, 'PalOil', 20)",
            [],
        )
        .unwrap();

    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM inventory_slots", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn catalog_can_find_species_by_korean_name_alias_and_work_trait() {
    let connection = migrated_connection();
    connection
        .execute_batch(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, verified) \
             VALUES ('data', '1.0.1', '24181527', '1', 'fixture', 1);
             INSERT INTO catalog_species \
             (dataset_version, species_id, name_ko, name_en, work_json) \
             VALUES ('data', 'Quivern', '페스키', 'Quivern', '{\"Transport\":3,\"Mining\":2}');
             INSERT INTO catalog_aliases \
             (dataset_version, entity_kind, entity_id, language, alias, normalized_alias) \
             VALUES ('data', 'species', 'Quivern', 'ko', '흰 용', '흰 용');",
        )
        .unwrap();

    for query in ["페스키", "흰 용", "채굴"] {
        let pattern = format!("%{query}%");
        let work_pattern = if query == "채굴" {
            "%mining%".to_owned()
        } else {
            pattern.clone()
        };
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(DISTINCT cs.species_id) \
                 FROM catalog_species cs \
                 LEFT JOIN catalog_aliases ca \
                   ON ca.dataset_version=cs.dataset_version \
                  AND ca.entity_kind='species' \
                  AND ca.entity_id=cs.species_id \
                 WHERE cs.dataset_version='data' AND (\
                   cs.name_ko LIKE ?1 OR cs.name_en LIKE ?1 COLLATE NOCASE OR \
                   cs.species_id LIKE ?1 COLLATE NOCASE OR cs.work_json LIKE ?1 OR \
                   cs.work_json LIKE ?2 COLLATE NOCASE OR ca.alias LIKE ?1 COLLATE NOCASE)",
                [&pattern, &work_pattern],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "query {query} should find 페스키");
    }
}

#[test]
fn catalog_can_find_passive_by_korean_name_or_alias() {
    let connection = migrated_connection();
    connection
        .execute_batch(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, verified) \
             VALUES ('data', '1.0.1', '24181527', '1', 'fixture', 1);
             INSERT INTO catalog_passives \
             (dataset_version, passive_id, name_ko, description_ko) \
             VALUES ('data', 'Swift', '신속', '이동 속도가 증가한다');
             INSERT INTO catalog_aliases \
             (dataset_version, entity_kind, entity_id, language, alias, normalized_alias) \
             VALUES ('data', 'passive', 'Swift', 'ko', '이속 30', '이속 30');",
        )
        .unwrap();

    for query in ["신속", "이속 30"] {
        let pattern = format!("%{query}%");
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(DISTINCT cp.passive_id) \
                 FROM catalog_passives cp \
                 LEFT JOIN catalog_aliases ca \
                   ON ca.dataset_version=cp.dataset_version \
                  AND ca.entity_kind='passive' \
                  AND ca.entity_id=cp.passive_id \
                 WHERE cp.dataset_version='data' AND (\
                   cp.name_ko LIKE ?1 OR cp.passive_id LIKE ?1 COLLATE NOCASE OR \
                   cp.description_ko LIKE ?1 OR ca.alias LIKE ?1 COLLATE NOCASE)",
                [&pattern],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "query {query} should find 신속");
    }
}
