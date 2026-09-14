use std::{
    collections::BTreeMap,
    env, fs,
    io::{Read, Write as _},
    path::{Path, PathBuf},
    process::ExitCode,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const SCHEMA_VERSION: u32 = 1;
const MAX_INPUT_BYTES: u64 = 64 * 1024;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_DATA_BYTES: u64 = 8 * 1024 * 1024;
const MAX_QUERY_CHARS: usize = 80;
const MAX_RESULTS: usize = 4096;
const PALS_SHA256: &str = "7ed73e0518b74af7cb72ebed688e903587b21b2d644be8dda9b26aa7c007f512";
const KO_PALS_SHA256: &str = "ce7c85a0de8da454f61a9f37acddc0d675f7d92f92334ad5b2bcf6236a58a892";
const ACTIVE_SKILLS_SHA256: &str =
    "61b68d1b33f9abaea3e8c8d4be44ae37fe3c4240ccfad2d6a8b3433e6ffaaefa";
const KO_ACTIVE_SKILLS_SHA256: &str =
    "182478889dcffb13a6f33a96ac908876533526e698d9fd34ccb3020fccbc4ab4";
const PASSIVE_SKILLS_SHA256: &str =
    "3085aee5328fe8e90418f5f682a4379ae4131b3a038a6f83f1780e2ef08f7033";
const KO_PASSIVE_SKILLS_SHA256: &str =
    "f90c106e4307f13a08a095354f9eda79a974b47f6720efb34afb5e54601738da";
const KO_UI_TAXONOMY_SHA256: &str =
    "8ae3432ede29327ca0e36e6bc8ea1942e4cbd54296977dc1197889a69497e2a6";
const ITEMS_V1_SHA256: &str = "92af4551b21248c4babbeb9ca46d2e97e5b5c6613192554a840a24ab63005f31";
const WORLD_V1_SHA256: &str = "c8867007ac0361b667ddb896cd98a2b56e99caa1dc0464f596ddc3c0ee614962";
const ITEM_CATALOG_GAME_BUILD_ID: &str = "steam:24575825";

#[derive(Debug)]
struct Options {
    catalog_dir: PathBuf,
    serve: bool,
}

#[derive(Debug, Deserialize)]
struct Request {
    schema_version: u32,
    #[serde(default)]
    kind: CatalogKind,
    #[serde(default)]
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CatalogKind {
    #[default]
    Species,
    ActiveSkill,
    Passive,
    Item,
    Building,
    Technology,
    Shop,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum CatalogResponse {
    Species(Response),
    ActiveSkills(ActiveSkillResponse),
    Passives(PassiveResponse),
    Items(ItemResponse),
    Buildings(ExactCatalogResponse<RawBuildingRecord>),
    Technologies(ExactCatalogResponse<RawTechnologyRecord>),
    Shops(ExactCatalogResponse<RawShopGroupRecord>),
}

#[derive(Debug, Serialize)]
struct Response {
    schema_version: u32,
    worker_id: &'static str,
    status: &'static str,
    query: String,
    matched_count: usize,
    returned_count: usize,
    quality: &'static str,
    source_id: &'static str,
    kind: &'static str,
    records: Vec<SpeciesRecord>,
    warnings: Vec<&'static str>,
}

#[derive(Clone, Debug, Default)]
struct RawCatalog {
    pals: serde_json::Map<String, serde_json::Value>,
    pal_names: BTreeMap<String, String>,
    skill_data: BTreeMap<String, serde_json::Value>,
    skill_names: BTreeMap<String, String>,
    passive_names: BTreeMap<String, String>,
    active_skill_records: BTreeMap<String, serde_json::Value>,
    active_skill_ids: BTreeMap<String, String>,
    active_skill_texts: BTreeMap<String, LocalizedText>,
    passive_records: BTreeMap<String, serde_json::Value>,
    passive_ids: BTreeMap<String, String>,
    passive_texts: BTreeMap<String, LocalizedText>,
    element_names: BTreeMap<String, ExactKoreanUiText>,
    work_suitability_names: BTreeMap<String, ExactKoreanUiText>,
    item_category_names: BTreeMap<String, ExactKoreanUiText>,
    items: BTreeMap<String, ItemCatalogEntry>,
    buildings: BTreeMap<String, RawBuildingRecord>,
    technologies: BTreeMap<String, RawTechnologyRecord>,
    shop_groups: BTreeMap<String, RawShopGroupRecord>,
}

#[derive(Clone, Debug, Serialize)]
struct SpeciesRecord {
    species_id: String,
    name_ko: Option<String>,
    paldex_number: Option<u32>,
    rarity: Option<u32>,
    elements: Vec<LocalizedValue>,
    hp: Option<i64>,
    attack: Option<i64>,
    defense: Option<i64>,
    run_speed: Option<i64>,
    ride_sprint_speed: Option<i64>,
    transport_speed: Option<i64>,
    stamina: Option<i64>,
    food_amount: Option<i64>,
    nocturnal: bool,
    work_suitability: Vec<WorkSuitability>,
    learned_skills: Vec<LearnedSkill>,
    guaranteed_passives: Vec<LocalizedValue>,
}

#[derive(Clone, Debug, Serialize)]
struct LocalizedValue {
    id: String,
    name_ko: String,
}

#[derive(Clone, Debug, Serialize)]
struct WorkSuitability {
    id: String,
    name_ko: String,
    level: i64,
}

#[derive(Clone, Debug, Serialize)]
struct LearnedSkill {
    skill_id: String,
    name_ko: String,
    learn_level: i64,
    element_ko: Option<String>,
    power: Option<i64>,
    cool_time: Option<f64>,
}

#[derive(Clone, Debug)]
struct LocalizedText {
    name_ko: String,
    description_ko: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct VerifiedKoreanUiDocument {
    schema_version: u32,
    game_build_id: String,
    verified: bool,
    language: String,
    localization_origin: String,
    machine_translation_allowed: bool,
    source: ExactKoreanUiSource,
    elements: BTreeMap<String, ExactKoreanUiText>,
    work_suitability: BTreeMap<String, ExactKoreanUiText>,
    item_categories: BTreeMap<String, ExactKoreanUiText>,
}

#[derive(Clone, Debug, Deserialize)]
struct ExactKoreanUiSource {
    package_path: String,
    row_count: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct ExactKoreanUiText {
    source_row_key: String,
    name_ko: String,
}

#[derive(Debug, Serialize)]
struct ActiveSkillResponse {
    schema_version: u32,
    worker_id: &'static str,
    status: &'static str,
    query: String,
    matched_count: usize,
    returned_count: usize,
    quality: &'static str,
    source_id: &'static str,
    kind: &'static str,
    records: Vec<ActiveSkillRecord>,
    warnings: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
struct ActiveSkillRecord {
    skill_id: String,
    name_ko: String,
    description_ko: Option<String>,
    element: Option<String>,
    element_ko: Option<String>,
    skill_kind: Option<String>,
    power: Option<i64>,
    min_range: Option<i64>,
    max_range: Option<i64>,
    cool_time: Option<f64>,
    effects: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct PassiveResponse {
    schema_version: u32,
    worker_id: &'static str,
    status: &'static str,
    query: String,
    matched_count: usize,
    returned_count: usize,
    quality: &'static str,
    source_id: &'static str,
    kind: &'static str,
    records: Vec<PassiveRecord>,
    warnings: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
struct PassiveRecord {
    passive_id: String,
    name_ko: String,
    description_ko: Option<String>,
    rank: Option<i64>,
    effects: Vec<serde_json::Value>,
}

#[derive(Clone, Debug)]
struct ItemCatalogEntry {
    item: RawItemRecord,
    recipes: Vec<RawRecipeRecord>,
    pal_drops: Vec<RawPalDropRecord>,
}

#[derive(Clone, Debug, Deserialize)]
struct VerifiedItemCatalogDocument {
    schema_version: u32,
    game_build_id: String,
    verified: bool,
    items: Vec<RawItemRecord>,
    recipes: Vec<RawRecipeRecord>,
    pal_drops: Vec<RawPalDropRecord>,
}

#[derive(Clone, Debug, Deserialize)]
struct RawItemRecord {
    item_id: String,
    name_ko: String,
    description_ko: Option<String>,
    type_a: String,
    type_b: String,
    price: i64,
    weight_milli: i64,
    maximum_stack_count: i64,
    rarity: i64,
    rank: i64,
    icon_name: String,
    legal_in_game: bool,
    localization_fallback: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct RawRecipeIngredient {
    item_id: String,
    quantity: i64,
}

#[derive(Clone, Debug, Deserialize)]
struct RawRecipeRecord {
    recipe_id: String,
    output_item_id: String,
    output_quantity: i64,
    ingredients: Vec<RawRecipeIngredient>,
    work_amount: i64,
    unlock_item_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct RawPalDropRecord {
    method_id: String,
    pal_id: String,
    level: i64,
    item_id: String,
    minimum_quantity: i64,
    maximum_quantity: i64,
    probability_ppm: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RawBuildingMaterial {
    item_id: String,
    name_ko: String,
    quantity: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RawProductionMethodRecord {
    method_id: String,
    building_id: String,
    building_name_ko: String,
    item_id: String,
    item_name_ko: String,
    required_work_amount: i64,
    auto_work_amount_per_second: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RawBuildingRecord {
    building_id: String,
    name_ko: String,
    description_ko: Option<String>,
    category: String,
    subcategory: String,
    ui_category: String,
    required_energy_type: String,
    #[serde(default)]
    required_energy_name_ko: Option<String>,
    required_build_work_amount: i64,
    consume_energy_speed: i64,
    build_capacity: i64,
    install_max_num_in_base_camp: i64,
    rank: i64,
    sort_id: i64,
    hp: Option<i64>,
    defense: Option<i64>,
    material_type: Option<String>,
    material_subtype: Option<String>,
    icon_source_path: Option<String>,
    belongs_to_base_camp: bool,
    install_only_near_palbox: bool,
    install_only_in_door: bool,
    install_only_on_base: bool,
    prohibited_in_raid_boss_area: bool,
    paintable: bool,
    in_development: bool,
    localization_fallback: bool,
    materials: Vec<RawBuildingMaterial>,
    #[serde(default)]
    production_methods: Vec<RawProductionMethodRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RawTechnologyRecord {
    technology_id: String,
    name_ko: String,
    description_ko: Option<String>,
    icon_name: String,
    level: i64,
    cost: i64,
    tier: i64,
    is_boss_technology: bool,
    required_tower_boss: Option<String>,
    required_research_id: Option<String>,
    required_technology_id: Option<String>,
    localization_fallback: bool,
    unlock_building_ids: Vec<String>,
    unlock_recipe_ids: Vec<String>,
    unlock_item_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RawShopProductRecord {
    product_id: String,
    item_id: String,
    item_name_ko: String,
    quantity: i64,
    price: i64,
    product_type: String,
    stock: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RawShopGroupRecord {
    shop_group_id: String,
    currency_item_id: String,
    currency_name_ko: String,
    products: Vec<RawShopProductRecord>,
}

#[derive(Clone, Debug, Deserialize)]
struct VerifiedWorldCatalogDocument {
    schema_version: u32,
    game_build_id: String,
    verified: bool,
    buildings: Vec<RawBuildingRecord>,
    technologies: Vec<RawTechnologyRecord>,
    production_methods: Vec<RawProductionMethodRecord>,
    shop_groups: Vec<RawShopGroupRecord>,
}

#[derive(Debug, Serialize)]
struct ExactCatalogResponse<T> {
    schema_version: u32,
    worker_id: &'static str,
    status: &'static str,
    query: String,
    matched_count: usize,
    returned_count: usize,
    quality: &'static str,
    source_id: &'static str,
    kind: &'static str,
    game_build_id: &'static str,
    records: Vec<T>,
    warnings: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ItemResponse {
    schema_version: u32,
    worker_id: &'static str,
    status: &'static str,
    query: String,
    matched_count: usize,
    returned_count: usize,
    quality: &'static str,
    source_id: &'static str,
    kind: &'static str,
    game_build_id: &'static str,
    records: Vec<ItemRecord>,
    warnings: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
struct ItemRecord {
    item_id: String,
    name_ko: String,
    description_ko: Option<String>,
    category: String,
    category_ko: Option<String>,
    subcategory: String,
    price: i64,
    weight_milli: i64,
    maximum_stack_count: i64,
    rarity: i64,
    rank: i64,
    icon_name: String,
    legal_in_game: bool,
    localization_fallback: bool,
    recipe_count: usize,
    pal_drop_count: usize,
    recipes: Vec<ItemRecipeRecord>,
    pal_drops: Vec<ItemPalDropRecord>,
}

#[derive(Clone, Debug, Serialize)]
struct ItemRecipeRecord {
    recipe_id: String,
    output_quantity: i64,
    ingredients: Vec<ItemRecipeIngredient>,
    work_amount: i64,
    unlock_item_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ItemRecipeIngredient {
    item_id: String,
    name_ko: String,
    quantity: i64,
}

#[derive(Clone, Debug, Serialize)]
struct ItemPalDropRecord {
    method_id: String,
    pal_id: String,
    pal_name_ko: Option<String>,
    level: i64,
    minimum_quantity: i64,
    maximum_quantity: i64,
    probability_ppm: i64,
}

fn default_limit() -> usize {
    50
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pal-catalog-worker: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let options = parse_options(env::args_os().skip(1))?;
    let catalog = RawCatalog::load(&options.catalog_dir)?;
    if options.serve {
        return serve_requests(&catalog, std::io::stdin().lock(), std::io::stdout().lock());
    }
    let request = read_request(std::io::stdin().lock())?;
    let response = catalog.search(request)?;
    let encoded = encode_response(&response)?;
    std::io::stdout()
        .lock()
        .write_all(&encoded)
        .map_err(|_| "검색 결과를 전달하지 못했습니다.".to_owned())
}

fn parse_options(arguments: impl Iterator<Item = std::ffi::OsString>) -> Result<Options, String> {
    let arguments = arguments.collect::<Vec<_>>();
    if arguments.len() < 2
        || arguments.len() > 3
        || arguments[0] != "--catalog-dir"
        || (arguments.len() == 3 && arguments[2] != "--serve")
    {
        return Err("usage: pal-catalog-worker --catalog-dir <path> [--serve]".to_owned());
    }
    let catalog_dir = PathBuf::from(&arguments[1]);
    if !catalog_dir.is_absolute() {
        return Err("--catalog-dir must be absolute".to_owned());
    }
    Ok(Options {
        catalog_dir,
        serve: arguments.len() == 3,
    })
}

fn read_request(mut input: impl Read) -> Result<Request, String> {
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "검색 요청을 읽지 못했습니다.".to_owned())?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("검색 요청 크기가 올바르지 않습니다.".to_owned());
    }
    decode_request(&bytes)
}

fn decode_request(bytes: &[u8]) -> Result<Request, String> {
    let request: Request =
        serde_json::from_slice(bytes).map_err(|_| "검색 요청 형식이 올바르지 않습니다.")?;
    if request.schema_version != SCHEMA_VERSION {
        return Err("지원하지 않는 검색 요청 버전입니다.".to_owned());
    }
    if request.query.chars().count() > MAX_QUERY_CHARS {
        return Err("검색어는 80자 이하여야 합니다.".to_owned());
    }
    if request.limit == 0 || request.limit > MAX_RESULTS {
        return Err(format!("검색 결과 제한은 1~{MAX_RESULTS}이어야 합니다."));
    }
    Ok(request)
}

fn encode_response(response: &CatalogResponse) -> Result<Vec<u8>, String> {
    let encoded = serde_json::to_vec(response).map_err(|_| "검색 결과 인코딩에 실패했습니다.")?;
    if encoded.len() > MAX_OUTPUT_BYTES {
        return Err("검색 결과가 허용 크기를 초과했습니다.".to_owned());
    }
    Ok(encoded)
}

fn serve_requests(
    catalog: &RawCatalog,
    mut input: impl std::io::BufRead,
    mut output: impl std::io::Write,
) -> Result<(), String> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = input
            .read_until(b'\n', &mut line)
            .map_err(|_| "검색 요청 스트림을 읽지 못했습니다.".to_owned())?;
        if read == 0 {
            return Ok(());
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        if line.is_empty() || line.len() as u64 > MAX_INPUT_BYTES {
            return Err("검색 요청 크기가 올바르지 않습니다.".to_owned());
        }
        let request = decode_request(&line)?;
        let response = catalog.search(request)?;
        let encoded = encode_response(&response)?;
        output
            .write_all(&encoded)
            .and_then(|_| output.write_all(b"\n"))
            .and_then(|_| output.flush())
            .map_err(|_| "검색 결과 스트림을 전달하지 못했습니다.".to_owned())?;
    }
}

impl RawCatalog {
    fn load(directory: &Path) -> Result<Self, String> {
        let pals = verified_json_object(&directory.join("pals.json"), PALS_SHA256)?;
        let pal_names = localized_map(&directory.join("l10n/ko/pals.json"), KO_PALS_SHA256)?;
        let active_skills =
            verified_json_object(&directory.join("active_skills.json"), ACTIVE_SKILLS_SHA256)?;
        let active_texts = localized_text_map(
            &directory.join("l10n/ko/active_skills.json"),
            KO_ACTIVE_SKILLS_SHA256,
        )?;
        let passive_skills = verified_json_object(
            &directory.join("passive_skills.json"),
            PASSIVE_SKILLS_SHA256,
        )?;
        let passive_texts = localized_text_map(
            &directory.join("l10n/ko/passive_skills.json"),
            KO_PASSIVE_SKILLS_SHA256,
        )?;
        let korean_ui: VerifiedKoreanUiDocument = verified_json(
            &directory.join("l10n/ko/ui_taxonomy.v1.json"),
            KO_UI_TAXONOMY_SHA256,
        )?;
        validate_korean_ui_document(&korean_ui)?;
        let item_document: VerifiedItemCatalogDocument =
            verified_json(&directory.join("items.v1.json"), ITEMS_V1_SHA256)?;
        if item_document.schema_version != SCHEMA_VERSION
            || item_document.game_build_id != ITEM_CATALOG_GAME_BUILD_ID
            || !item_document.verified
        {
            return Err("아이템 데이터의 Build 또는 검증 상태가 올바르지 않습니다.".to_owned());
        }
        let mut items = item_document
            .items
            .into_iter()
            .map(|item| {
                let key = normalize_id(&item.item_id);
                (
                    key,
                    ItemCatalogEntry {
                        item,
                        recipes: Vec::new(),
                        pal_drops: Vec::new(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        for recipe in item_document.recipes {
            let key = normalize_id(&recipe.output_item_id);
            let entry = items
                .get_mut(&key)
                .ok_or_else(|| "제작식이 존재하지 않는 아이템을 참조합니다.".to_owned())?;
            entry.recipes.push(recipe);
        }
        for drop in item_document.pal_drops {
            let key = normalize_id(&drop.item_id);
            let entry = items
                .get_mut(&key)
                .ok_or_else(|| "드롭 정보가 존재하지 않는 아이템을 참조합니다.".to_owned())?;
            entry.pal_drops.push(drop);
        }
        for entry in items.values_mut() {
            entry
                .recipes
                .sort_by(|left, right| left.recipe_id.cmp(&right.recipe_id));
            entry.pal_drops.sort_by(|left, right| {
                right
                    .probability_ppm
                    .cmp(&left.probability_ppm)
                    .then(left.pal_id.cmp(&right.pal_id))
                    .then(left.method_id.cmp(&right.method_id))
            });
        }
        let world_document: VerifiedWorldCatalogDocument =
            verified_json(&directory.join("world.v1.json"), WORLD_V1_SHA256)?;
        if world_document.schema_version != SCHEMA_VERSION
            || world_document.game_build_id != ITEM_CATALOG_GAME_BUILD_ID
            || !world_document.verified
        {
            return Err("월드 카탈로그의 Build 또는 검증 상태가 올바르지 않습니다.".to_owned());
        }
        let mut buildings = world_document
            .buildings
            .into_iter()
            .map(|record| (normalize_id(&record.building_id), record))
            .collect::<BTreeMap<_, _>>();
        for production in world_document.production_methods {
            if let Some(building) = buildings.get_mut(&normalize_id(&production.building_id)) {
                building.production_methods.push(production);
            }
        }
        for building in buildings.values_mut() {
            building.required_energy_name_ko = korean_ui
                .elements
                .get(&building.required_energy_type)
                .map(|text| text.name_ko.clone());
            building
                .production_methods
                .sort_by(|left, right| left.item_name_ko.cmp(&right.item_name_ko));
        }
        let mut technologies = world_document
            .technologies
            .into_iter()
            .map(|record| (normalize_id(&record.technology_id), record))
            .collect::<BTreeMap<_, _>>();
        for technology in technologies.values_mut() {
            if !technology.name_ko.trim().is_empty() {
                continue;
            }
            let mut relation_names = technology
                .unlock_item_ids
                .iter()
                .filter_map(|item_id| items.get(&normalize_id(item_id)))
                .filter(|entry| !entry.item.localization_fallback)
                .map(|entry| entry.item.name_ko.clone())
                .chain(
                    technology
                        .unlock_building_ids
                        .iter()
                        .filter_map(|building_id| buildings.get(&normalize_id(building_id)))
                        .filter(|building| !building.localization_fallback)
                        .map(|building| building.name_ko.clone()),
                )
                .filter(|name| !name.trim().is_empty())
                .collect::<Vec<_>>();
            relation_names.sort();
            relation_names.dedup();
            technology.name_ko = relation_names.join(" / ");
        }
        let shop_groups = world_document
            .shop_groups
            .into_iter()
            .map(|record| (normalize_id(&record.shop_group_id), record))
            .collect::<BTreeMap<_, _>>();

        let mut skill_data = BTreeMap::new();
        let mut active_skill_records = BTreeMap::new();
        let mut active_skill_ids = BTreeMap::new();
        for (id, value) in active_skills {
            skill_data.insert(normalize_id(&id), value.clone());
            let short = id.rsplit("::").next().unwrap_or(&id);
            let normalized = normalize_id(short);
            skill_data.insert(normalized.clone(), value.clone());
            active_skill_ids.insert(normalized.clone(), short.to_owned());
            active_skill_records.insert(normalized, value);
        }
        let mut skill_names = BTreeMap::new();
        let mut active_skill_texts = BTreeMap::new();
        for (id, text) in active_texts {
            skill_names.insert(normalize_id(&id), text.name_ko.clone());
            let short = id.rsplit("::").next().unwrap_or(&id);
            let normalized = normalize_id(short);
            skill_names.insert(normalized.clone(), text.name_ko.clone());
            active_skill_texts.insert(normalized, text);
        }
        let declared_passives = passive_skills
            .keys()
            .map(|id| normalize_id(id))
            .collect::<std::collections::BTreeSet<_>>();
        let passive_names = passive_texts
            .iter()
            .filter(|(id, _)| declared_passives.contains(&normalize_id(id)))
            .map(|(id, text)| (normalize_id(id), text.name_ko.clone()))
            .collect();
        let passive_texts = passive_texts
            .into_iter()
            .filter(|(id, _)| declared_passives.contains(&normalize_id(id)))
            .map(|(id, text)| (normalize_id(&id), text))
            .collect();
        let mut passive_records = BTreeMap::new();
        let mut passive_ids = BTreeMap::new();
        for (id, value) in passive_skills {
            let normalized = normalize_id(&id);
            passive_ids.insert(normalized.clone(), id);
            passive_records.insert(normalized, value);
        }
        if active_skill_records
            .keys()
            .any(|id| !active_skill_texts.contains_key(id))
        {
            return Err("액티브 스킬에 대응하는 한국어 게임 원문이 없습니다.".to_owned());
        }
        Ok(Self {
            pals,
            pal_names: pal_names
                .into_iter()
                .map(|(id, name)| (normalize_id(&id), name))
                .collect(),
            skill_data,
            skill_names,
            passive_names,
            active_skill_records,
            active_skill_ids,
            active_skill_texts,
            passive_records,
            passive_ids,
            passive_texts,
            element_names: korean_ui
                .elements
                .into_iter()
                .map(|(id, text)| (normalize_id(&id), text))
                .collect(),
            work_suitability_names: korean_ui
                .work_suitability
                .into_iter()
                .map(|(id, text)| (normalize_id(&id), text))
                .collect(),
            item_category_names: korean_ui
                .item_categories
                .into_iter()
                .map(|(id, text)| (normalize_id(&id), text))
                .collect(),
            items,
            buildings,
            technologies,
            shop_groups,
        })
    }

    fn search(&self, request: Request) -> Result<CatalogResponse, String> {
        match request.kind {
            CatalogKind::Species => self.search_species(request).map(CatalogResponse::Species),
            CatalogKind::ActiveSkill => Ok(CatalogResponse::ActiveSkills(
                self.search_active_skills(request),
            )),
            CatalogKind::Passive => Ok(CatalogResponse::Passives(self.search_passives(request))),
            CatalogKind::Item => Ok(CatalogResponse::Items(self.search_items(request))),
            CatalogKind::Building => Ok(CatalogResponse::Buildings(self.search_buildings(request))),
            CatalogKind::Technology => Ok(CatalogResponse::Technologies(
                self.search_technologies(request),
            )),
            CatalogKind::Shop => Ok(CatalogResponse::Shops(self.search_shops(request))),
        }
    }

    fn search_species(&self, request: Request) -> Result<Response, String> {
        let query = request.query.trim().to_lowercase();
        let matches = self
            .pals
            .iter()
            .filter_map(|(id, value)| {
                let record = self.record(id, value)?;
                let score = search_score(&record, &query)?;
                Some((score, record))
            })
            .collect::<Vec<_>>();
        let mut unique_matches = BTreeMap::<String, (u8, SpeciesRecord)>::new();
        for (score, record) in matches {
            let key = normalize_id(&record.species_id);
            let replace = unique_matches
                .get(&key)
                .is_none_or(|(current_score, current_record)| {
                    score > *current_score
                        || (score == *current_score
                            && current_record.name_ko.is_none()
                            && record.name_ko.is_some())
                });
            if replace {
                unique_matches.insert(key, (score, record));
            }
        }
        let mut matches = unique_matches.into_values().collect::<Vec<_>>();
        matches.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then(
                    left.paldex_number
                        .unwrap_or(u32::MAX)
                        .cmp(&right.paldex_number.unwrap_or(u32::MAX)),
                )
                .then(left.species_id.cmp(&right.species_id))
        });
        let matched_count = matches.len();
        let records = matches
            .into_iter()
            .map(|(_, record)| record)
            .take(request.limit)
            .collect::<Vec<_>>();
        Ok(Response {
            schema_version: SCHEMA_VERSION,
            worker_id: "pal-catalog-worker-v1",
            status: "ok",
            query: request.query.trim().to_owned(),
            matched_count,
            returned_count: records.len(),
            quality: "unknown",
            source_id: "psp-static-catalog:pinned-private-alpha",
            kind: "species",
            records,
            warnings: vec!["PINNED_DATASET_BUILD_MATCH_NOT_YET_PROVEN"],
        })
    }

    fn search_active_skills(&self, request: Request) -> ActiveSkillResponse {
        let query = request.query.trim().to_lowercase();
        let mut records = self
            .active_skill_records
            .iter()
            .filter_map(|(normalized_id, value)| {
                let record = self.active_skill_record(normalized_id, value);
                search_active_skill_score(&record, &query).map(|score| (score, record))
            })
            .collect::<Vec<_>>();
        records.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then(right.power.cmp(&left.power))
                .then(left.name_ko.cmp(&right.name_ko))
                .then(left.skill_id.cmp(&right.skill_id))
        });
        let matched_count = records.len();
        let records = records
            .into_iter()
            .map(|(_, record)| record)
            .take(request.limit)
            .collect::<Vec<_>>();
        ActiveSkillResponse {
            schema_version: SCHEMA_VERSION,
            worker_id: "pal-catalog-worker-v1",
            status: "ok",
            query: request.query.trim().to_owned(),
            matched_count,
            returned_count: records.len(),
            quality: "unknown",
            source_id: "psp-static-catalog:pinned-private-alpha",
            kind: "active_skill",
            records,
            warnings: vec!["PINNED_DATASET_BUILD_MATCH_NOT_YET_PROVEN"],
        }
    }

    fn search_passives(&self, request: Request) -> PassiveResponse {
        let query = request.query.trim().to_lowercase();
        let mut records = self
            .passive_records
            .iter()
            .filter_map(|(normalized_id, value)| {
                let record = self.passive_record(normalized_id, value)?;
                search_passive_score(&record, &query).map(|score| (score, record))
            })
            .collect::<Vec<_>>();
        records.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then(right.rank.cmp(&left.rank))
                .then(left.name_ko.cmp(&right.name_ko))
                .then(left.passive_id.cmp(&right.passive_id))
        });
        let matched_count = records.len();
        let records = records
            .into_iter()
            .map(|(_, record)| record)
            .take(request.limit)
            .collect::<Vec<_>>();
        PassiveResponse {
            schema_version: SCHEMA_VERSION,
            worker_id: "pal-catalog-worker-v1",
            status: "ok",
            query: request.query.trim().to_owned(),
            matched_count,
            returned_count: records.len(),
            quality: "unknown",
            source_id: "psp-static-catalog:pinned-private-alpha",
            kind: "passive",
            records,
            warnings: vec!["PINNED_DATASET_BUILD_MATCH_NOT_YET_PROVEN"],
        }
    }

    fn search_items(&self, request: Request) -> ItemResponse {
        let query = request.query.trim().to_lowercase();
        let mut records = self
            .items
            .values()
            .filter(|entry| !entry.item.localization_fallback)
            .filter(|entry| entry.item.legal_in_game || !query.is_empty())
            .filter_map(|entry| {
                let score = search_item_score(&entry.item, &query)?;
                Some((score, self.item_record(entry)))
            })
            .collect::<Vec<_>>();
        records.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then(right.legal_in_game.cmp(&left.legal_in_game))
                .then(left.name_ko.cmp(&right.name_ko))
                .then(left.item_id.cmp(&right.item_id))
        });
        let matched_count = records.len();
        let records = records
            .into_iter()
            .map(|(_, record)| record)
            .take(request.limit)
            .collect::<Vec<_>>();
        ItemResponse {
            schema_version: SCHEMA_VERSION,
            worker_id: "pal-catalog-worker-v1",
            status: "ok",
            query: request.query.trim().to_owned(),
            matched_count,
            returned_count: records.len(),
            quality: "exact",
            source_id: "game-files:steam:24575825:item-catalog",
            kind: "item",
            game_build_id: ITEM_CATALOG_GAME_BUILD_ID,
            records,
            warnings: Vec::new(),
        }
    }

    fn search_buildings(&self, request: Request) -> ExactCatalogResponse<RawBuildingRecord> {
        let query = request.query.trim().to_lowercase();
        let mut records = self
            .buildings
            .values()
            .filter(|record| !record.localization_fallback)
            .filter(|record| !record.in_development)
            .filter_map(|record| {
                search_building_score(record, &query).map(|score| (score, record.clone()))
            })
            .collect::<Vec<_>>();
        records.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then(left.sort_id.cmp(&right.sort_id))
                .then(left.name_ko.cmp(&right.name_ko))
        });
        exact_response(
            request,
            "building",
            records.into_iter().map(|(_, record)| record).collect(),
        )
    }

    fn search_technologies(&self, request: Request) -> ExactCatalogResponse<RawTechnologyRecord> {
        let query = request.query.trim().to_lowercase();
        let mut records = self
            .technologies
            .values()
            .filter(|record| !record.localization_fallback)
            .filter_map(|record| {
                search_technology_score(record, &query).map(|score| (score, record.clone()))
            })
            .collect::<Vec<_>>();
        records.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then(left.level.cmp(&right.level))
                .then(left.tier.cmp(&right.tier))
                .then(left.name_ko.cmp(&right.name_ko))
        });
        exact_response(
            request,
            "technology",
            records.into_iter().map(|(_, record)| record).collect(),
        )
    }

    fn search_shops(&self, request: Request) -> ExactCatalogResponse<RawShopGroupRecord> {
        let query = request.query.trim().to_lowercase();
        let mut records = self
            .shop_groups
            .values()
            .filter_map(|record| {
                search_shop_score(record, &query).map(|score| {
                    let mut record = record.clone();
                    if !query.is_empty() {
                        record.products.sort_by(|left, right| {
                            shop_product_matches(right, &query)
                                .cmp(&shop_product_matches(left, &query))
                                .then(left.item_name_ko.cmp(&right.item_name_ko))
                        });
                    }
                    (score, record)
                })
            })
            .collect::<Vec<_>>();
        records.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then(left.shop_group_id.cmp(&right.shop_group_id))
        });
        exact_response(
            request,
            "shop",
            records.into_iter().map(|(_, record)| record).collect(),
        )
    }

    fn item_record(&self, entry: &ItemCatalogEntry) -> ItemRecord {
        let recipes = entry
            .recipes
            .iter()
            .map(|recipe| ItemRecipeRecord {
                recipe_id: recipe.recipe_id.clone(),
                output_quantity: recipe.output_quantity,
                ingredients: recipe
                    .ingredients
                    .iter()
                    .map(|ingredient| ItemRecipeIngredient {
                        item_id: ingredient.item_id.clone(),
                        name_ko: self
                            .items
                            .get(&normalize_id(&ingredient.item_id))
                            .map(|entry| entry.item.name_ko.clone())
                            .unwrap_or_else(|| ingredient.item_id.clone()),
                        quantity: ingredient.quantity,
                    })
                    .collect(),
                work_amount: recipe.work_amount,
                unlock_item_id: recipe.unlock_item_id.clone(),
            })
            .collect();
        let pal_drops = entry
            .pal_drops
            .iter()
            .take(16)
            .map(|drop| ItemPalDropRecord {
                method_id: drop.method_id.clone(),
                pal_id: drop.pal_id.clone(),
                pal_name_ko: self.pal_display_name(&drop.pal_id),
                level: drop.level,
                minimum_quantity: drop.minimum_quantity,
                maximum_quantity: drop.maximum_quantity,
                probability_ppm: drop.probability_ppm,
            })
            .collect();
        ItemRecord {
            item_id: entry.item.item_id.clone(),
            name_ko: entry.item.name_ko.clone(),
            description_ko: entry.item.description_ko.clone(),
            category: entry.item.type_a.clone(),
            category_ko: self
                .item_category_names
                .get(&normalize_id(&entry.item.type_a))
                .map(|text| text.name_ko.clone()),
            subcategory: entry.item.type_b.clone(),
            price: entry.item.price,
            weight_milli: entry.item.weight_milli,
            maximum_stack_count: entry.item.maximum_stack_count,
            rarity: entry.item.rarity,
            rank: entry.item.rank,
            icon_name: entry.item.icon_name.clone(),
            legal_in_game: entry.item.legal_in_game,
            localization_fallback: entry.item.localization_fallback,
            recipe_count: entry.recipes.len(),
            pal_drop_count: entry.pal_drops.len(),
            recipes,
            pal_drops,
        }
    }

    fn pal_display_name(&self, pal_id: &str) -> Option<String> {
        let normalized = normalize_id(pal_id);
        self.pal_names.get(&normalized).cloned().or_else(|| {
            normalized
                .strip_prefix("boss_")
                .and_then(|candidate| self.pal_names.get(candidate))
                .cloned()
        })
    }

    fn active_skill_record(
        &self,
        normalized_id: &str,
        value: &serde_json::Value,
    ) -> ActiveSkillRecord {
        let text = self.active_skill_texts.get(normalized_id);
        let skill_id = self
            .active_skill_ids
            .get(normalized_id)
            .cloned()
            .unwrap_or_else(|| normalized_id.to_owned());
        let element = value
            .get("element")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        ActiveSkillRecord {
            skill_id,
            name_ko: text
                .map(|text| text.name_ko.clone())
                .expect("exact-Build active skills are validated during catalog load"),
            description_ko: text.and_then(|text| text.description_ko.clone()),
            element_ko: element
                .as_deref()
                .and_then(|element| self.element_name_ko(element))
                .map(str::to_owned),
            element,
            skill_kind: value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            power: value.get("power").and_then(serde_json::Value::as_i64),
            min_range: value.get("min_range").and_then(serde_json::Value::as_i64),
            max_range: value.get("max_range").and_then(serde_json::Value::as_i64),
            cool_time: value.get("cool_time").and_then(serde_json::Value::as_f64),
            effects: value
                .get("effects")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default(),
        }
    }

    fn passive_record(
        &self,
        normalized_id: &str,
        value: &serde_json::Value,
    ) -> Option<PassiveRecord> {
        let text = self.passive_texts.get(normalized_id)?;
        Some(PassiveRecord {
            passive_id: self
                .passive_ids
                .get(normalized_id)
                .cloned()
                .unwrap_or_else(|| normalized_id.to_owned()),
            name_ko: text.name_ko.clone(),
            description_ko: text.description_ko.clone(),
            rank: value.get("rank").and_then(serde_json::Value::as_i64),
            effects: value
                .get("effects")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default(),
        })
    }

    fn record(&self, id: &str, value: &serde_json::Value) -> Option<SpeciesRecord> {
        if value.get("is_pal").and_then(serde_json::Value::as_bool) != Some(true)
            || value.get("disabled").and_then(serde_json::Value::as_bool) == Some(true)
            || value
                .get("icon")
                .and_then(serde_json::Value::as_str)
                .is_none_or(str::is_empty)
        {
            return None;
        }
        let normalized_id = normalize_id(id);
        let name_ko = self.pal_names.get(&normalized_id).cloned()?;
        let scaling = value.get("scaling");
        let elements = value
            .get("element_types")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .filter_map(|element| {
                self.element_name_ko(element).map(|name_ko| LocalizedValue {
                    id: element.to_owned(),
                    name_ko: name_ko.to_owned(),
                })
            })
            .collect();
        let work_suitability = value
            .get("work_suitability")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(work, level)| {
                let level = level.as_i64()?;
                let name_ko = self.work_suitability_name_ko(work)?;
                (level > 0).then(|| WorkSuitability {
                    id: work.clone(),
                    name_ko: name_ko.to_owned(),
                    level,
                })
            })
            .collect();
        let mut learned_skills = value
            .get("skill_set")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(skill_id, learn_level)| {
                let normalized = normalize_id(skill_id);
                let skill = self.skill_data.get(&normalized);
                Some(LearnedSkill {
                    skill_id: skill_id.clone(),
                    name_ko: self.skill_names.get(&normalized)?.clone(),
                    learn_level: learn_level.as_i64()?,
                    element_ko: skill
                        .and_then(|value| value.get("element"))
                        .and_then(serde_json::Value::as_str)
                        .and_then(|element| self.element_name_ko(element))
                        .map(str::to_owned),
                    power: skill
                        .and_then(|value| value.get("power"))
                        .and_then(serde_json::Value::as_i64),
                    cool_time: skill
                        .and_then(|value| value.get("cool_time"))
                        .and_then(serde_json::Value::as_f64),
                })
            })
            .collect::<Vec<_>>();
        learned_skills.sort_by_key(|skill| skill.learn_level);
        let guaranteed_passives = value
            .get("passive_skills")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .filter_map(|passive_id| {
                Some(LocalizedValue {
                    id: passive_id.to_owned(),
                    name_ko: self.passive_names.get(&normalize_id(passive_id))?.clone(),
                })
            })
            .collect();
        Some(SpeciesRecord {
            species_id: id.to_owned(),
            name_ko: Some(name_ko),
            paldex_number: value
                .get("pal_deck_index")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            rarity: value
                .get("rarity")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            elements,
            hp: scaling
                .and_then(|value| value.get("hp"))
                .and_then(serde_json::Value::as_i64),
            attack: scaling
                .and_then(|value| value.get("attack"))
                .and_then(serde_json::Value::as_i64),
            defense: scaling
                .and_then(|value| value.get("defense"))
                .and_then(serde_json::Value::as_i64),
            run_speed: value.get("run_speed").and_then(serde_json::Value::as_i64),
            ride_sprint_speed: value
                .get("ride_sprint_speed")
                .and_then(serde_json::Value::as_i64),
            transport_speed: value
                .get("transport_speed")
                .and_then(serde_json::Value::as_i64),
            stamina: value.get("stamina").and_then(serde_json::Value::as_i64),
            food_amount: value.get("food_amount").and_then(serde_json::Value::as_i64),
            nocturnal: value
                .get("nocturnal")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            work_suitability,
            learned_skills,
            guaranteed_passives,
        })
    }

    fn element_name_ko(&self, value: &str) -> Option<&str> {
        let canonical = match value {
            "Electric" => "Electricity",
            "Grass" => "Leaf",
            "Ground" => "Earth",
            "Normal" => "Neutral",
            other => other,
        };
        self.element_names
            .get(&normalize_id(canonical))
            .map(|text| text.name_ko.as_str())
    }

    fn work_suitability_name_ko(&self, value: &str) -> Option<&str> {
        self.work_suitability_names
            .get(&normalize_id(value))
            .map(|text| text.name_ko.as_str())
    }
}

fn search_score(record: &SpeciesRecord, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(1);
    }
    let direct = search_atom_score(record, query);
    let token_score = query
        .split(|character: char| !character.is_alphanumeric() && character != '#')
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
        .filter(|token| !is_search_stopword(token))
        .filter_map(|token| search_atom_score(record, token))
        .max();
    direct.into_iter().chain(token_score).max()
}

fn search_active_skill_score(record: &ActiveSkillRecord, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(1);
    }
    let id = record.skill_id.to_lowercase();
    let name = record.name_ko.to_lowercase();
    let description = record
        .description_ko
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let element = record.element.as_deref().unwrap_or_default().to_lowercase();
    let element_ko = record
        .element_ko
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    if id == query || name == query {
        Some(100)
    } else if id.starts_with(query) || name.starts_with(query) {
        Some(80)
    } else if id.contains(query) || name.contains(query) {
        Some(60)
    } else if element.contains(query) || element_ko.contains(query) {
        Some(40)
    } else if description.contains(query) {
        Some(20)
    } else {
        None
    }
}

fn search_passive_score(record: &PassiveRecord, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(1);
    }
    let id = record.passive_id.to_lowercase();
    let name = record.name_ko.to_lowercase();
    let description = record
        .description_ko
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    if id == query || name == query {
        Some(100)
    } else if id.starts_with(query) || name.starts_with(query) {
        Some(80)
    } else if id.contains(query) || name.contains(query) {
        Some(60)
    } else if description.contains(query) {
        Some(20)
    } else {
        None
    }
}

fn search_item_score(record: &RawItemRecord, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(1);
    }
    let id = record.item_id.to_lowercase();
    let name = record.name_ko.to_lowercase();
    let description = record
        .description_ko
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let category = record.type_a.to_lowercase();
    let subcategory = record.type_b.to_lowercase();
    if id == query || name == query {
        Some(100)
    } else if id.starts_with(query) || name.starts_with(query) {
        Some(80)
    } else if id.contains(query) || name.contains(query) {
        Some(60)
    } else if category.contains(query) || subcategory.contains(query) {
        Some(40)
    } else if description.contains(query) {
        Some(20)
    } else {
        None
    }
}

fn search_building_score(record: &RawBuildingRecord, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(1);
    }
    let id = record.building_id.to_lowercase();
    let name = record.name_ko.to_lowercase();
    let description = record
        .description_ko
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    if id == query || name == query {
        Some(100)
    } else if id.starts_with(query) || name.starts_with(query) {
        Some(80)
    } else if id.contains(query) || name.contains(query) {
        Some(60)
    } else if record.category.to_lowercase().contains(query)
        || record.subcategory.to_lowercase().contains(query)
        || record.ui_category.to_lowercase().contains(query)
    {
        Some(40)
    } else if record.materials.iter().any(|material| {
        material.item_id.to_lowercase().contains(query)
            || material.name_ko.to_lowercase().contains(query)
    }) || record.production_methods.iter().any(|method| {
        method.item_id.to_lowercase().contains(query)
            || method.item_name_ko.to_lowercase().contains(query)
    }) {
        Some(30)
    } else if description.contains(query) {
        Some(20)
    } else {
        None
    }
}

fn search_technology_score(record: &RawTechnologyRecord, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(1);
    }
    let id = record.technology_id.to_lowercase();
    let name = record.name_ko.to_lowercase();
    let description = record
        .description_ko
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    if id == query || name == query {
        Some(100)
    } else if id.starts_with(query) || name.starts_with(query) {
        Some(80)
    } else if id.contains(query) || name.contains(query) {
        Some(60)
    } else if record
        .unlock_building_ids
        .iter()
        .chain(record.unlock_item_ids.iter())
        .any(|value| value.to_lowercase().contains(query))
    {
        Some(30)
    } else if description.contains(query) {
        Some(20)
    } else {
        None
    }
}

fn search_shop_score(record: &RawShopGroupRecord, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(1);
    }
    let id = record.shop_group_id.to_lowercase();
    let currency_id = record.currency_item_id.to_lowercase();
    let currency_name = record.currency_name_ko.to_lowercase();
    if id == query || currency_id == query || currency_name == query {
        Some(100)
    } else if id.starts_with(query)
        || currency_id.starts_with(query)
        || currency_name.starts_with(query)
    {
        Some(80)
    } else if id.contains(query) || currency_id.contains(query) || currency_name.contains(query) {
        Some(60)
    } else if record.products.iter().any(|product| {
        product.item_id.to_lowercase().contains(query)
            || product.item_name_ko.to_lowercase().contains(query)
    }) {
        Some(40)
    } else {
        None
    }
}

fn shop_product_matches(record: &RawShopProductRecord, query: &str) -> bool {
    record.item_id.to_lowercase().contains(query)
        || record.item_name_ko.to_lowercase().contains(query)
}

fn exact_response<T>(
    request: Request,
    kind: &'static str,
    records: Vec<T>,
) -> ExactCatalogResponse<T> {
    let matched_count = records.len();
    let records = records.into_iter().take(request.limit).collect::<Vec<_>>();
    ExactCatalogResponse {
        schema_version: SCHEMA_VERSION,
        worker_id: "pal-catalog-worker-v1",
        status: "ok",
        query: request.query.trim().to_owned(),
        matched_count,
        returned_count: records.len(),
        quality: "exact",
        source_id: "game-files:steam:24575825:world-catalog",
        kind,
        game_build_id: ITEM_CATALOG_GAME_BUILD_ID,
        records,
        warnings: Vec::new(),
    }
}

fn search_atom_score(record: &SpeciesRecord, query: &str) -> Option<u8> {
    let id = record.species_id.to_lowercase();
    let name = record.name_ko.as_deref().unwrap_or_default().to_lowercase();
    let dex = record.paldex_number.map(|value| value.to_string());
    let dex_query = query.strip_prefix('#').unwrap_or(query);
    if id == query || name == query || dex.as_deref() == Some(dex_query) {
        return Some(100);
    }
    if id.starts_with(query) || name.starts_with(query) {
        return Some(80);
    }
    if id.contains(query) || name.contains(query) {
        return Some(60);
    }
    if record.elements.iter().any(|element| {
        element.id.to_lowercase().contains(query) || element.name_ko.to_lowercase().contains(query)
    }) {
        return Some(40);
    }
    if record.work_suitability.iter().any(|work| {
        work.id.to_lowercase().contains(query) || work.name_ko.to_lowercase().contains(query)
    }) {
        return Some(30);
    }
    if record.learned_skills.iter().any(|skill| {
        skill.skill_id.to_lowercase().contains(query)
            || skill.name_ko.to_lowercase().contains(query)
    }) {
        return Some(20);
    }
    None
}

fn is_search_stopword(token: &str) -> bool {
    matches!(
        token,
        "알려줘"
            | "추천해줘"
            | "비교해줘"
            | "뭐가"
            | "어떤"
            | "가장"
            | "좋은"
            | "좋아"
            | "수치"
            | "비교"
            | "검색"
            | "팰월드"
    )
}

fn verified_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    expected_sha256: &str,
) -> Result<T, String> {
    let bytes = read_bounded(path)?;
    if sha256_hex(&bytes) != expected_sha256 {
        return Err(format!(
            "검증된 정적 데이터의 해시가 일치하지 않습니다: {}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("catalog")
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| "정적 데이터 형식이 올바르지 않습니다.".to_owned())
}

fn verified_json_object(
    path: &Path,
    expected_sha256: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let bytes = read_bounded(path)?;
    if sha256_hex(&bytes) != expected_sha256 {
        return Err(format!(
            "검증된 정적 데이터와 해시가 일치하지 않습니다: {}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("catalog")
        ));
    }
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|_| "정적 데이터 형식이 올바르지 않습니다.".to_owned())?
        .as_object()
        .cloned()
        .ok_or_else(|| "정적 데이터가 객체가 아닙니다.".to_owned())
}

fn localized_map(path: &Path, expected_sha256: &str) -> Result<BTreeMap<String, String>, String> {
    Ok(localized_text_map(path, expected_sha256)?
        .into_iter()
        .map(|(id, text)| (id, text.name_ko))
        .collect())
}

fn localized_text_map(
    path: &Path,
    expected_sha256: &str,
) -> Result<BTreeMap<String, LocalizedText>, String> {
    Ok(verified_json_object(path, expected_sha256)?
        .into_iter()
        .filter_map(|(id, value)| {
            let name = value
                .get("localized_name")
                .and_then(serde_json::Value::as_str)?
                .trim();
            (!name.is_empty()).then(|| {
                let description_ko = value
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|description| !description.is_empty())
                    .map(str::to_owned);
                (
                    id,
                    LocalizedText {
                        name_ko: name.to_owned(),
                        description_ko,
                    },
                )
            })
        })
        .collect())
}

fn validate_korean_ui_document(document: &VerifiedKoreanUiDocument) -> Result<(), String> {
    const SOURCE_PATH: &str = "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_UI_Common_Text_Common";
    if document.schema_version != SCHEMA_VERSION
        || document.game_build_id != ITEM_CATALOG_GAME_BUILD_ID
        || !document.verified
        || document.language != "ko"
        || document.localization_origin != "game_l10n"
        || document.machine_translation_allowed
        || document.source.package_path != SOURCE_PATH
        || document.source.row_count != 3175
        || document.elements.len() != 9
        || document.work_suitability.len() != 13
        || document.item_categories.len() != 12
    {
        return Err("한국어 UI 원문 데이터의 출처 계약이 올바르지 않습니다.".to_owned());
    }
    let valid = document.elements.values().all(|text| {
        text.source_row_key.starts_with("COMMON_ELEMENT_NAME_")
            && !text.name_ko.trim().is_empty()
            && text.name_ko != "ko_Text"
    }) && document.work_suitability.values().all(|text| {
        text.source_row_key.starts_with("COMMON_WORK_SUITABILITY_")
            && !text.name_ko.trim().is_empty()
            && text.name_ko != "ko_Text"
    }) && document.item_categories.values().all(|text| {
        text.source_row_key.starts_with("COMMON_ITEMTYPE_A_")
            && !text.name_ko.trim().is_empty()
            && text.name_ko != "ko_Text"
    });
    valid
        .then_some(())
        .ok_or_else(|| "한국어 UI 원문 행이 올바르지 않습니다.".to_owned())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|_| "정적 데이터 파일이 없습니다.".to_owned())?;
    if !metadata.is_file() || metadata.len() > MAX_DATA_BYTES {
        return Err("정적 데이터 파일 크기가 올바르지 않습니다.".to_owned());
    }
    fs::read(path).map_err(|_| "정적 데이터 파일을 읽지 못했습니다.".to_owned())
}

fn normalize_id(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> RawCatalog {
        RawCatalog::load(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/save-parser"),
        )
        .unwrap()
    }

    fn species_search(catalog: &RawCatalog, query: &str, limit: usize) -> Response {
        catalog
            .search_species(Request {
                schema_version: 1,
                kind: CatalogKind::Species,
                query: query.to_owned(),
                limit,
            })
            .unwrap()
    }

    #[test]
    fn searches_species_by_korean_name_dex_work_and_element() {
        let catalog = catalog();
        for (query, expected_id) in [
            ("페스키", "SkyDragon"),
            ("#124", "SkyDragon"),
            ("채굴", "SkyDragon"),
            ("용", "SkyDragon"),
        ] {
            let response = species_search(&catalog, query, 100);
            assert!(
                response
                    .records
                    .iter()
                    .any(|record| record.species_id == expected_id),
                "query={query}"
            );
        }
    }

    #[test]
    fn exact_species_result_contains_stats_work_and_localized_skills() {
        let catalog = catalog();
        let response = species_search(&catalog, "페스키", 10);
        let pal = response.records.first().unwrap();

        assert_eq!(pal.species_id, "SkyDragon");
        assert_eq!(pal.paldex_number, Some(124));
        assert_eq!(pal.ride_sprint_speed, Some(950));
        assert_eq!(pal.stamina, Some(220));
        assert!(
            pal.work_suitability
                .iter()
                .any(|work| work.name_ko == "채굴" && work.level == 4)
        );
        assert!(
            pal.learned_skills
                .iter()
                .any(|skill| skill.name_ko == "용의 숨결")
        );
        assert!(
            pal.elements
                .iter()
                .any(|element| element.id == "Dragon" && element.name_ko == "용 속성")
        );
        assert!(
            pal.work_suitability
                .iter()
                .any(|work| work.id == "Handcraft" && work.name_ko == "수작업")
        );
        assert_eq!(response.quality, "unknown");
    }

    #[test]
    fn natural_korean_question_reuses_structured_search_terms() {
        let catalog = catalog();
        let response = species_search(&catalog, "페스키 수치 비교해줘", 10);
        assert_eq!(response.records.first().unwrap().species_id, "SkyDragon");

        let work_response = species_search(&catalog, "채굴 좋은 팰 추천해줘", 10);
        assert!(work_response.records.iter().all(|record| {
            record
                .work_suitability
                .iter()
                .any(|work| work.name_ko == "채굴")
        }));
    }

    #[test]
    fn request_bounds_are_fail_closed() {
        assert!(read_request(&b""[..]).is_err());
        assert!(
            read_request(br#"{"schema_version":1,"query":"","limit":4097}"#.as_slice()).is_err()
        );
    }

    #[test]
    fn service_mode_reuses_one_catalog_for_multiple_requests() {
        let input = concat!(
            "{\"schema_version\":1,\"kind\":\"species\",\"query\":\"페스키\",\"limit\":10}\n",
            "{\"schema_version\":1,\"kind\":\"item\",\"query\":\"고급 팰 기름\",\"limit\":10}\n"
        );
        let mut output = Vec::new();
        serve_requests(&catalog(), input.as_bytes(), &mut output).unwrap();
        let lines = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["kind"], "species");
        assert_eq!(lines[0]["records"][0]["species_id"], "SkyDragon");
        assert_eq!(lines[1]["kind"], "item");
        assert_eq!(lines[1]["records"][0]["item_id"], "PalOil");
    }

    #[test]
    fn searches_active_skills_by_korean_name_and_element() {
        let catalog = catalog();
        let response = catalog.search_active_skills(Request {
            schema_version: 1,
            kind: CatalogKind::ActiveSkill,
            query: "산성비".to_owned(),
            limit: 10,
        });
        let skill = response.records.first().expect("산성비 검색 결과");
        assert_eq!(skill.skill_id, "AcidRain");
        assert_eq!(skill.name_ko, "산성비");
        assert!(skill.description_ko.as_deref().is_some_and(|value| {
            value.contains("산성 구름") && value.contains("산성비")
        }));

        let element_response = catalog.search_active_skills(Request {
            schema_version: 1,
            kind: CatalogKind::ActiveSkill,
            query: "용".to_owned(),
            limit: 50,
        });
        assert!(element_response.records.iter().any(|record| {
            record.element.as_deref() == Some("Dragon")
                || record.element_ko.as_deref() == Some("용")
        }));
    }

    #[test]
    fn searches_passives_by_korean_name_and_description() {
        let catalog = catalog();
        let response = catalog.search_passives(Request {
            schema_version: 1,
            kind: CatalogKind::Passive,
            query: "신속".to_owned(),
            limit: 10,
        });
        let passive = response.records.first().expect("신속 검색 결과");
        assert_eq!(passive.passive_id, "MoveSpeed_up_3");
        assert_eq!(passive.name_ko, "신속");
        assert_eq!(
            passive.description_ko.as_deref(),
            Some("이동 속도 상승 {EffectValue1}%")
        );

        let description_response = catalog.search_passives(Request {
            schema_version: 1,
            kind: CatalogKind::Passive,
            query: "이동 속도".to_owned(),
            limit: 20,
        });
        assert!(
            description_response
                .records
                .iter()
                .any(|record| record.name_ko == "신속")
        );
    }

    #[test]
    fn bounded_full_catalog_query_supports_owned_pal_analysis() {
        let encoded = br#"{"schema_version":1,"query":"","limit":500}"#.as_slice();
        let request = read_request(encoded).expect("the bounded full catalog request is valid");
        let response = catalog()
            .search_species(request)
            .expect("full catalog search");

        assert!(response.returned_count > 100);
        assert!(response.returned_count <= MAX_RESULTS);
    }

    #[test]
    fn searches_exact_build_items_by_korean_name_and_internal_id() {
        let catalog = catalog();
        for (query, expected_id, expected_category) in [
            ("고급 팰 기름", "PalOil", "소재"),
            ("PalOil", "PalOil", "소재"),
            ("순수한 석영", "Quartz", "소재"),
            ("화염방사기", "FlameThrower", "무기"),
            ("FlameThrower", "FlameThrower", "무기"),
        ] {
            let response = catalog.search_items(Request {
                schema_version: 1,
                kind: CatalogKind::Item,
                query: query.to_owned(),
                limit: 20,
            });
            assert_eq!(response.quality, "exact");
            assert_eq!(response.game_build_id, "steam:24575825");
            assert_eq!(response.records.first().unwrap().item_id, expected_id);
            assert_eq!(
                response.records.first().unwrap().category_ko.as_deref(),
                Some(expected_category)
            );
            assert!(response.warnings.is_empty());
        }
    }

    #[test]
    fn item_results_include_localized_recipe_and_drop_context() {
        let catalog = catalog();
        let missile = catalog.search_items(Request {
            schema_version: 1,
            kind: CatalogKind::Item,
            query: "미사일탄".to_owned(),
            limit: 10,
        });
        let item = missile.records.first().unwrap();
        assert_eq!(item.item_id, "MissileBullet");
        assert_eq!(item.recipe_count, 1);
        assert!(
            item.recipes[0]
                .ingredients
                .iter()
                .all(|ingredient| !ingredient.name_ko.is_empty())
        );

        let oil = catalog.search_items(Request {
            schema_version: 1,
            kind: CatalogKind::Item,
            query: "고급 팰 기름".to_owned(),
            limit: 10,
        });
        let item = oil.records.first().unwrap();
        assert!(item.pal_drop_count > 0);
        assert!(item.pal_drops.iter().all(|drop| drop.probability_ppm > 0));
        assert!(item.pal_drops.iter().any(|drop| drop.pal_name_ko.is_some()));
    }

    #[test]
    fn searches_exact_build_buildings_technologies_and_shops() {
        let catalog = catalog();

        let buildings = catalog.search_buildings(Request {
            schema_version: 1,
            kind: CatalogKind::Building,
            query: "고대 문명 화로".to_owned(),
            limit: 10,
        });
        assert_eq!(buildings.quality, "exact");
        assert_eq!(buildings.records[0].building_id, "AncientBlastFurnace");
        assert_eq!(buildings.records[0].materials.len(), 4);

        let technologies = catalog.search_technologies(Request {
            schema_version: 1,
            kind: CatalogKind::Technology,
            query: "AI 코어".to_owned(),
            limit: 10,
        });
        assert_eq!(technologies.records[0].technology_id, "AIcore");
        assert_eq!(technologies.records[0].unlock_item_ids, ["AIcore"]);

        let related_name = catalog.search_technologies(Request {
            schema_version: 1,
            kind: CatalogKind::Technology,
            query: "AncientBlastFurnace".to_owned(),
            limit: 10,
        });
        assert_eq!(related_name.records[0].name_ko, "고대 문명 화로");

        let shops = catalog.search_shops(Request {
            schema_version: 1,
            kind: CatalogKind::Shop,
            query: "현상범 토벌 증표".to_owned(),
            limit: 10,
        });
        assert_eq!(shops.records[0].shop_group_id, "Bounty_Shop_1");
        assert!(
            shops.records[0]
                .products
                .iter()
                .any(|product| product.item_id == "TechnologyBook_G1")
        );
    }

    #[test]
    fn hides_internal_records_without_user_facing_original_art() {
        let catalog = catalog();

        let pal = catalog
            .search_species(Request {
                schema_version: 1,
                kind: CatalogKind::Species,
                query: String::new(),
                limit: MAX_RESULTS,
            })
            .expect("species search");
        assert!(
            pal.records
                .iter()
                .all(|record| record.species_id != "RAID_YakushimaBoss002")
        );

        let building = catalog.search_buildings(Request {
            schema_version: 1,
            kind: CatalogKind::Building,
            query: String::new(),
            limit: MAX_RESULTS,
        });
        assert!(
            building
                .records
                .iter()
                .all(|record| record.building_id != "FastTravelPoint")
        );
    }
}
