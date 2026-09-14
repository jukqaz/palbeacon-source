use std::{cell::RefCell, collections::BTreeMap};

use futures_util::StreamExt;
use js_sys::{Array, Date, Object, Reflect, Uint8Array};
use pal_companion_service::{
    AccessRole, AssistantEvidence, AssistantEvidenceKind, BreedingRule, GroundedAssistantRequest,
    GroundedAssistantResponse, KnowledgeRetrievalPlan, OwnedPalSeed, PrincipalContext,
    ProtectedCapability, solve_owned_breeding,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use sha2::{Digest, Sha256};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Crypto, CryptoKey};
use worker::{
    Cache, Context, D1Database, Env, Fetch, Headers, Method, Request, Response, ResponseBuilder,
    Result as WorkerResult, event,
};

use crate::{
    access_token::ParsedAccessAssertion,
    build_policy::{BuildPolicyDecision, evaluate_build_policy, normalize_game_build_id},
    public_cache::{for_get_path as public_cache_policy_for_get_path, if_none_match_matches},
    public_catalog::{MANIFEST_PATH as PUBLIC_CATALOG_MANIFEST_PATH, PublicCatalogManifest},
    public_media::{
        ROUTE_PREFIX as PUBLIC_MEDIA_ROUTE_PREFIX, content_type_from_object_key,
        object_key_from_path,
    },
    web_visibility::is_private_web_path,
};

mod knowledge;
mod ops_knowledge;

const ACCESS_ASSERTION_HEADER: &str = "Cf-Access-Jwt-Assertion";
const JWKS_CACHE_MS: f64 = 60.0 * 60.0 * 1_000.0;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_GROUNDED_ASSISTANT_BODY_BYTES: usize = 128 * 1024;
const MAX_IMPORT_PALS: usize = 10_000;
const MAX_IMPORT_INVENTORY: usize = 10_000;
const PUBLIC_MEDIA_CACHE_REVISION: &str = "2";

thread_local! {
    static JWKS_CACHE: RefCell<Option<CachedJwks>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct CachedJwks {
    expires_at_ms: f64,
    keys: Vec<JsonWebKey>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Jwks {
    keys: Vec<JsonWebKey>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct JsonWebKey {
    kty: String,
    kid: String,
    #[serde(default)]
    alg: Option<String>,
    #[serde(rename = "use", default)]
    usage: Option<String>,
    n: String,
    e: String,
}

#[derive(Debug, Deserialize)]
struct ProjectionRow {
    projection_id: String,
    game_build_id: String,
    dataset_version: String,
    projected_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ProfileRow {
    character_name: String,
    level: i32,
    experience: Option<i32>,
    hp: Option<i32>,
    stamina: Option<i32>,
    attack: Option<i32>,
    defense: Option<i32>,
    work_speed: Option<i32>,
    carry_weight: Option<i32>,
    gold: Option<i32>,
    last_save_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct OwnedPalRow {
    pal_instance_id: String,
    species_id: String,
    species_name: String,
    nickname: Option<String>,
    level: i32,
    gender: Option<String>,
    rank: i32,
    iv_hp: Option<i32>,
    iv_attack: Option<i32>,
    iv_defense: Option<i32>,
    location_kind: String,
    location_label: Option<String>,
    passive_ids: String,
    passive_names: String,
    work_json: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct InventorySummaryRow {
    item_id: String,
    item_name: String,
    total_quantity: i64,
    container_count: i64,
}

#[derive(Debug, Deserialize)]
struct BreedingRuleRow {
    parent_a_species_id: String,
    parent_b_species_id: String,
    child_species_id: String,
}

#[derive(Debug, Deserialize)]
struct BreedingApiRequest {
    #[serde(default)]
    target_species_id: Option<String>,
    #[serde(default)]
    target_species_query: Option<String>,
    #[serde(default)]
    required_passive_ids: Vec<String>,
    #[serde(default)]
    required_passive_queries: Vec<String>,
    #[serde(default = "default_max_generations")]
    maximum_generations: u32,
}

#[derive(Debug, Deserialize)]
struct CatalogSearchApiRequest {
    query: String,
    kind: String,
    #[serde(default = "default_catalog_search_limit")]
    limit: u32,
}

#[derive(Debug, Deserialize)]
struct CatalogSpeciesSearchRow {
    species_id: String,
    name_ko: String,
    name_en: Option<String>,
    paldex_number: Option<i64>,
    rarity: Option<i64>,
    aliases: String,
    element_json: String,
    work_json: String,
    hp: Option<i64>,
    attack: Option<i64>,
    defense: Option<i64>,
    run_speed: Option<i64>,
    ride_sprint_speed: Option<i64>,
    transport_speed: Option<i64>,
    stamina: Option<i64>,
    food_amount: Option<i64>,
    nocturnal: i64,
    learned_skills_json: String,
    guaranteed_passives_json: String,
    owned_count: i64,
}

#[derive(Debug, Deserialize)]
struct CatalogPassiveSearchRow {
    passive_id: String,
    name_ko: String,
    description_ko: Option<String>,
    aliases: String,
    owned_donor_count: i64,
}

#[derive(Debug, Deserialize)]
struct CatalogActiveSkillSearchRow {
    skill_id: String,
    name_ko: String,
    description_ko: Option<String>,
    element: Option<String>,
    skill_kind: Option<String>,
    power: Option<i64>,
    min_range: Option<i64>,
    max_range: Option<i64>,
    cool_time: Option<f64>,
    effects_json: String,
    aliases: String,
}

#[derive(Debug, Deserialize)]
struct CatalogItemSearchRow {
    item_id: String,
    name_ko: String,
    description_ko: Option<String>,
    category: Option<String>,
    subcategory: Option<String>,
    price: Option<i64>,
    weight_milli: i64,
    maximum_stack_count: i64,
    rarity: i64,
    rank: i64,
    icon_name: Option<String>,
    legal_in_game: i64,
    localization_fallback: i64,
    aliases: String,
    recipe_count: i64,
    pal_drop_count: i64,
    recipes_json: String,
    pal_drops_json: String,
}

#[derive(Debug, Deserialize)]
struct CatalogIdRow {
    entity_id: String,
}

#[derive(Debug, Deserialize)]
struct CatalogNameRow {
    entity_id: String,
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct AssistantApiRequest {
    question: String,
}

#[derive(Debug, Deserialize)]
struct AiOutput {
    response: String,
}

#[derive(Debug, Serialize)]
struct AiInputMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct AiInput<'a> {
    messages: Vec<AiInputMessage<'a>>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Deserialize, Serialize)]
struct BindingRow {
    owner_subject: String,
    principal_id: String,
    email: String,
    world_id: String,
    character_name: Option<String>,
    active: i32,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct BindingApiRequest {
    owner_subject: String,
    member_email: String,
    world_id: String,
    #[serde(default)]
    character_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RevokeBindingApiRequest {
    owner_subject: String,
}

#[derive(Debug, Deserialize)]
struct GameBuildPolicyApiRequest {
    game_build_id: String,
    compatibility_state: String,
    #[serde(default)]
    dataset_version: Option<String>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GameBuildPolicyRow {
    game_build_id: String,
    compatibility_state: String,
    dataset_version: Option<String>,
    projection_schema: String,
    observation_count: i64,
    last_parser_version: Option<String>,
    last_requested_dataset_version: Option<String>,
    notes: Option<String>,
    first_seen_at: String,
    last_seen_at: String,
    reviewed_at: Option<String>,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct ProjectionImportRequest {
    projection_id: String,
    sync_id: String,
    owner_subject: String,
    world_id: String,
    game_build_id: String,
    dataset_version: String,
    parser_version: String,
    projected_at: String,
    profile: ImportedProfile,
    #[serde(default)]
    pals: Vec<ImportedPal>,
    #[serde(default)]
    inventory: Vec<ImportedInventorySlot>,
}

#[derive(Debug, Deserialize)]
struct ImportedProfile {
    level: i32,
    #[serde(default)]
    experience: Option<i32>,
    #[serde(default)]
    hp: Option<i32>,
    #[serde(default)]
    stamina: Option<i32>,
    #[serde(default)]
    attack: Option<i32>,
    #[serde(default)]
    defense: Option<i32>,
    #[serde(default)]
    work_speed: Option<i32>,
    #[serde(default)]
    carry_weight: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct ImportedPal {
    pal_instance_id: String,
    species_id: String,
    level: i32,
    #[serde(default)]
    gender: Option<String>,
    #[serde(default)]
    rank: i32,
    #[serde(default)]
    iv_hp: Option<i32>,
    #[serde(default)]
    iv_attack: Option<i32>,
    #[serde(default)]
    iv_defense: Option<i32>,
    location_kind: String,
    #[serde(default)]
    container_ordinal: i32,
    #[serde(default)]
    slot_index: i32,
    #[serde(default)]
    passive_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ImportedInventorySlot {
    container_kind: String,
    #[serde(default)]
    container_ordinal: i32,
    slot_index: i32,
    item_id: String,
    quantity: i32,
}

#[derive(Debug, Deserialize, Serialize)]
struct SyncRow {
    sync_id: String,
    agent_id: String,
    world_id: String,
    game_build_id: String,
    parser_version: String,
    status: String,
    owner_count: i32,
    pal_count: i32,
    inventory_slot_count: i32,
    error_code: Option<String>,
    received_at: String,
    completed_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct AuditRow {
    event_id: i64,
    action: String,
    target_kind: String,
    target_key_redacted: Option<String>,
    outcome: String,
    request_id: Option<String>,
    created_at: String,
}

#[event(fetch)]
pub async fn main(mut req: Request, env: Env, ctx: Context) -> WorkerResult<Response> {
    if req.path().starts_with(PUBLIC_MEDIA_ROUTE_PREFIX) {
        return public_media_response(&req, &env, &ctx).await;
    }
    if is_private_web_path(&req.path()) {
        let mut not_found_url = req.url()?;
        not_found_url.set_path("/404.html");
        not_found_url.set_query(None);
        not_found_url.set_fragment(None);
        let assets = env.assets("ASSETS")?;
        return Ok(assets
            .fetch(not_found_url.to_string(), None)
            .await?
            .with_status(404));
    }
    if !req.path().starts_with("/api/") {
        return error_response(404, "NOT_FOUND", "API route not found");
    }
    if req.method() == Method::Options {
        return Response::empty();
    }

    let request_id = request_id(&req);
    let cache_policy = (req.method() == Method::Get)
        .then(|| public_cache_policy_for_get_path(&req.path()))
        .flatten();
    let cache_key = cache_policy.zip(req.url().ok()).map(|(policy, mut url)| {
        if !policy.varies_by_query() {
            url.set_query(None);
        }
        url.set_fragment(None);
        url.to_string()
    });
    if let Some(policy) = cache_policy
        && let Some(cache_key) = cache_key.as_deref()
        && let Ok(Some(response)) = Cache::default().get(cache_key, false).await
    {
        let mut response = prepare_cached_response(&req, response)?;
        let cache_control = policy
            .cache_control_for_status(response.status_code())
            .unwrap_or("no-store");
        set_security_headers(&mut response, &request_id, cache_control, "HIT")?;
        return Ok(response);
    }

    let result = handle_api(&mut req, &env, &request_id).await;
    match result {
        Ok(mut response) => {
            if let (Some(policy), Some(cache_key)) = (cache_policy, cache_key)
                && let Some(cache_control) = policy.cache_control_for_status(response.status_code())
            {
                set_security_headers(&mut response, &request_id, cache_control, "MISS")?;
                set_response_etag(&mut response).await?;
                let cached_response = response.cloned()?;
                ctx.wait_until(async move {
                    let _ = Cache::default().put(cache_key, cached_response).await;
                });
            } else {
                set_security_headers(&mut response, &request_id, "no-store", "BYPASS")?;
            }
            Ok(response)
        }
        Err(error) => {
            let mut response = error_response(error.status, error.code, &error.message)?;
            let cache_control = cache_policy
                .and_then(|policy| policy.cache_control_for_status(response.status_code()));
            if let (Some(cache_control), Some(cache_key)) = (cache_control, cache_key) {
                set_security_headers(&mut response, &request_id, cache_control, "MISS")?;
                let cached_response = response.cloned()?;
                ctx.wait_until(async move {
                    let _ = Cache::default().put(cache_key, cached_response).await;
                });
            } else {
                set_security_headers(&mut response, &request_id, "no-store", "BYPASS")?;
            }
            Ok(response)
        }
    }
}

async fn public_media_response(req: &Request, env: &Env, ctx: &Context) -> WorkerResult<Response> {
    let method = req.method();
    if method == Method::Options {
        let headers = Headers::new();
        headers.set("Allow", "GET, HEAD, OPTIONS")?;
        return Ok(Response::empty()?.with_status(204).with_headers(headers));
    }
    if method != Method::Get && method != Method::Head {
        let headers = Headers::new();
        headers.set("Allow", "GET, HEAD, OPTIONS")?;
        return Ok(Response::error("Method not allowed", 405)?.with_headers(headers));
    }

    let object_key = match object_key_from_path(&req.path()) {
        Ok(value) => value,
        Err(_) => return Response::error("Invalid public media path", 400),
    };
    let mut cache_url = req.url()?;
    cache_url.set_query(None);
    cache_url.set_fragment(None);
    cache_url
        .query_pairs_mut()
        .append_pair("__pal_media_cache", PUBLIC_MEDIA_CACHE_REVISION);
    let cache_key = cache_url.to_string();
    let cache = Cache::default();

    if method == Method::Get
        && let Ok(Some(cached)) = cache.get(&cache_key, false).await
    {
        return Ok(cached);
    }

    let bucket = env.bucket("PUBLIC_MEDIA")?;
    let object = if method == Method::Head {
        bucket.head(&object_key).await?
    } else {
        bucket.get(&object_key).execute().await?
    };
    let Some(object) = object else {
        return Response::error("Public media not found", 404);
    };

    let headers = Headers::new();
    object.write_http_metadata(headers.clone())?;
    headers.set(
        "Content-Type",
        content_type_from_object_key(&object_key)
            .ok_or_else(|| worker::Error::RustError("public media type is unavailable".into()))?,
    )?;
    headers.set("Cache-Control", "public, max-age=31536000, immutable")?;
    headers.set("ETag", &object.http_etag())?;
    headers.set("Content-Length", &object.size().to_string())?;
    headers.set("Accept-Ranges", "bytes")?;
    headers.set("Cross-Origin-Resource-Policy", "same-origin")?;
    headers.set("X-Content-Type-Options", "nosniff")?;
    headers.set("X-Pal-Media-Cache", "MISS")?;

    if let Some(if_none_match) = req.headers().get("If-None-Match")?
        && if_none_match_matches(&if_none_match, &object.http_etag())
    {
        return Ok(Response::empty()?.with_status(304).with_headers(headers));
    }

    if method == Method::Head {
        return Ok(Response::empty()?.with_headers(headers));
    }
    let body = object
        .body()
        .ok_or_else(|| worker::Error::RustError("R2 object body is unavailable".into()))?
        .response_body()?;
    let mut response = Response::from_body(body)?.with_headers(headers);
    let mut cached_response = response.cloned()?;
    cached_response
        .headers_mut()
        .set("X-Pal-Media-Cache", "HIT")?;
    ctx.wait_until(async move {
        let _ = Cache::default().put(cache_key, cached_response).await;
    });
    Ok(response)
}

async fn handle_api(req: &mut Request, env: &Env, request_id: &str) -> Result<Response, ApiError> {
    let path = req.path();
    let method = req.method();

    if path == "/api/v1/catalog/search" {
        return Err(ApiError::not_found(
            "Public catalog search is served from versioned static data",
        ));
    }
    if method != Method::Get
        && matches!(
            path.as_str(),
            "/api/v1/wiki/search" | "/api/v1/wiki/read" | "/api/v1/knowledge/related"
        )
    {
        return Err(ApiError::method_not_allowed(
            "Public knowledge routes accept GET only",
        ));
    }

    // Public reference data is versioned server-side and never checks a
    // visitor's installed game build.
    if method == Method::Get && path == "/api/v1/catalog/status" {
        return catalog_status_response(req, env).await;
    }

    let db = env.d1("DB").map_err(ApiError::internal)?;

    if method == Method::Get && path == "/api/v1/knowledge/status" {
        return knowledge::status_response(&db).await;
    }
    if method == Method::Get && path == "/api/v1/wiki/search" {
        return knowledge::wiki_search_response(req, &db).await;
    }
    if method == Method::Get && path == "/api/v1/wiki/read" {
        return knowledge::wiki_read_response(req, &db).await;
    }
    if method == Method::Get && path == "/api/v1/knowledge/related" {
        return knowledge::related_response(req, &db).await;
    }

    let personal_route =
        path == "/api/v1/me" || path.starts_with("/api/v1/me/") || path.starts_with("/api/v1/ops/");
    if !personal_route || !optional_bool_var(env, "PALBEACON_ENABLE_PERSONAL_CLOUD") {
        return Err(ApiError::not_found("API route not found"));
    }

    // The public PalBeacon deployment leaves personal cloud disabled. This
    // compatibility path can only be enabled explicitly in a separately
    // protected deployment with its complete identity configuration.
    let principal = authenticate_human(req, env).await?;
    upsert_principal(&db, &principal).await?;

    match (method, path.as_str()) {
        (Method::Get, "/api/v1/me") => me_response(&db, &principal).await,
        (Method::Get, "/api/v1/me/pals") => pals_response(&db, &principal).await,
        (Method::Get, "/api/v1/me/inventory") => inventory_response(&db, &principal).await,
        (Method::Post, "/api/v1/me/catalog/search") => {
            catalog_search_response(req, &db, Some(&principal)).await
        }
        (Method::Post, "/api/v1/me/breeding") => breeding_response(req, &db, &principal).await,
        (Method::Post, "/api/v1/me/assistant") => {
            assistant_response(req, env, &db, &principal, request_id).await
        }
        (Method::Post, "/api/v1/me/assistant/grounded") => {
            grounded_assistant_response(req, env, &db, &principal, request_id).await
        }
        (Method::Get, "/api/v1/ops/health") => ops_health(&db, &principal).await,
        (Method::Get, "/api/v1/ops/bindings") => ops_bindings(&db, &principal).await,
        (Method::Post, "/api/v1/ops/bindings") => {
            ops_upsert_binding(req, &db, &principal, request_id).await
        }
        (Method::Post, "/api/v1/ops/bindings/revoke") => {
            ops_revoke_binding(req, &db, &principal, request_id).await
        }
        (Method::Post, "/api/v1/ops/projections/import") => {
            ops_import_projection(req, &db, &principal, request_id).await
        }
        (Method::Get, "/api/v1/ops/game-builds") => ops_game_builds(&db, &principal).await,
        (Method::Post, "/api/v1/ops/game-builds") => {
            ops_upsert_game_build(req, &db, &principal, request_id).await
        }
        (Method::Get, "/api/v1/ops/sync-runs") => ops_sync_runs(&db, &principal).await,
        (Method::Get, "/api/v1/ops/audit") => ops_audit(&db, &principal).await,
        (Method::Get, "/api/v1/ops/knowledge/errors") => {
            ops_knowledge::errors_response(req, &db, &principal).await
        }
        (Method::Post, "/api/v1/ops/knowledge/errors/review") => {
            ops_knowledge::review_error_response(req, &db, &principal, request_id).await
        }
        (Method::Get, "/api/v1/ops/knowledge/manifests") => {
            ops_knowledge::manifests_response(req, &db, &principal).await
        }
        (Method::Post, "/api/v1/ops/knowledge/manifests/validate") => {
            ops_knowledge::validate_manifest_response(req, &db, &principal, request_id).await
        }
        (Method::Get, "/api/v1/ops/knowledge/wiki-revisions") => {
            ops_knowledge::wiki_revisions_response(req, &db, &principal).await
        }
        (Method::Post, "/api/v1/ops/knowledge/wiki-revisions/review") => {
            ops_knowledge::review_wiki_revision_response(req, &db, &principal, request_id).await
        }
        _ => Err(ApiError::not_found("API route not found")),
    }
}

fn optional_bool_var(env: &Env, name: &str) -> bool {
    env.var(name)
        .ok()
        .is_some_and(|value| value.to_string().eq_ignore_ascii_case("true"))
}

async fn authenticate_human(req: &Request, env: &Env) -> Result<PrincipalContext, ApiError> {
    let assertion = req
        .headers()
        .get(ACCESS_ASSERTION_HEADER)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::unauthorized("Cloudflare Access assertion is missing"))?;
    let parsed = ParsedAccessAssertion::parse(&assertion)
        .map_err(|_| ApiError::unauthorized("Cloudflare Access assertion is invalid"))?;

    let team_domain = required_var(env, "ACCESS_TEAM_DOMAIN")?;
    let audience = required_var(env, "ACCESS_POLICY_AUD")?;
    let operator_email = required_var(env, "OPERATOR_EMAIL")?;
    let issuer = format!("https://{}", team_domain.trim().trim_end_matches('/'));
    let keys = load_jwks(&team_domain, false).await?;
    let mut key = keys.iter().find(|key| key.kid == parsed.header.kid);
    let refreshed;
    if key.is_none() {
        refreshed = load_jwks(&team_domain, true).await?;
        key = refreshed
            .iter()
            .find(|entry| entry.kid == parsed.header.kid);
    }
    let key = key.ok_or_else(|| ApiError::unauthorized("Access signing key is unknown"))?;
    verify_rs256(&parsed, key).await?;

    let now = (Date::now() / 1_000.0) as i64;
    let identity = parsed
        .validate_verified(&issuer, &audience, now)
        .map_err(|_| ApiError::unauthorized("Cloudflare Access claims are invalid"))?;
    PrincipalContext::from_verified_identity(identity, &operator_email)
        .map_err(|_| ApiError::unauthorized("Cloudflare Access identity is invalid"))
}

async fn load_jwks(team_domain: &str, force: bool) -> Result<Vec<JsonWebKey>, ApiError> {
    if !force {
        let now = Date::now();
        if let Some(keys) = JWKS_CACHE.with(|cache| {
            cache
                .borrow()
                .as_ref()
                .filter(|cached| cached.expires_at_ms > now)
                .map(|cached| cached.keys.clone())
        }) {
            return Ok(keys);
        }
    }

    let domain = team_domain.trim().trim_end_matches('/');
    let url = format!("https://{domain}/cdn-cgi/access/certs");
    let mut response = Fetch::Url(
        url.parse()
            .map_err(|_| ApiError::internal("invalid Access team domain"))?,
    )
    .send()
    .await
    .map_err(ApiError::internal)?;
    if response.status_code() != 200 {
        return Err(ApiError::unavailable(
            "Cloudflare Access signing keys are unavailable",
        ));
    }
    let jwks: Jwks = response.json().await.map_err(ApiError::internal)?;
    if jwks.keys.is_empty() {
        return Err(ApiError::unavailable(
            "Cloudflare Access signing keys are empty",
        ));
    }
    JWKS_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(CachedJwks {
            expires_at_ms: Date::now() + JWKS_CACHE_MS,
            keys: jwks.keys.clone(),
        });
    });
    Ok(jwks.keys)
}

async fn verify_rs256(assertion: &ParsedAccessAssertion, jwk: &JsonWebKey) -> Result<(), ApiError> {
    if jwk.kty != "RSA"
        || jwk.alg.as_deref().is_some_and(|alg| alg != "RS256")
        || jwk.usage.as_deref().is_some_and(|usage| usage != "sig")
    {
        return Err(ApiError::unauthorized("Access signing key is not RS256"));
    }

    let global = js_sys::global();
    let crypto: Crypto = Reflect::get(&global, &JsValue::from_str("crypto"))
        .map_err(ApiError::internal_js)?
        .dyn_into()
        .map_err(|_| ApiError::internal("WebCrypto is unavailable"))?;
    let subtle = crypto.subtle();
    let key_data: Object = serde_wasm_bindgen::to_value(jwk)
        .map_err(ApiError::internal)?
        .dyn_into()
        .map_err(|_| ApiError::internal("invalid JWK object"))?;
    let algorithm = Object::new();
    Reflect::set(
        &algorithm,
        &JsValue::from_str("name"),
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )
    .map_err(ApiError::internal_js)?;
    Reflect::set(
        &algorithm,
        &JsValue::from_str("hash"),
        &JsValue::from_str("SHA-256"),
    )
    .map_err(ApiError::internal_js)?;
    let usages = Array::of1(&JsValue::from_str("verify"));
    let key_value = JsFuture::from(
        subtle
            .import_key_with_object("jwk", &key_data, &algorithm, false, usages.as_ref())
            .map_err(ApiError::internal_js)?,
    )
    .await
    .map_err(ApiError::internal_js)?;
    let key: CryptoKey = key_value
        .dyn_into()
        .map_err(|_| ApiError::internal("Access CryptoKey import failed"))?;
    let signature = Uint8Array::from(assertion.signature.as_slice());
    let verified = JsFuture::from(
        subtle
            .verify_with_str_and_u8_array_and_u8_slice(
                "RSASSA-PKCS1-v1_5",
                &key,
                &signature,
                &assertion.signing_input,
            )
            .map_err(ApiError::internal_js)?,
    )
    .await
    .map_err(ApiError::internal_js)?
    .as_bool()
    .unwrap_or(false);
    if !verified {
        return Err(ApiError::unauthorized(
            "Cloudflare Access signature is invalid",
        ));
    }
    Ok(())
}

async fn upsert_principal(db: &D1Database, principal: &PrincipalContext) -> Result<(), ApiError> {
    let role = match principal.role() {
        AccessRole::Member => "member",
        AccessRole::Operator => "operator",
    };
    db.prepare(
        "INSERT INTO principals \
         (principal_id, access_subject, email, role, last_seen_at) \
         VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP) \
         ON CONFLICT(access_subject) DO UPDATE SET \
         email=excluded.email, role=excluded.role, last_seen_at=CURRENT_TIMESTAMP",
    )
    .bind(&[
        JsValue::from_str(&principal.principal_hex()),
        // Store only the stable principal in this first-party row. Access sub
        // is separately protected by Access and is not returned to clients.
        JsValue::from_str(&format!("verified:{}", principal.principal_hex())),
        JsValue::from_str(principal.email()),
        JsValue::from_str(role),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn active_projection(
    db: &D1Database,
    principal: &PrincipalContext,
) -> Result<Option<ProjectionRow>, ApiError> {
    db.prepare(
        "SELECT pc.projection_id, pc.game_build_id, pc.dataset_version, pc.projected_at \
         FROM owner_bindings ob \
         JOIN projection_commits pc \
           ON pc.owner_subject = ob.owner_subject AND pc.world_id = ob.world_id \
         WHERE ob.principal_id=?1 AND ob.active=1 AND pc.is_active=1 \
         ORDER BY pc.activated_at DESC LIMIT 1",
    )
    .bind(&[JsValue::from_str(&principal.principal_hex())])
    .map_err(ApiError::internal)?
    .first(None)
    .await
    .map_err(ApiError::internal)
}

async fn me_response(db: &D1Database, principal: &PrincipalContext) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OwnProfile)
        .map_err(|_| ApiError::forbidden("own profile access denied"))?;
    let projection = active_projection(db, principal).await?;
    let profile = if let Some(ref projection) = projection {
        db.prepare(
            "SELECT character_name, level, experience, hp, stamina, attack, defense, \
             work_speed, carry_weight, gold, last_save_at \
             FROM character_profiles WHERE projection_id=?1",
        )
        .bind(&[JsValue::from_str(&projection.projection_id)])
        .map_err(ApiError::internal)?
        .first::<ProfileRow>(None)
        .await
        .map_err(ApiError::internal)?
    } else {
        None
    };
    json_response(&json!({
        "principal": {
            "email": principal.email(),
            "role": match principal.role() {
                AccessRole::Member => "member",
                AccessRole::Operator => "operator",
            }
        },
        "projection": projection.map(|row| json!({
            "game_build_id": row.game_build_id,
            "dataset_version": row.dataset_version,
            "projected_at": row.projected_at,
        })),
        "profile": profile,
        "freshness": if profile.is_some() { "save_snapshot" } else { "not_synced" },
        "privacy": {
            "scope": "own_only",
            "friend_sharing": false,
            "raw_save_uploaded": false
        }
    }))
}

async fn load_owned_pals(
    db: &D1Database,
    projection: &ProjectionRow,
) -> Result<Vec<OwnedPalRow>, ApiError> {
    db.prepare(
        "SELECT op.pal_instance_id, op.species_id, \
         COALESCE(NULLIF(TRIM(cs.name_ko), ''), '한국어 원문 없음') AS species_name, \
         op.nickname, op.level, \
         op.gender, op.rank, op.iv_hp, op.iv_attack, op.iv_defense, \
         op.location_kind, op.location_label, \
         COALESCE(GROUP_CONCAT(opp.passive_id, ','), '') AS passive_ids, \
         COALESCE(GROUP_CONCAT(\
           COALESCE(NULLIF(TRIM(cp.name_ko), ''), '한국어 원문 없음'), ','), '') \
           AS passive_names, \
         COALESCE(cs.work_json, '{}') AS work_json \
         FROM owned_pals op \
         LEFT JOIN catalog_species cs \
           ON cs.dataset_version=?2 AND cs.species_id=op.species_id \
         LEFT JOIN owned_pal_passives opp \
           ON opp.projection_id=op.projection_id AND opp.pal_instance_id=op.pal_instance_id \
         LEFT JOIN catalog_passives cp \
           ON cp.dataset_version=?2 AND cp.passive_id=opp.passive_id \
         WHERE op.projection_id=?1 \
         GROUP BY op.projection_id, op.pal_instance_id \
         ORDER BY op.level DESC, species_name, op.pal_instance_id \
         LIMIT 10000",
    )
    .bind(&[
        JsValue::from_str(&projection.projection_id),
        JsValue::from_str(&projection.dataset_version),
    ])
    .map_err(ApiError::internal)?
    .all()
    .await
    .map_err(ApiError::internal)?
    .results::<OwnedPalRow>()
    .map_err(ApiError::internal)
}

async fn pals_response(
    db: &D1Database,
    principal: &PrincipalContext,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OwnPals)
        .map_err(|_| ApiError::forbidden("own Pals access denied"))?;
    let Some(projection) = active_projection(db, principal).await? else {
        return json_response(&json!({
            "items": [],
            "count": 0,
            "sync": "not_bound"
        }));
    };
    let pals = load_owned_pals(db, &projection).await?;
    json_response(&json!({
        "items": pals,
        "count": pals.len(),
        "game_build_id": projection.game_build_id,
        "dataset_version": projection.dataset_version,
        "projected_at": projection.projected_at
    }))
}

async fn catalog_status_response(req: &Request, env: &Env) -> Result<Response, ApiError> {
    let mut manifest_url = req.url().map_err(ApiError::internal)?;
    manifest_url.set_path(PUBLIC_CATALOG_MANIFEST_PATH);
    manifest_url.set_query(None);
    manifest_url.set_fragment(None);

    let assets = env.assets("ASSETS").map_err(ApiError::internal)?;
    let mut response = assets
        .fetch(manifest_url.to_string(), None)
        .await
        .map_err(ApiError::internal)?;
    if response.status_code() != 200 {
        return Err(ApiError::unavailable(
            "Verified public catalog manifest is unavailable",
        ));
    }
    let manifest = response
        .json::<PublicCatalogManifest>()
        .await
        .map_err(ApiError::internal)?;
    manifest.validate().map_err(ApiError::unavailable)?;
    json_response(&manifest.into_status_json())
}

async fn catalog_search_response(
    req: &mut Request,
    db: &D1Database,
    principal: Option<&PrincipalContext>,
) -> Result<Response, ApiError> {
    if let Some(principal) = principal {
        principal
            .authorize(ProtectedCapability::OwnPals)
            .map_err(|_| ApiError::forbidden("catalog search access denied"))?;
    }
    let input: CatalogSearchApiRequest = read_json_body(req, MAX_BODY_BYTES).await?;
    let query = input.query.trim();
    if query.chars().count() > 80 {
        return Err(ApiError::bad_request(
            "catalog search query must be at most 80 characters",
        ));
    }
    let limit = input
        .limit
        .clamp(1, if principal.is_none() { 512 } else { 30 });
    let projection = if let Some(principal) = principal {
        active_projection(db, principal).await?
    } else {
        None
    };
    let (dataset_version, game_build_id, projection_id) = if let Some(projection) = projection {
        (
            projection.dataset_version,
            projection.game_build_id,
            projection.projection_id,
        )
    } else {
        let active_dataset = db
            .prepare(
                "SELECT dataset_version, game_build_id, activated_at \
                     FROM data_versions WHERE dataset_scope='full_catalog' \
                       AND verified=1 AND activated_at IS NOT NULL \
                     ORDER BY activated_at DESC LIMIT 1",
            )
            .first::<ActiveDatasetRow>(None)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::conflict("No active catalog dataset is available"))?;
        (
            active_dataset.dataset_version,
            active_dataset.game_build_id,
            "__no_personal_projection__".to_owned(),
        )
    };
    let pattern = catalog_like_pattern(query);
    let work_pattern = catalog_work_like_pattern(query);
    match input.kind.as_str() {
        "species" => {
            let rows = db
                .prepare(
                    "SELECT cs.species_id, cs.name_ko, cs.name_en, \
                     cs.paldex_number, cs.rarity, \
                     COALESCE(GROUP_CONCAT(DISTINCT ca.alias), '') AS aliases, \
                     cs.element_json, cs.work_json, \
                     cs.base_hp AS hp, cs.base_attack AS attack, \
                     cs.base_defense AS defense, cs.run_speed, cs.ride_sprint_speed, \
                     cs.transport_speed, cs.ride_stamina AS stamina, cs.food_amount, \
                     cs.nocturnal, cs.learned_skills_json, cs.guaranteed_passives_json, \
                     (SELECT COUNT(*) FROM owned_pals op \
                       WHERE op.projection_id=?2 AND op.species_id=cs.species_id) \
                       AS owned_count \
                     FROM catalog_species cs \
                     LEFT JOIN catalog_aliases ca \
                       ON ca.dataset_version=cs.dataset_version \
                      AND ca.entity_kind='species' AND ca.entity_id=cs.species_id \
                     WHERE cs.dataset_version=?1 AND (\
                       ?3='' OR cs.name_ko LIKE ?4 ESCAPE '\\' OR \
                       cs.name_en LIKE ?4 ESCAPE '\\' COLLATE NOCASE OR \
                       cs.species_id LIKE ?4 ESCAPE '\\' COLLATE NOCASE OR \
                       cs.work_json LIKE ?4 ESCAPE '\\' OR \
                       cs.work_json LIKE ?5 ESCAPE '\\' COLLATE NOCASE OR \
                       ca.alias LIKE ?4 ESCAPE '\\' COLLATE NOCASE) \
                     GROUP BY cs.dataset_version, cs.species_id \
                     ORDER BY CASE \
                       WHEN cs.name_ko=?3 THEN 0 \
                       WHEN cs.name_en=?3 COLLATE NOCASE THEN 0 \
                       WHEN cs.species_id=?3 COLLATE NOCASE THEN 0 \
                       WHEN cs.name_ko LIKE (?3 || '%') THEN 1 \
                       WHEN cs.name_en LIKE (?3 || '%') COLLATE NOCASE THEN 1 \
                       ELSE 2 END, \
                       owned_count DESC, cs.name_ko \
                     LIMIT ?6",
                )
                .bind(&[
                    JsValue::from_str(&dataset_version),
                    JsValue::from_str(&projection_id),
                    JsValue::from_str(query),
                    JsValue::from_str(&pattern),
                    JsValue::from_str(&work_pattern),
                    JsValue::from_f64(f64::from(limit)),
                ])
                .map_err(ApiError::internal)?
                .all()
                .await
                .map_err(ApiError::internal)?
                .results::<CatalogSpeciesSearchRow>()
                .map_err(ApiError::internal)?;
            let items = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "species_id": row.species_id,
                        "name_ko": row.name_ko,
                        "name_en": row.name_en,
                        "paldex_no": row.paldex_number,
                        "rarity": row.rarity,
                        "aliases": split_ids(&row.aliases),
                        "elements": parse_json_or_default(&row.element_json, json!([])),
                        "work_suitabilities": parse_json_or_default(&row.work_json, json!({})),
                        "hp": row.hp,
                        "attack": row.attack,
                        "defense": row.defense,
                        "run_speed": row.run_speed,
                        "ride_sprint_speed": row.ride_sprint_speed,
                        "transport_speed": row.transport_speed,
                        "stamina": row.stamina,
                        "food_amount": row.food_amount,
                        "nocturnal": row.nocturnal != 0,
                        "learned_skills": parse_json_or_default(&row.learned_skills_json, json!([])),
                        "guaranteed_passives": parse_json_or_default(
                            &row.guaranteed_passives_json,
                            json!([])
                        ),
                        "owned_count": row.owned_count
                    })
                })
                .collect::<Vec<_>>();
            json_response(&json!({
                "kind": "species",
                "query": query,
                "items": items,
                "count": items.len(),
                "dataset_version": dataset_version,
                "game_build_id": game_build_id
            }))
        }
        "active_skill" => {
            let rows = db
                .prepare(
                    "SELECT skill.skill_id, skill.name_ko, skill.description_ko, \
                     skill.element, skill.skill_kind, skill.power, skill.min_range, \
                     skill.max_range, skill.cool_time, skill.effects_json, \
                     COALESCE(GROUP_CONCAT(DISTINCT alias.alias), '') AS aliases \
                     FROM catalog_active_skills skill \
                     LEFT JOIN catalog_aliases alias \
                       ON alias.dataset_version=skill.dataset_version \
                      AND alias.entity_kind='active_skill' AND alias.entity_id=skill.skill_id \
                     WHERE skill.dataset_version=?1 AND (\
                       ?2='' OR skill.name_ko LIKE ?3 ESCAPE '\\' OR \
                       skill.skill_id LIKE ?3 ESCAPE '\\' COLLATE NOCASE OR \
                       skill.description_ko LIKE ?3 ESCAPE '\\' OR \
                       skill.element LIKE ?3 ESCAPE '\\' COLLATE NOCASE OR \
                       alias.alias LIKE ?3 ESCAPE '\\' COLLATE NOCASE) \
                     GROUP BY skill.dataset_version, skill.skill_id \
                     ORDER BY CASE \
                       WHEN skill.name_ko=?2 THEN 0 \
                       WHEN skill.skill_id=?2 COLLATE NOCASE THEN 0 \
                       WHEN skill.name_ko LIKE (?2 || '%') THEN 1 \
                       ELSE 2 END, \
                       skill.power DESC, skill.name_ko \
                     LIMIT ?4",
                )
                .bind(&[
                    JsValue::from_str(&dataset_version),
                    JsValue::from_str(query),
                    JsValue::from_str(&pattern),
                    JsValue::from_f64(f64::from(limit)),
                ])
                .map_err(ApiError::internal)?
                .all()
                .await
                .map_err(ApiError::internal)?
                .results::<CatalogActiveSkillSearchRow>()
                .map_err(ApiError::internal)?;
            let items = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "skill_id": row.skill_id,
                        "name_ko": row.name_ko,
                        "description_ko": row.description_ko,
                        "element": row.element,
                        "skill_kind": row.skill_kind,
                        "power": row.power,
                        "min_range": row.min_range,
                        "max_range": row.max_range,
                        "cool_time": row.cool_time,
                        "effects": parse_json_or_default(&row.effects_json, json!([])),
                        "aliases": split_ids(&row.aliases)
                    })
                })
                .collect::<Vec<_>>();
            json_response(&json!({
                "kind": "active_skill",
                "query": query,
                "items": items,
                "count": items.len(),
                "dataset_version": dataset_version,
                "game_build_id": game_build_id
            }))
        }
        "passive" => {
            let rows = db
                .prepare(
                    "SELECT cp.passive_id, cp.name_ko, cp.description_ko, \
                     COALESCE(GROUP_CONCAT(DISTINCT ca.alias), '') AS aliases, \
                     (SELECT COUNT(DISTINCT opp.pal_instance_id) \
                        FROM owned_pal_passives opp \
                       WHERE opp.projection_id=?2 AND opp.passive_id=cp.passive_id) \
                       AS owned_donor_count \
                     FROM catalog_passives cp \
                     LEFT JOIN catalog_aliases ca \
                       ON ca.dataset_version=cp.dataset_version \
                      AND ca.entity_kind='passive' AND ca.entity_id=cp.passive_id \
                     WHERE cp.dataset_version=?1 AND (\
                       ?3='' OR cp.name_ko LIKE ?4 ESCAPE '\\' OR \
                       cp.passive_id LIKE ?4 ESCAPE '\\' COLLATE NOCASE OR \
                       cp.description_ko LIKE ?4 ESCAPE '\\' OR \
                       ca.alias LIKE ?4 ESCAPE '\\' COLLATE NOCASE) \
                     GROUP BY cp.dataset_version, cp.passive_id \
                     ORDER BY CASE \
                       WHEN cp.name_ko=?3 THEN 0 \
                       WHEN cp.passive_id=?3 COLLATE NOCASE THEN 0 \
                       WHEN cp.name_ko LIKE (?3 || '%') THEN 1 \
                       ELSE 2 END, \
                       owned_donor_count DESC, cp.name_ko \
                     LIMIT ?5",
                )
                .bind(&[
                    JsValue::from_str(&dataset_version),
                    JsValue::from_str(&projection_id),
                    JsValue::from_str(query),
                    JsValue::from_str(&pattern),
                    JsValue::from_f64(f64::from(limit)),
                ])
                .map_err(ApiError::internal)?
                .all()
                .await
                .map_err(ApiError::internal)?
                .results::<CatalogPassiveSearchRow>()
                .map_err(ApiError::internal)?;
            let items = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "passive_id": row.passive_id,
                        "name_ko": row.name_ko,
                        "description_ko": row.description_ko,
                        "aliases": split_ids(&row.aliases),
                        "owned_donor_count": row.owned_donor_count
                    })
                })
                .collect::<Vec<_>>();
            json_response(&json!({
                "kind": "passive",
                "query": query,
                "items": items,
                "count": items.len(),
                "dataset_version": dataset_version,
                "game_build_id": game_build_id
            }))
        }
        "item" => {
            // Remote item search includes recipes and drops, so keep the
            // response intentionally smaller than the lightweight species
            // index. The web client performs debounced server-side searches.
            let item_limit = limit.min(60);
            let rows = db
                .prepare(
                    "SELECT ci.item_id, ci.name_ko, ci.description_ko, ci.category, \
                     ci.subcategory, ci.price, ci.weight_milli, ci.maximum_stack_count, \
                     ci.rarity, ci.rank, ci.icon_name, ci.legal_in_game, \
                     ci.localization_fallback, \
                     COALESCE(GROUP_CONCAT(DISTINCT ca.alias), '') AS aliases, \
                     (SELECT COUNT(*) FROM catalog_recipes cr \
                       WHERE cr.dataset_version=ci.dataset_version \
                         AND cr.output_item_id=ci.item_id) AS recipe_count, \
                     (SELECT COUNT(*) FROM acquisition_methods am \
                       WHERE am.dataset_version=ci.dataset_version \
                         AND am.item_id=ci.item_id AND am.method_kind='pal_drop') \
                       AS pal_drop_count, \
                     COALESCE((SELECT json_group_array(json_object(\
                         'recipe_id', cr.recipe_id, \
                         'output_quantity', cr.output_quantity, \
                         'ingredients', json(cr.ingredients_json), \
                         'work_amount', cr.work_amount, \
                         'unlock_item_id', cr.unlock_item_id)) \
                       FROM catalog_recipes cr \
                       WHERE cr.dataset_version=ci.dataset_version \
                         AND cr.output_item_id=ci.item_id), '[]') AS recipes_json, \
                     COALESCE((SELECT json_group_array(json_object(\
                         'method_id', am.method_id, \
                         'pal_id', am.source_entity_id, \
                         'pal_name_ko', cs.name_ko, \
                         'level', json_extract(am.requirements_json, '$.level'), \
                         'minimum_quantity', CAST(am.quantity_min AS INTEGER), \
                         'maximum_quantity', CAST(am.quantity_max AS INTEGER), \
                         'probability_ppm', CAST(ROUND(am.probability * 1000000) AS INTEGER)) \
                       FROM acquisition_methods am \
                       LEFT JOIN catalog_species cs \
                         ON cs.dataset_version=am.dataset_version \
                        AND cs.species_id=am.source_entity_id \
                       WHERE am.dataset_version=ci.dataset_version \
                         AND am.item_id=ci.item_id AND am.method_kind='pal_drop'), '[]') \
                       AS pal_drops_json \
                     FROM catalog_items ci \
                     LEFT JOIN catalog_aliases ca \
                       ON ca.dataset_version=ci.dataset_version \
                      AND ca.entity_kind='item' AND ca.entity_id=ci.item_id \
                      WHERE ci.dataset_version=?1 AND ci.localization_fallback=0 AND (\
                        (?2='' AND ci.legal_in_game=1) OR \
                       ci.name_ko LIKE ?3 ESCAPE '\\' OR \
                       ci.item_id LIKE ?3 ESCAPE '\\' COLLATE NOCASE OR \
                       ci.description_ko LIKE ?3 ESCAPE '\\' OR \
                       ci.category LIKE ?3 ESCAPE '\\' COLLATE NOCASE OR \
                       ci.subcategory LIKE ?3 ESCAPE '\\' COLLATE NOCASE OR \
                       ca.alias LIKE ?3 ESCAPE '\\' COLLATE NOCASE) \
                     GROUP BY ci.dataset_version, ci.item_id \
                     ORDER BY CASE \
                       WHEN ci.name_ko=?2 THEN 0 \
                       WHEN ci.item_id=?2 COLLATE NOCASE THEN 0 \
                       WHEN ci.name_ko LIKE (?2 || '%') THEN 1 \
                       ELSE 2 END, \
                       ci.legal_in_game DESC, ci.name_ko \
                     LIMIT ?4",
                )
                .bind(&[
                    JsValue::from_str(&dataset_version),
                    JsValue::from_str(query),
                    JsValue::from_str(&pattern),
                    JsValue::from_f64(f64::from(item_limit)),
                ])
                .map_err(ApiError::internal)?
                .all()
                .await
                .map_err(ApiError::internal)?
                .results::<CatalogItemSearchRow>()
                .map_err(ApiError::internal)?;
            let items = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "item_id": row.item_id,
                        "name_ko": row.name_ko,
                        "description_ko": row.description_ko,
                        "category": row.category,
                        "subcategory": row.subcategory,
                        "price": row.price,
                        "weight_milli": row.weight_milli,
                        "maximum_stack_count": row.maximum_stack_count,
                        "rarity": row.rarity,
                        "rank": row.rank,
                        "icon_name": row.icon_name,
                        "legal_in_game": row.legal_in_game != 0,
                        "localization_fallback": row.localization_fallback != 0,
                        "aliases": split_ids(&row.aliases),
                        "recipe_count": row.recipe_count,
                        "pal_drop_count": row.pal_drop_count,
                        "recipes": parse_json_or_default(&row.recipes_json, json!([])),
                        "pal_drops": parse_json_or_default(&row.pal_drops_json, json!([]))
                    })
                })
                .collect::<Vec<_>>();
            json_response(&json!({
                "kind": "item",
                "query": query,
                "items": items,
                "count": items.len(),
                "limit": item_limit,
                "has_more": items.len() >= item_limit as usize,
                "dataset_version": dataset_version,
                "game_build_id": game_build_id
            }))
        }
        _ => Err(ApiError::bad_request(
            "catalog search kind must be species, active_skill, passive or item",
        )),
    }
}

async fn load_inventory(
    db: &D1Database,
    projection: &ProjectionRow,
) -> Result<Vec<InventorySummaryRow>, ApiError> {
    db.prepare(
        "SELECT slots.item_id, \
         COALESCE(NULLIF(TRIM(items.name_ko), ''), '한국어 원문 없음') AS item_name, \
         SUM(slots.quantity) AS total_quantity, \
         COUNT(DISTINCT slots.container_kind || ':' || slots.container_ordinal) AS container_count \
         FROM inventory_slots slots \
         LEFT JOIN catalog_items items \
           ON items.dataset_version=?2 AND items.item_id=slots.item_id \
          AND items.localization_fallback=0 \
         WHERE slots.projection_id=?1 \
         GROUP BY slots.item_id, item_name \
         ORDER BY total_quantity DESC, item_name \
         LIMIT 10000",
    )
    .bind(&[
        JsValue::from_str(&projection.projection_id),
        JsValue::from_str(&projection.dataset_version),
    ])
    .map_err(ApiError::internal)?
    .all()
    .await
    .map_err(ApiError::internal)?
    .results::<InventorySummaryRow>()
    .map_err(ApiError::internal)
}

async fn inventory_response(
    db: &D1Database,
    principal: &PrincipalContext,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OwnInventory)
        .map_err(|_| ApiError::forbidden("own inventory access denied"))?;
    let Some(projection) = active_projection(db, principal).await? else {
        return json_response(&json!({
            "items": [],
            "count": 0,
            "sync": "not_bound"
        }));
    };
    let items = load_inventory(db, &projection).await?;
    json_response(&json!({
        "items": items,
        "count": items.len(),
        "game_build_id": projection.game_build_id,
        "dataset_version": projection.dataset_version,
        "projected_at": projection.projected_at
    }))
}

async fn breeding_response(
    req: &mut Request,
    db: &D1Database,
    principal: &PrincipalContext,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OwnBreeding)
        .map_err(|_| ApiError::forbidden("own breeding access denied"))?;
    let input: BreedingApiRequest = read_json_body(req, MAX_BODY_BYTES).await?;
    let Some(projection) = active_projection(db, principal).await? else {
        return Err(ApiError::conflict("No active personal projection is bound"));
    };
    let target_species_id = resolve_catalog_entity(
        db,
        &projection.dataset_version,
        "species",
        input.target_species_id.as_deref(),
        input.target_species_query.as_deref(),
    )
    .await?;
    let mut required_passive_ids = Vec::new();
    for value in &input.required_passive_ids {
        required_passive_ids.push(
            resolve_catalog_entity(
                db,
                &projection.dataset_version,
                "passive",
                Some(value),
                None,
            )
            .await?,
        );
    }
    for value in &input.required_passive_queries {
        required_passive_ids.push(
            resolve_catalog_entity(
                db,
                &projection.dataset_version,
                "passive",
                None,
                Some(value),
            )
            .await?,
        );
    }
    required_passive_ids.sort();
    required_passive_ids.dedup();
    if required_passive_ids.len() > 8 {
        return Err(ApiError::bad_request(
            "at most 8 required passive traits are allowed",
        ));
    }

    let pals = load_owned_pals(db, &projection).await?;
    let owned = pals
        .iter()
        .map(|pal| OwnedPalSeed {
            instance_id_hex: pal.pal_instance_id.clone(),
            species_id: pal.species_id.clone(),
            passive_ids: split_ids(&pal.passive_ids),
        })
        .collect::<Vec<_>>();
    let rule_rows = db
        .prepare(
            "SELECT parent_a_species_id, parent_b_species_id, child_species_id \
             FROM breeding_rules WHERE dataset_version=?1 \
             ORDER BY child_species_id, parent_a_species_id, parent_b_species_id",
        )
        .bind(&[JsValue::from_str(&projection.dataset_version)])
        .map_err(ApiError::internal)?
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<BreedingRuleRow>()
        .map_err(ApiError::internal)?;
    let rules = rule_rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| BreedingRule {
            rule_id: format!("{}:{index}", projection.dataset_version),
            parent_a_species_id: row.parent_a_species_id,
            parent_b_species_id: row.parent_b_species_id,
            child_species_id: row.child_species_id,
        })
        .collect::<Vec<_>>();
    let plan = solve_owned_breeding(
        &owned,
        &rules,
        &target_species_id,
        &required_passive_ids,
        input.maximum_generations,
        2_000_000,
    )
    .map_err(|error| ApiError::unprocessable(error.to_string()))?;
    let species_names = load_catalog_names(db, &projection.dataset_version, "species").await?;
    let passive_names = load_catalog_names(db, &projection.dataset_version, "passive").await?;
    json_response(&json!({
        "plan": plan,
        "species_names": species_names,
        "passive_names": passive_names,
        "game_build_id": projection.game_build_id,
        "dataset_version": projection.dataset_version,
        "quality": "exact_reachability",
        "passive_probability": "unknown"
    }))
}

async fn assistant_response(
    req: &mut Request,
    env: &Env,
    db: &D1Database,
    principal: &PrincipalContext,
    request_id: &str,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OwnAssistant)
        .map_err(|_| ApiError::forbidden("own assistant access denied"))?;
    let input: AssistantApiRequest = read_json_body(req, MAX_GROUNDED_ASSISTANT_BODY_BYTES).await?;
    let Some(projection) = active_projection(db, principal).await? else {
        return Err(ApiError::conflict("No active personal projection is bound"));
    };
    let retrieval_plan = KnowledgeRetrievalPlan::for_korean_question(&input.question);
    let use_personal_context = knowledge::question_uses_personal_context(&input.question);
    let mut evidence = Vec::new();
    if use_personal_context {
        if let Some(profile) = db
            .prepare(
                "SELECT character_name, level, experience, hp, stamina, attack, defense, \
                 work_speed, carry_weight, gold, last_save_at \
                 FROM character_profiles WHERE projection_id=?1",
            )
            .bind(&[JsValue::from_str(&projection.projection_id)])
            .map_err(ApiError::internal)?
            .first::<ProfileRow>(None)
            .await
            .map_err(ApiError::internal)?
        {
            evidence.push(AssistantEvidence {
                evidence_id: "character:active".to_owned(),
                kind: AssistantEvidenceKind::Character,
                title: format!("{} 캐릭터", profile.character_name),
                detail: format!(
                    "레벨 {}, 마지막 세이브 {}",
                    profile.level, profile.last_save_at
                ),
                quality: "exact".to_owned(),
                source_ids: vec!["projection:active".to_owned()],
            });
        }
        let inventory = load_inventory(db, &projection).await?;
        for item in inventory.into_iter().take(8) {
            evidence.push(AssistantEvidence {
                evidence_id: format!("inventory:{}", item.item_id),
                kind: AssistantEvidenceKind::Inventory,
                title: item.item_name,
                detail: format!(
                    "현재 수량 {}, 보관 컨테이너 {}개",
                    item.total_quantity, item.container_count
                ),
                quality: "exact".to_owned(),
                source_ids: vec!["projection:active".to_owned()],
            });
        }
        let pals = load_owned_pals(db, &projection).await?;
        for pal in pals.into_iter().take(12) {
            evidence.push(AssistantEvidence {
                evidence_id: format!("owned-pal:{}", pal.pal_instance_id),
                kind: AssistantEvidenceKind::OwnedPal,
                title: pal.species_name,
                detail: format!(
                    "레벨 {}, IV HP/공격/방어 {}/{}/{}, 패시브 {}",
                    pal.level,
                    optional_number(pal.iv_hp),
                    optional_number(pal.iv_attack),
                    optional_number(pal.iv_defense),
                    if pal.passive_names.is_empty() {
                        "없음"
                    } else {
                        &pal.passive_names
                    }
                ),
                quality: "exact".to_owned(),
                source_ids: vec!["projection:active".to_owned()],
            });
        }
    }
    let mut retrieved = knowledge::retrieve_for_assistant(
        db,
        &projection.dataset_version,
        &input.question,
        &retrieval_plan,
    )
    .await?;
    let remaining_evidence =
        pal_companion_service::MAX_EVIDENCE_ROWS.saturating_sub(evidence.len());
    retrieved.evidence.truncate(remaining_evidence);
    retrieved.metrics.context_row_count = retrieved.evidence.len();
    retrieved.metrics.context_bytes = retrieved
        .evidence
        .iter()
        .map(|row| row.title.len() + row.detail.len())
        .sum();
    evidence.extend(retrieved.evidence);
    let manifest_sha256 = knowledge::manifest_sha256(db, &projection.dataset_version).await?;

    let grounded = GroundedAssistantRequest {
        question: input.question,
        game_build_id: projection.game_build_id.clone(),
        dataset_manifest_id_hex: manifest_sha256,
        projection_id_hex: digest_like_id(&projection.projection_id),
        evidence,
    };
    let (response, model) = execute_grounded_assistant(env, &grounded).await?;
    record_assistant_run(
        db,
        principal,
        request_id,
        &projection.dataset_version,
        &projection.game_build_id,
        &model,
        &response,
    )
    .await?;
    let _ = knowledge::record_retrieval(
        db,
        &format!("{request_id}:knowledge"),
        request_id,
        &projection.dataset_version,
        &retrieved.metrics,
    )
    .await;
    json_response(&response)
}

async fn grounded_assistant_response(
    req: &mut Request,
    env: &Env,
    db: &D1Database,
    principal: &PrincipalContext,
    request_id: &str,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OwnAssistant)
        .map_err(|_| ApiError::forbidden("own assistant access denied"))?;
    let body = read_bounded_body(req, MAX_GROUNDED_ASSISTANT_BODY_BYTES).await?;
    if body.is_empty() {
        return Err(ApiError::bad_request(
            "grounded assistant request size is invalid",
        ));
    }
    let grounded: GroundedAssistantRequest =
        serde_json::from_slice(&body).map_err(ApiError::bad_request)?;
    grounded
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    require_known_compatible_build(db, &grounded.game_build_id).await?;

    let (response, model) = execute_grounded_assistant(env, &grounded).await?;
    let dataset_version = format!(
        "local:{}",
        grounded
            .dataset_manifest_id_hex
            .get(..16)
            .unwrap_or(&grounded.dataset_manifest_id_hex)
    );
    record_assistant_run(
        db,
        principal,
        request_id,
        &dataset_version,
        &grounded.game_build_id,
        &model,
        &response,
    )
    .await?;
    json_response(&response)
}

async fn execute_grounded_assistant(
    env: &Env,
    grounded: &GroundedAssistantRequest,
) -> Result<(GroundedAssistantResponse, String), ApiError> {
    let prompt = grounded
        .prompt()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let model = required_var(env, "AI_MODEL")?;
    let ai = env.ai("AI").map_err(ApiError::internal)?;
    let ai_result = ai
        .run::<_, AiOutput>(
            &model,
            AiInput {
                messages: vec![
                    AiInputMessage {
                        role: "system",
                        content: &prompt.system,
                    },
                    AiInputMessage {
                        role: "user",
                        content: &prompt.user,
                    },
                ],
                max_tokens: 900,
                temperature: 0.1,
            },
        )
        .await;
    let response = match ai_result {
        Ok(output) if !output.response.trim().is_empty() => GroundedAssistantResponse {
            answer: output.response,
            evidence: grounded.evidence.clone(),
            model_used: true,
            warnings: Vec::new(),
        },
        Ok(_) | Err(_) => {
            let fallback = grounded
                .fallback()
                .map_err(|error| ApiError::internal(error.to_string()))?;
            GroundedAssistantResponse {
                answer: fallback.answer,
                evidence: grounded.evidence.clone(),
                model_used: false,
                warnings: vec!["AI_MODEL_UNAVAILABLE".to_owned()],
            }
        }
    };
    Ok((response, model))
}

async fn record_assistant_run(
    db: &D1Database,
    principal: &PrincipalContext,
    request_id: &str,
    dataset_version: &str,
    game_build_id: &str,
    model: &str,
    response: &GroundedAssistantResponse,
) -> Result<(), ApiError> {
    db.prepare(
        "INSERT INTO assistant_runs \
         (run_id, principal_id, dataset_version, game_build_id, evidence_count, \
          quality, model_id, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&[
        JsValue::from_str(request_id),
        JsValue::from_str(&principal.principal_hex()),
        JsValue::from_str(dataset_version),
        JsValue::from_str(game_build_id),
        JsValue::from_f64(response.evidence.len() as f64),
        JsValue::from_str(assistant_quality(&response.evidence)),
        JsValue::from_str(model),
        JsValue::from_str(if response.model_used {
            "answered"
        } else {
            "fallback"
        }),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct GameBuildPolicyLookupRow {
    compatibility_state: String,
    dataset_version: Option<String>,
}

fn canonical_game_build_id(value: &str) -> Result<String, ApiError> {
    normalize_game_build_id(value).ok_or_else(|| ApiError::bad_request("game build id is invalid"))
}

async fn load_game_build_policy(
    db: &D1Database,
    game_build_id: &str,
) -> Result<Option<GameBuildPolicyLookupRow>, ApiError> {
    db.prepare(
        "SELECT compatibility_state, dataset_version \
         FROM game_build_policies WHERE game_build_id=?1 LIMIT 1",
    )
    .bind(&[JsValue::from_str(game_build_id)])
    .map_err(ApiError::internal)?
    .first(None)
    .await
    .map_err(ApiError::internal)
}

async fn observe_projection_build(
    db: &D1Database,
    game_build_id: &str,
    parser_version: &str,
    requested_dataset_version: &str,
) -> Result<(), ApiError> {
    db.prepare(
        "INSERT INTO game_build_policies \
         (game_build_id, compatibility_state, projection_schema, observation_count, \
          last_parser_version, last_requested_dataset_version) \
         VALUES (?1, 'observed', 'cloud-profile-projection-v1', 1, ?2, ?3) \
         ON CONFLICT(game_build_id) DO UPDATE SET \
           observation_count=game_build_policies.observation_count+1, \
           last_parser_version=excluded.last_parser_version, \
           last_requested_dataset_version=excluded.last_requested_dataset_version, \
           last_seen_at=CURRENT_TIMESTAMP, updated_at=CURRENT_TIMESTAMP",
    )
    .bind(&[
        JsValue::from_str(game_build_id),
        JsValue::from_str(parser_version),
        JsValue::from_str(requested_dataset_version),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn require_projection_build_compatibility(
    db: &D1Database,
    game_build_id: &str,
    parser_version: &str,
    requested_dataset_version: &str,
) -> Result<String, ApiError> {
    let game_build_id = canonical_game_build_id(game_build_id)?;
    observe_projection_build(
        db,
        &game_build_id,
        parser_version,
        requested_dataset_version,
    )
    .await?;
    let policy = load_game_build_policy(db, &game_build_id).await?;
    match evaluate_build_policy(
        policy.as_ref().map(|row| row.compatibility_state.as_str()),
        policy
            .as_ref()
            .and_then(|row| row.dataset_version.as_deref()),
        requested_dataset_version,
    ) {
        BuildPolicyDecision::Compatible { dataset_version } => Ok(dataset_version.to_owned()),
        BuildPolicyDecision::ReviewRequired => Err(ApiError::conflict_with_code(
            "BUILD_REVIEW_REQUIRED",
            "This game build was recorded and is awaiting compatibility review",
        )),
        BuildPolicyDecision::Blocked => Err(ApiError::conflict_with_code(
            "BUILD_BLOCKED",
            "This game build is not compatible with personal projection sync",
        )),
        BuildPolicyDecision::DatasetMismatch {
            expected_dataset_version,
        } => Err(ApiError::conflict_with_code(
            "DATASET_VERSION_MISMATCH",
            format!(
                "Use compatible dataset {expected_dataset_version} for this personal projection"
            ),
        )),
    }
}

async fn require_known_compatible_build(
    db: &D1Database,
    game_build_id: &str,
) -> Result<String, ApiError> {
    let game_build_id = canonical_game_build_id(game_build_id)?;
    let policy = load_game_build_policy(db, &game_build_id).await?;
    match policy.as_ref().map(|row| row.compatibility_state.as_str()) {
        Some("compatible") => policy
            .and_then(|row| row.dataset_version)
            .ok_or_else(|| ApiError::internal("compatible build has no dataset")),
        Some("blocked") => Err(ApiError::conflict_with_code(
            "BUILD_BLOCKED",
            "This game build is not compatible with personal evidence",
        )),
        Some("observed") | None | Some(_) => Err(ApiError::conflict_with_code(
            "BUILD_REVIEW_REQUIRED",
            "This game build is awaiting compatibility review",
        )),
    }
}

fn assistant_quality(evidence: &[AssistantEvidence]) -> &'static str {
    if evidence.is_empty() || evidence.iter().any(|row| row.quality == "unknown") {
        "unknown"
    } else if evidence.iter().any(|row| row.quality == "model") {
        "model"
    } else if evidence.iter().any(|row| row.quality == "measured") {
        "measured"
    } else {
        "exact"
    }
}

#[derive(Debug, Deserialize)]
struct KnowledgeRow {
    document_id: String,
    title: String,
    content: String,
    source_url: Option<String>,
    confidence: String,
}

async fn ops_health(db: &D1Database, principal: &PrincipalContext) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OperatorAudit)
        .map_err(|_| ApiError::forbidden("operator role required"))?;
    let principal_count = scalar_count(db, "SELECT COUNT(*) AS count FROM principals").await?;
    let bound_count = scalar_count(
        db,
        "SELECT COUNT(*) AS count FROM owner_bindings WHERE active=1",
    )
    .await?;
    let stale_agents = scalar_count(
        db,
        "SELECT COUNT(*) AS count FROM service_agents \
         WHERE revoked_at IS NULL AND \
         (last_seen_at IS NULL OR last_seen_at < datetime('now', '-5 minutes'))",
    )
    .await?;
    let active_dataset: Option<ActiveDatasetRow> = db
        .prepare(
            "SELECT dataset_version, game_build_id, activated_at \
              FROM data_versions WHERE dataset_scope='full_catalog' \
                AND verified=1 AND activated_at IS NOT NULL \
             ORDER BY activated_at DESC LIMIT 1",
        )
        .first(None)
        .await
        .map_err(ApiError::internal)?;
    json_response(&json!({
        "status": if stale_agents == 0 { "healthy" } else { "degraded" },
        "members": principal_count,
        "active_bindings": bound_count,
        "stale_agents": stale_agents,
        "active_dataset": active_dataset,
        "privacy": "operator_metadata_only"
    }))
}

#[derive(Debug, Deserialize, Serialize)]
struct ActiveDatasetRow {
    dataset_version: String,
    game_build_id: String,
    activated_at: String,
}

async fn ops_bindings(db: &D1Database, principal: &PrincipalContext) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OperatorBindings)
        .map_err(|_| ApiError::forbidden("operator role required"))?;
    let rows = db
        .prepare(
            "SELECT ob.owner_subject, ob.principal_id, p.email, ob.world_id, \
             ob.character_name, ob.active, ob.updated_at \
             FROM owner_bindings ob JOIN principals p ON p.principal_id=ob.principal_id \
             ORDER BY p.email, ob.world_id",
        )
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<BindingRow>()
        .map_err(ApiError::internal)?;
    json_response(&json!({"items": rows, "count": rows.len()}))
}

async fn ops_upsert_binding(
    req: &mut Request,
    db: &D1Database,
    principal: &PrincipalContext,
    request_id: &str,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OperatorBindings)
        .map_err(|_| ApiError::forbidden("operator role required"))?;
    let input: BindingApiRequest = read_json_body(req, MAX_BODY_BYTES).await?;
    if !valid_hex_id(&input.owner_subject, 64)
        || !valid_short_id(&input.world_id)
        || !valid_email(&input.member_email)
        || input
            .character_name
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 128)
    {
        return Err(ApiError::bad_request("binding fields are invalid"));
    }
    let email = input.member_email.trim().to_ascii_lowercase();
    let member: Option<PrincipalIdRow> = db
        .prepare("SELECT principal_id FROM principals WHERE email=?1 LIMIT 1")
        .bind(&[JsValue::from_str(&email)])
        .map_err(ApiError::internal)?
        .first(None)
        .await
        .map_err(ApiError::internal)?;
    let member = member.ok_or_else(|| {
        ApiError::not_found("The member must sign in through Cloudflare Access once before binding")
    })?;
    db.prepare(
        "INSERT INTO owner_bindings \
         (owner_subject, principal_id, world_id, character_name, active, \
          bound_by_principal_id, updated_at) \
         VALUES (?1, ?2, ?3, ?4, 1, ?5, CURRENT_TIMESTAMP) \
         ON CONFLICT(owner_subject) DO UPDATE SET \
           principal_id=excluded.principal_id, world_id=excluded.world_id, \
           character_name=excluded.character_name, active=1, \
           bound_by_principal_id=excluded.bound_by_principal_id, \
           updated_at=CURRENT_TIMESTAMP",
    )
    .bind(&[
        JsValue::from_str(&input.owner_subject),
        JsValue::from_str(&member.principal_id),
        JsValue::from_str(&input.world_id),
        input
            .character_name
            .as_deref()
            .map(JsValue::from_str)
            .unwrap_or(JsValue::NULL),
        JsValue::from_str(&principal.principal_hex()),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    audit(
        db,
        principal,
        "binding.upsert",
        "owner_binding",
        Some(&input.owner_subject),
        "success",
        request_id,
    )
    .await?;
    json_response(&json!({"ok": true, "email": email, "world_id": input.world_id}))
}

async fn ops_revoke_binding(
    req: &mut Request,
    db: &D1Database,
    principal: &PrincipalContext,
    request_id: &str,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OperatorBindings)
        .map_err(|_| ApiError::forbidden("operator role required"))?;
    let input: RevokeBindingApiRequest = read_json_body(req, MAX_BODY_BYTES).await?;
    if !valid_hex_id(&input.owner_subject, 64) {
        return Err(ApiError::bad_request("owner subject is invalid"));
    }
    db.prepare(
        "UPDATE owner_bindings SET active=0, updated_at=CURRENT_TIMESTAMP \
         WHERE owner_subject=?1",
    )
    .bind(&[JsValue::from_str(&input.owner_subject)])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    audit(
        db,
        principal,
        "binding.revoke",
        "owner_binding",
        Some(&input.owner_subject),
        "success",
        request_id,
    )
    .await?;
    json_response(&json!({"ok": true}))
}

async fn ops_game_builds(
    db: &D1Database,
    principal: &PrincipalContext,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OperatorSync)
        .map_err(|_| ApiError::forbidden("operator role required"))?;
    let rows = db
        .prepare(
            "SELECT game_build_id, compatibility_state, dataset_version, projection_schema, \
             observation_count, last_parser_version, last_requested_dataset_version, notes, \
             first_seen_at, last_seen_at, reviewed_at, updated_at \
             FROM game_build_policies \
             ORDER BY CASE compatibility_state \
               WHEN 'observed' THEN 0 WHEN 'blocked' THEN 1 ELSE 2 END, \
               last_seen_at DESC LIMIT 200",
        )
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<GameBuildPolicyRow>()
        .map_err(ApiError::internal)?;
    json_response(&json!({"items": rows, "count": rows.len()}))
}

async fn ops_upsert_game_build(
    req: &mut Request,
    db: &D1Database,
    principal: &PrincipalContext,
    request_id: &str,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OperatorSync)
        .map_err(|_| ApiError::forbidden("operator role required"))?;
    let input: GameBuildPolicyApiRequest = read_json_body(req, MAX_BODY_BYTES).await?;
    let game_build_id = canonical_game_build_id(&input.game_build_id)?;
    if !matches!(input.compatibility_state.as_str(), "compatible" | "blocked") {
        return Err(ApiError::bad_request(
            "compatibility_state must be compatible or blocked",
        ));
    }
    let notes = input
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if notes.is_some_and(|value| value.len() > 500) {
        return Err(ApiError::bad_request("game build notes are too long"));
    }
    let dataset_version = if input.compatibility_state == "compatible" {
        let dataset_version = input
            .dataset_version
            .as_deref()
            .map(str::trim)
            .filter(|value| valid_short_id(value))
            .ok_or_else(|| {
                ApiError::bad_request("a compatible build requires a valid dataset_version")
            })?;
        let dataset: Option<ActiveDatasetRow> = db
            .prepare(
                "SELECT dataset_version, game_build_id, \
                 COALESCE(activated_at, '') AS activated_at \
                 FROM data_versions WHERE dataset_version=?1 \
                   AND dataset_scope='full_catalog' AND verified=1 LIMIT 1",
            )
            .bind(&[JsValue::from_str(dataset_version)])
            .map_err(ApiError::internal)?
            .first(None)
            .await
            .map_err(ApiError::internal)?;
        if dataset.is_none() {
            return Err(ApiError::conflict_with_code(
                "DATASET_VERSION_UNAVAILABLE",
                "Only a verified dataset can be mapped to a compatible build",
            ));
        }
        Some(dataset_version)
    } else {
        None
    };

    db.prepare(
        "INSERT INTO game_build_policies \
         (game_build_id, compatibility_state, dataset_version, projection_schema, \
          notes, reviewed_by_principal_id, reviewed_at) \
         VALUES (?1, ?2, ?3, 'cloud-profile-projection-v1', ?4, ?5, CURRENT_TIMESTAMP) \
         ON CONFLICT(game_build_id) DO UPDATE SET \
           compatibility_state=excluded.compatibility_state, \
           dataset_version=excluded.dataset_version, \
           projection_schema=excluded.projection_schema, notes=excluded.notes, \
           reviewed_by_principal_id=excluded.reviewed_by_principal_id, \
           reviewed_at=CURRENT_TIMESTAMP, updated_at=CURRENT_TIMESTAMP",
    )
    .bind(&[
        JsValue::from_str(&game_build_id),
        JsValue::from_str(&input.compatibility_state),
        optional_js_str(dataset_version),
        optional_js_str(notes),
        JsValue::from_str(&principal.principal_hex()),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    audit(
        db,
        principal,
        "game_build.review",
        "game_build",
        Some(&game_build_id),
        &input.compatibility_state,
        request_id,
    )
    .await?;
    json_response(&json!({
        "ok": true,
        "game_build_id": game_build_id,
        "compatibility_state": input.compatibility_state,
        "dataset_version": dataset_version
    }))
}

async fn ops_import_projection(
    req: &mut Request,
    db: &D1Database,
    principal: &PrincipalContext,
    request_id: &str,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OperatorSync)
        .map_err(|_| ApiError::forbidden("operator role required"))?;
    let input: ProjectionImportRequest = read_json_body(req, MAX_BODY_BYTES).await?;
    validate_projection_import(&input)?;

    let binding: Option<ExistsRow> = db
        .prepare(
            "SELECT 1 AS present, character_name FROM owner_bindings \
             WHERE owner_subject=?1 AND world_id=?2 AND active=1 LIMIT 1",
        )
        .bind(&[
            JsValue::from_str(&input.owner_subject),
            JsValue::from_str(&input.world_id),
        ])
        .map_err(ApiError::internal)?
        .first(None)
        .await
        .map_err(ApiError::internal)?;
    let binding = binding.filter(|row| row.present == 1).ok_or_else(|| {
        ApiError::conflict("Bind this owner subject to an Access member before importing")
    })?;
    let character_name = binding.character_name.ok_or_else(|| {
        ApiError::conflict("Set a character name on the owner binding before importing")
    })?;
    let compatible_dataset_version = require_projection_build_compatibility(
        db,
        &input.game_build_id,
        &input.parser_version,
        &input.dataset_version,
    )
    .await?;
    let compatible_dataset: Option<ActiveDatasetRow> = db
        .prepare(
            "SELECT dataset_version, game_build_id, COALESCE(activated_at, '') AS activated_at \
             FROM data_versions \
              WHERE dataset_version=?1 AND dataset_scope='full_catalog' \
                AND verified=1 LIMIT 1",
        )
        .bind(&[JsValue::from_str(&compatible_dataset_version)])
        .map_err(ApiError::internal)?
        .first(None)
        .await
        .map_err(ApiError::internal)?;
    compatible_dataset.ok_or_else(|| {
        ApiError::conflict_with_code(
            "DATASET_VERSION_UNAVAILABLE",
            "The remotely mapped projection dataset is not verified",
        )
    })?;

    let importer_agent_id = format!("operator-import:{}", principal.principal_hex());
    db.prepare(
        "INSERT INTO service_agents \
         (agent_id, world_id, access_common_name, last_seen_at) \
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP) \
         ON CONFLICT(agent_id) DO UPDATE SET \
           world_id=excluded.world_id, last_seen_at=CURRENT_TIMESTAMP",
    )
    .bind(&[
        JsValue::from_str(&importer_agent_id),
        JsValue::from_str(&input.world_id),
        JsValue::from_str(&importer_agent_id),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    db.prepare(
        "INSERT INTO sync_runs \
         (sync_id, agent_id, world_id, game_build_id, parser_version, \
          projection_schema, status, owner_count, pal_count, inventory_slot_count) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'cloud-profile-projection-v1', \
                 'received', 1, ?6, ?7)",
    )
    .bind(&[
        JsValue::from_str(&input.sync_id),
        JsValue::from_str(&importer_agent_id),
        JsValue::from_str(&input.world_id),
        JsValue::from_str(&input.game_build_id),
        JsValue::from_str(&input.parser_version),
        JsValue::from_f64(input.pals.len() as f64),
        JsValue::from_f64(input.inventory.len() as f64),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    db.prepare(
        "INSERT INTO projection_commits \
         (projection_id, owner_subject, world_id, sync_id, game_build_id, \
          dataset_version, projected_at, is_active) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
    )
    .bind(&[
        JsValue::from_str(&input.projection_id),
        JsValue::from_str(&input.owner_subject),
        JsValue::from_str(&input.world_id),
        JsValue::from_str(&input.sync_id),
        JsValue::from_str(&input.game_build_id),
        JsValue::from_str(&input.dataset_version),
        JsValue::from_str(&input.projected_at),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    insert_import_profile(
        db,
        &input.projection_id,
        &character_name,
        &input.projected_at,
        &input.profile,
    )
    .await?;
    insert_import_pals(db, &input.projection_id, &input.pals).await?;
    insert_import_inventory(db, &input.projection_id, &input.inventory).await?;

    db.batch(vec![
        db.prepare(
            "UPDATE projection_commits SET is_active=0 \
             WHERE owner_subject=?1 AND world_id=?2 AND is_active=1",
        )
        .bind(&[
            JsValue::from_str(&input.owner_subject),
            JsValue::from_str(&input.world_id),
        ])
        .map_err(ApiError::internal)?,
        db.prepare(
            "UPDATE projection_commits SET is_active=1, activated_at=CURRENT_TIMESTAMP \
             WHERE projection_id=?1",
        )
        .bind(&[JsValue::from_str(&input.projection_id)])
        .map_err(ApiError::internal)?,
        db.prepare(
            "UPDATE sync_runs SET status='activated', completed_at=CURRENT_TIMESTAMP \
             WHERE sync_id=?1",
        )
        .bind(&[JsValue::from_str(&input.sync_id)])
        .map_err(ApiError::internal)?,
    ])
    .await
    .map_err(ApiError::internal)?;
    audit(
        db,
        principal,
        "projection.import",
        "projection",
        Some(&input.projection_id),
        "success",
        request_id,
    )
    .await?;
    json_response(&json!({
        "ok": true,
        "projection_id": input.projection_id,
        "pal_count": input.pals.len(),
        "inventory_slot_count": input.inventory.len(),
        "game_build_id": input.game_build_id,
        "dataset_version": input.dataset_version
    }))
}

#[derive(Debug, Deserialize)]
struct ExistsRow {
    present: i32,
    character_name: Option<String>,
}

async fn insert_import_profile(
    db: &D1Database,
    projection_id: &str,
    character_name: &str,
    last_save_at: &str,
    profile: &ImportedProfile,
) -> Result<(), ApiError> {
    db.prepare(
        "INSERT INTO character_profiles \
         (projection_id, character_name, level, experience, hp, stamina, attack, \
          defense, work_speed, carry_weight, gold, last_save_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )
    .bind(&[
        JsValue::from_str(projection_id),
        JsValue::from_str(character_name),
        JsValue::from_f64(profile.level as f64),
        optional_js_i32(profile.experience),
        optional_js_i32(profile.hp),
        optional_js_i32(profile.stamina),
        optional_js_i32(profile.attack),
        optional_js_i32(profile.defense),
        optional_js_i32(profile.work_speed),
        optional_js_i32(profile.carry_weight),
        JsValue::NULL,
        JsValue::from_str(last_save_at),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn insert_import_pals(
    db: &D1Database,
    projection_id: &str,
    pals: &[ImportedPal],
) -> Result<(), ApiError> {
    for chunk in pals.chunks(100) {
        let mut statements = Vec::new();
        for pal in chunk {
            let location_label = format!(
                "{} {}-{}",
                pal.location_kind, pal.container_ordinal, pal.slot_index
            );
            statements.push(
                db.prepare(
                    "INSERT INTO owned_pals \
                     (projection_id, pal_instance_id, species_id, nickname, level, \
                      gender, rank, iv_hp, iv_attack, iv_defense, location_kind, location_label) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                )
                .bind(&[
                    JsValue::from_str(projection_id),
                    JsValue::from_str(&pal.pal_instance_id),
                    JsValue::from_str(&pal.species_id),
                    JsValue::NULL,
                    JsValue::from_f64(pal.level as f64),
                    optional_js_str(pal.gender.as_deref()),
                    JsValue::from_f64(pal.rank as f64),
                    optional_js_i32(pal.iv_hp),
                    optional_js_i32(pal.iv_attack),
                    optional_js_i32(pal.iv_defense),
                    JsValue::from_str(&pal.location_kind),
                    JsValue::from_str(&location_label),
                ])
                .map_err(ApiError::internal)?,
            );
            for (position, passive_id) in pal.passive_ids.iter().enumerate() {
                statements.push(
                    db.prepare(
                        "INSERT INTO owned_pal_passives \
                         (projection_id, pal_instance_id, position, passive_id) \
                         VALUES (?1, ?2, ?3, ?4)",
                    )
                    .bind(&[
                        JsValue::from_str(projection_id),
                        JsValue::from_str(&pal.pal_instance_id),
                        JsValue::from_f64(position as f64),
                        JsValue::from_str(passive_id),
                    ])
                    .map_err(ApiError::internal)?,
                );
            }
        }
        db.batch(statements).await.map_err(ApiError::internal)?;
    }
    Ok(())
}

async fn insert_import_inventory(
    db: &D1Database,
    projection_id: &str,
    inventory: &[ImportedInventorySlot],
) -> Result<(), ApiError> {
    for chunk in inventory.chunks(200) {
        let statements = chunk
            .iter()
            .map(|slot| {
                db.prepare(
                    "INSERT INTO inventory_slots \
                     (projection_id, container_kind, container_ordinal, slot_index, item_id, quantity) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .bind(&[
                    JsValue::from_str(projection_id),
                    JsValue::from_str(&slot.container_kind),
                    JsValue::from_f64(slot.container_ordinal as f64),
                    JsValue::from_f64(slot.slot_index as f64),
                    JsValue::from_str(&slot.item_id),
                    JsValue::from_f64(slot.quantity as f64),
                ])
                .map_err(ApiError::internal)
            })
            .collect::<Result<Vec<_>, _>>()?;
        db.batch(statements).await.map_err(ApiError::internal)?;
    }
    Ok(())
}

fn validate_projection_import(input: &ProjectionImportRequest) -> Result<(), ApiError> {
    if !valid_hex_id(&input.projection_id, 64)
        || !valid_hex_id(&input.sync_id, 64)
        || !valid_hex_id(&input.owner_subject, 64)
        || !valid_short_id(&input.world_id)
        || !valid_short_id(&input.game_build_id)
        || !valid_short_id(&input.dataset_version)
        || !valid_short_id(&input.parser_version)
        || input.projected_at.len() > 64
        || input.profile.level < 0
        || input.pals.len() > MAX_IMPORT_PALS
        || input.inventory.len() > MAX_IMPORT_INVENTORY
    {
        return Err(ApiError::bad_request("projection envelope is invalid"));
    }
    for pal in &input.pals {
        if !valid_hex_id(&pal.pal_instance_id, 64)
            || !valid_short_id(&pal.species_id)
            || pal.level < 0
            || pal.rank < 0
            || !valid_short_id(&pal.location_kind)
            || pal.container_ordinal < 0
            || pal.slot_index < 0
            || pal.passive_ids.len() > 16
            || pal.passive_ids.iter().any(|value| !valid_short_id(value))
        {
            return Err(ApiError::bad_request("projection contains an invalid Pal"));
        }
    }
    for slot in &input.inventory {
        if !valid_short_id(&slot.container_kind)
            || slot.container_ordinal < 0
            || slot.slot_index < 0
            || !valid_short_id(&slot.item_id)
            || slot.quantity < 0
        {
            return Err(ApiError::bad_request(
                "projection contains an invalid inventory slot",
            ));
        }
    }
    Ok(())
}

fn optional_js_i32(value: Option<i32>) -> JsValue {
    value
        .map(|value| JsValue::from_f64(value as f64))
        .unwrap_or(JsValue::NULL)
}

fn optional_js_str(value: Option<&str>) -> JsValue {
    value.map(JsValue::from_str).unwrap_or(JsValue::NULL)
}

#[derive(Debug, Deserialize)]
struct PrincipalIdRow {
    principal_id: String,
}

async fn ops_sync_runs(
    db: &D1Database,
    principal: &PrincipalContext,
) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OperatorSync)
        .map_err(|_| ApiError::forbidden("operator role required"))?;
    let rows = db
        .prepare(
            "SELECT sync_id, agent_id, world_id, game_build_id, parser_version, status, \
             owner_count, pal_count, inventory_slot_count, error_code, received_at, completed_at \
             FROM sync_runs ORDER BY received_at DESC LIMIT 100",
        )
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<SyncRow>()
        .map_err(ApiError::internal)?;
    json_response(&json!({"items": rows, "count": rows.len()}))
}

async fn ops_audit(db: &D1Database, principal: &PrincipalContext) -> Result<Response, ApiError> {
    principal
        .authorize(ProtectedCapability::OperatorHealth)
        .map_err(|_| ApiError::forbidden("operator role required"))?;
    let rows = db
        .prepare(
            "SELECT event_id, action, target_kind, target_key_redacted, outcome, \
             request_id, created_at FROM audit_events \
             ORDER BY event_id DESC LIMIT 100",
        )
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<AuditRow>()
        .map_err(ApiError::internal)?;
    json_response(&json!({"items": rows, "count": rows.len()}))
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: i32,
}

async fn scalar_count(db: &D1Database, sql: &str) -> Result<i32, ApiError> {
    Ok(db
        .prepare(sql)
        .first::<CountRow>(None)
        .await
        .map_err(ApiError::internal)?
        .map(|row| row.count)
        .unwrap_or_default())
}

async fn audit(
    db: &D1Database,
    principal: &PrincipalContext,
    action: &str,
    target_kind: &str,
    target_key: Option<&str>,
    outcome: &str,
    request_id: &str,
) -> Result<(), ApiError> {
    let target_redacted = target_key.map(|value| {
        let digest = Sha256::digest(value.as_bytes());
        let mut output = String::with_capacity(16);
        use std::fmt::Write as _;
        for byte in digest.iter().take(8) {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
        }
        output
    });
    db.prepare(
        "INSERT INTO audit_events \
         (actor_principal_id, action, target_kind, target_key_redacted, outcome, request_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(&[
        JsValue::from_str(&principal.principal_hex()),
        JsValue::from_str(action),
        JsValue::from_str(target_kind),
        target_redacted
            .as_deref()
            .map(JsValue::from_str)
            .unwrap_or(JsValue::NULL),
        JsValue::from_str(outcome),
        JsValue::from_str(request_id),
    ])
    .map_err(ApiError::internal)?
    .run()
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

fn required_var(env: &Env, name: &str) -> Result<String, ApiError> {
    env.var(name)
        .map(|value| value.to_string())
        .map_err(|_| ApiError::internal(format!("missing Worker variable {name}")))
}

async fn resolve_catalog_entity(
    db: &D1Database,
    dataset_version: &str,
    entity_kind: &str,
    entity_id: Option<&str>,
    display_query: Option<&str>,
) -> Result<String, ApiError> {
    let candidate = entity_id
        .filter(|value| !value.trim().is_empty())
        .or_else(|| display_query.filter(|value| !value.trim().is_empty()))
        .map(str::trim)
        .ok_or_else(|| ApiError::bad_request("팰 또는 특성 이름을 입력하세요"))?;
    if candidate.chars().count() > 128 {
        return Err(ApiError::bad_request(
            "팰 또는 특성 이름은 128자 이하여야 합니다",
        ));
    }

    let sql = match entity_kind {
        "species" => {
            "SELECT DISTINCT cs.species_id AS entity_id \
             FROM catalog_species cs \
             LEFT JOIN catalog_aliases ca \
               ON ca.dataset_version=cs.dataset_version \
              AND ca.entity_kind='species' AND ca.entity_id=cs.species_id \
             WHERE cs.dataset_version=?1 AND (\
               cs.species_id=?2 COLLATE NOCASE OR cs.name_ko=?2 OR \
               cs.name_en=?2 COLLATE NOCASE OR ca.alias=?2 COLLATE NOCASE) \
             ORDER BY cs.species_id LIMIT 3"
        }
        "passive" => {
            "SELECT DISTINCT cp.passive_id AS entity_id \
             FROM catalog_passives cp \
             LEFT JOIN catalog_aliases ca \
               ON ca.dataset_version=cp.dataset_version \
              AND ca.entity_kind='passive' AND ca.entity_id=cp.passive_id \
             WHERE cp.dataset_version=?1 AND (\
               cp.passive_id=?2 COLLATE NOCASE OR cp.name_ko=?2 OR \
               ca.alias=?2 COLLATE NOCASE) \
             ORDER BY cp.passive_id LIMIT 3"
        }
        _ => return Err(ApiError::internal("unsupported catalog entity kind")),
    };
    let rows = db
        .prepare(sql)
        .bind(&[
            JsValue::from_str(dataset_version),
            JsValue::from_str(candidate),
        ])
        .map_err(ApiError::internal)?
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<CatalogIdRow>()
        .map_err(ApiError::internal)?;
    match rows.as_slice() {
        [row] => Ok(row.entity_id.clone()),
        [] => Err(ApiError::unprocessable(format!(
            "'{candidate}'에 해당하는 {}을(를) 찾지 못했습니다",
            if entity_kind == "species" {
                "팰"
            } else {
                "특성"
            }
        ))),
        _ => Err(ApiError::unprocessable(format!(
            "'{candidate}' 검색 결과가 여러 개입니다. 자동완성 목록에서 선택하세요"
        ))),
    }
}

async fn load_catalog_names(
    db: &D1Database,
    dataset_version: &str,
    entity_kind: &str,
) -> Result<BTreeMap<String, String>, ApiError> {
    let sql = match entity_kind {
        "species" => {
            "SELECT species_id AS entity_id, name_ko AS display_name \
             FROM catalog_species WHERE dataset_version=?1 ORDER BY species_id"
        }
        "passive" => {
            "SELECT passive_id AS entity_id, name_ko AS display_name \
             FROM catalog_passives WHERE dataset_version=?1 ORDER BY passive_id"
        }
        _ => return Err(ApiError::internal("unsupported catalog entity kind")),
    };
    let rows = db
        .prepare(sql)
        .bind(&[JsValue::from_str(dataset_version)])
        .map_err(ApiError::internal)?
        .all()
        .await
        .map_err(ApiError::internal)?
        .results::<CatalogNameRow>()
        .map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|row| (row.entity_id, row.display_name))
        .collect())
}

fn catalog_like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn catalog_work_like_pattern(query: &str) -> String {
    let query = query.trim().to_lowercase();
    let internal_term = [
        (
            ["불 피우기", "불피우기", "kindling"].as_slice(),
            "emitflame",
        ),
        (["관개", "watering"].as_slice(), "watering"),
        (["파종", "planting"].as_slice(), "seeding"),
        (
            ["발전", "전기", "electricity"].as_slice(),
            "generateelectricity",
        ),
        (["수작업", "handiwork"].as_slice(), "handcraft"),
        (["채집", "gathering"].as_slice(), "collection"),
        (["벌목", "lumbering"].as_slice(), "deforest"),
        (["채굴", "mining"].as_slice(), "mining"),
        (["제약", "medicine"].as_slice(), "productmedicine"),
        (["냉각", "cooling"].as_slice(), "cool"),
        (["운반", "transporting"].as_slice(), "transport"),
        (["목장", "ranch", "farming"].as_slice(), "monsterfarm"),
    ]
    .into_iter()
    .find_map(|(aliases, internal)| {
        aliases
            .iter()
            .any(|alias| query.contains(alias))
            .then_some(internal)
    })
    .unwrap_or(query.as_str());
    catalog_like_pattern(internal_term)
}

fn parse_json_or_default(value: &str, fallback: serde_json::Value) -> serde_json::Value {
    serde_json::from_str(value).unwrap_or(fallback)
}

fn split_ids(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn valid_hex_id(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_short_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn valid_email(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 320
        && value.contains('@')
        && !value.contains(char::is_whitespace)
}

fn default_max_generations() -> u32 {
    6
}

fn default_catalog_search_limit() -> u32 {
    12
}

fn optional_number(value: Option<i32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "?".to_owned())
}

fn fts_query(question: &str) -> String {
    question
        .split_whitespace()
        .take(10)
        .map(|word| {
            word.chars()
                .filter(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn digest_like_id(value: &str) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(value.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…", &value[..boundary])
}

async fn read_bounded_body(req: &mut Request, max_bytes: usize) -> Result<Vec<u8>, ApiError> {
    let declared_length = req
        .headers()
        .get("content-length")
        .map_err(ApiError::internal)?
        .map(|value| value.parse::<usize>().map_err(ApiError::bad_request))
        .transpose()?;
    if declared_length.is_some_and(|length| length > max_bytes) {
        return Err(ApiError::payload_too_large("request body is too large"));
    }

    let mut body = Vec::with_capacity(declared_length.unwrap_or(0).min(max_bytes));
    let mut stream = req.stream().map_err(ApiError::bad_request)?;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(ApiError::bad_request)?;
        let next_length = body
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| ApiError::payload_too_large("request body is too large"))?;
        if next_length > max_bytes {
            return Err(ApiError::payload_too_large("request body is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn read_json_body<T: DeserializeOwned>(
    req: &mut Request,
    max_bytes: usize,
) -> Result<T, ApiError> {
    let body = read_bounded_body(req, max_bytes).await?;
    if body.is_empty() {
        return Err(ApiError::bad_request("request body is empty"));
    }
    serde_json::from_slice(&body).map_err(ApiError::bad_request)
}

fn request_id(req: &Request) -> String {
    req.headers()
        .get("cf-ray")
        .ok()
        .flatten()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("local-{}", Date::now() as u64))
}

fn json_response<T: Serialize>(value: &T) -> Result<Response, ApiError> {
    Response::from_json(value).map_err(ApiError::internal)
}

async fn set_response_etag(response: &mut Response) -> WorkerResult<()> {
    use std::fmt::Write as _;

    let mut body = response.cloned()?;
    let digest = Sha256::digest(body.bytes().await?);
    let mut encoded = String::with_capacity(66);
    encoded.push('"');
    for byte in digest {
        write!(encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded.push('"');
    response.headers_mut().set("ETag", &encoded)?;
    Ok(())
}

fn prepare_cached_response(req: &Request, response: Response) -> WorkerResult<Response> {
    let headers = Headers::new();
    for (name, value) in response.headers().entries() {
        headers.append(&name, &value)?;
    }
    let not_modified = req
        .headers()
        .get("If-None-Match")?
        .zip(response.headers().get("ETag")?)
        .is_some_and(|(candidate, current)| if_none_match_matches(&candidate, &current));
    let status = response.status_code();
    let (_, body) = response.into_parts();
    let builder = ResponseBuilder::new()
        .with_status(if not_modified { 304 } else { status })
        .with_headers(headers);
    Ok(if not_modified {
        builder.empty()
    } else {
        builder.body(body)
    })
}

fn error_response(status: u16, code: &str, message: &str) -> WorkerResult<Response> {
    Response::from_json(&json!({
        "error": {
            "code": code,
            "message": message
        }
    }))
    .map(|response| response.with_status(status))
}

fn set_security_headers(
    response: &mut Response,
    request_id: &str,
    cache_control: &str,
    cache_status: &str,
) -> WorkerResult<()> {
    let headers = response.headers_mut();
    headers.set("Cache-Control", cache_control)?;
    headers.set("X-Pal-Cache", cache_status)?;
    headers.set("X-Content-Type-Options", "nosniff")?;
    headers.set("Referrer-Policy", "no-referrer")?;
    headers.set(
        "Permissions-Policy",
        "camera=(), microphone=(), geolocation=()",
    )?;
    headers.set(
        "Content-Security-Policy",
        "default-src 'none'; frame-ancestors 'none'",
    )?;
    headers.set("X-Request-Id", request_id)?;
    Ok(())
}

#[derive(Debug)]
struct ApiError {
    status: u16,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn internal(error: impl std::fmt::Display) -> Self {
        let _ = error;
        Self {
            status: 500,
            code: "INTERNAL_ERROR",
            message: "internal service error".to_owned(),
        }
    }

    fn internal_js(error: JsValue) -> Self {
        let _ = error;
        Self {
            status: 500,
            code: "INTERNAL_ERROR",
            message: "internal WebCrypto error".to_owned(),
        }
    }

    fn bad_request(error: impl std::fmt::Display) -> Self {
        Self {
            status: 400,
            code: "BAD_REQUEST",
            message: error.to_string(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: 401,
            code: "UNAUTHORIZED",
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: 403,
            code: "FORBIDDEN",
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: 404,
            code: "NOT_FOUND",
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: 409,
            code: "CONFLICT",
            message: message.into(),
        }
    }

    fn conflict_with_code(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: 409,
            code,
            message: message.into(),
        }
    }

    fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: 413,
            code: "PAYLOAD_TOO_LARGE",
            message: message.into(),
        }
    }

    fn unprocessable(message: impl Into<String>) -> Self {
        Self {
            status: 422,
            code: "UNPROCESSABLE",
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: 503,
            code: "DEPENDENCY_UNAVAILABLE",
            message: message.into(),
        }
    }

    fn method_not_allowed(message: impl Into<String>) -> Self {
        Self {
            status: 405,
            code: "METHOD_NOT_ALLOWED",
            message: message.into(),
        }
    }
}
