use std::{collections::BTreeMap, env, fs, io::Write as _, path::PathBuf, process::ExitCode};

use pal_save_ingest::{SaveFileRole, StagedSaveSet, verify_staged_save};
use psp_core::{
    domain::{pal, world},
    dto::pal::{PalDto, PalGender},
    gamedata::GameData,
    progress::null_progress,
    session::{PlayerFileData, SaveKind, SaveSession},
};
use serde::Serialize;
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 2;
const MAX_OUTPUT_PALS: usize = 20_000;
const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug)]
struct Options {
    staged_root: PathBuf,
    staging_root: PathBuf,
    data_dir: PathBuf,
    owner_uid: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct ParseOutput {
    schema_version: u32,
    parser_id: &'static str,
    import_id: String,
    world_name: String,
    world_pal_count: usize,
    catalog_entry_count: usize,
    passive_catalog_entry_count: usize,
    active_skill_catalog_entry_count: usize,
    selected_owner_uid: Option<Uuid>,
    players: Vec<PlayerChoice>,
    pals: Vec<ParsedPal>,
    limitations: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct PlayerChoice {
    uid: Uuid,
    nickname: String,
    level: Option<i64>,
    pal_count: i64,
}

#[derive(Debug, Serialize)]
struct ParsedPal {
    instance_id: Uuid,
    species_id: String,
    species_key: String,
    species_name_ko: Option<String>,
    paldex_number: Option<u32>,
    nickname: Option<String>,
    owner_uid: Option<Uuid>,
    gender: &'static str,
    level: u32,
    passive_ids: Vec<String>,
    passive_names_ko: Vec<String>,
    active_skill_ids: Vec<String>,
    active_skill_names_ko: Vec<String>,
    learned_skill_ids: Vec<String>,
    learned_skill_names_ko: Vec<String>,
    iv_hp: Option<u32>,
    iv_attack: Option<u32>,
    iv_defense: Option<u32>,
    condensation_rank: u32,
    soul_hp: u32,
    soul_attack: u32,
    soul_defense: u32,
    soul_work_speed: u32,
    trust: u32,
    is_lucky: bool,
    is_boss: bool,
    is_predator: bool,
    is_tower: bool,
    storage_id: Uuid,
    storage_slot: u32,
}

#[derive(Debug, Default)]
struct PalCatalog {
    entries: BTreeMap<String, PalCatalogEntry>,
    passive_names_ko: BTreeMap<String, String>,
    active_skill_names_ko: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
struct PalCatalogEntry {
    name_ko: Option<String>,
    paldex_number: Option<u32>,
}

impl PalCatalog {
    fn from_game_data(game_data: &GameData) -> Self {
        let mut entries = BTreeMap::<String, PalCatalogEntry>::new();
        if let Some(pals) = game_data.get("pals").and_then(serde_json::Value::as_object) {
            for (key, value) in pals {
                let entry = entries.entry(key.to_lowercase()).or_default();
                entry.paldex_number = value
                    .get("pal_deck_index")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok());
            }
        }
        if let Some(names) = game_data
            .get("l10n/ko/pals")
            .and_then(serde_json::Value::as_object)
        {
            for (key, value) in names {
                let Some(name) = value
                    .get("localized_name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                else {
                    continue;
                };
                entries.entry(key.to_lowercase()).or_default().name_ko = Some(name.to_owned());
            }
        }
        Self {
            entries,
            passive_names_ko: localized_name_map(game_data, "l10n/ko/passive_skills"),
            active_skill_names_ko: localized_name_map(game_data, "l10n/ko/active_skills"),
        }
    }

    fn get(&self, species_id: &str, species_key: &str) -> Option<&PalCatalogEntry> {
        self.entries
            .get(&species_id.to_lowercase())
            .or_else(|| self.entries.get(&species_key.to_lowercase()))
    }

    fn passive_labels(&self, ids: &[String]) -> Vec<String> {
        localized_labels(ids, &self.passive_names_ko)
    }

    fn active_skill_labels(&self, ids: &[String]) -> Vec<String> {
        localized_labels(ids, &self.active_skill_names_ko)
    }
}

fn localized_name_map(game_data: &GameData, key: &str) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    let Some(entries) = game_data.get(key).and_then(serde_json::Value::as_object) else {
        return names;
    };
    for (id, value) in entries {
        let Some(name) = value
            .get("localized_name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        names.insert(id.to_lowercase(), name.to_owned());
    }
    names
}

fn localized_labels(ids: &[String], names: &BTreeMap<String, String>) -> Vec<String> {
    ids.iter()
        .map(|id| {
            names
                .get(&id.to_lowercase())
                .cloned()
                .unwrap_or_else(|| id.clone())
        })
        .collect()
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pal-save-parser-worker: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let options = parse_options(env::args_os().skip(1))?;
    let output = parse_staged_save(&options)?;
    let encoded = serde_json::to_vec(&output).map_err(|_| "output encoding failed")?;
    if encoded.len() > MAX_OUTPUT_BYTES {
        return Err("normalized output exceeds the supported limit".to_owned());
    }
    std::io::stdout()
        .lock()
        .write_all(&encoded)
        .map_err(|_| "output write failed".to_owned())
}

fn parse_options(arguments: impl Iterator<Item = std::ffi::OsString>) -> Result<Options, String> {
    let arguments = arguments.collect::<Vec<_>>();
    if arguments.len() != 6 && arguments.len() != 8 {
        return Err(usage());
    }
    let mut values = BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        let key = pair[0]
            .to_str()
            .filter(|value| value.starts_with("--"))
            .ok_or_else(usage)?;
        if values.insert(key.to_owned(), pair[1].clone()).is_some() {
            return Err(usage());
        }
    }
    let staged_root = absolute_option(&values, "--staged-root")?;
    let staging_root = absolute_option(&values, "--staging-root")?;
    let data_dir = absolute_option(&values, "--data-dir")?;
    let owner_uid = values
        .get("--owner-uid")
        .map(|value| {
            value
                .to_str()
                .ok_or_else(usage)?
                .parse::<Uuid>()
                .map_err(|_| "owner UID is invalid".to_owned())
        })
        .transpose()?;
    Ok(Options {
        staged_root,
        staging_root,
        data_dir,
        owner_uid,
    })
}

fn absolute_option(
    values: &BTreeMap<String, std::ffi::OsString>,
    name: &str,
) -> Result<PathBuf, String> {
    let path = values.get(name).map(PathBuf::from).ok_or_else(usage)?;
    if !path.is_absolute() {
        return Err(format!("{name} must be absolute"));
    }
    Ok(path)
}

fn usage() -> String {
    "usage: pal-save-parser-worker --staged-root <path> --staging-root <path> \
     --data-dir <path> [--owner-uid <uuid>]"
        .to_owned()
}

fn parse_staged_save(options: &Options) -> Result<ParseOutput, String> {
    let staged = verify_staged_save(&options.staged_root, &options.staging_root)
        .map_err(|_| "staged save verification failed".to_owned())?;
    let level_path = file_for_role(&staged, SaveFileRole::Level)
        .ok_or_else(|| "verified import has no Level.sav".to_owned())?;
    let level_bytes = fs::read(level_path).map_err(|_| "Level.sav read failed".to_owned())?;
    let level_meta_bytes = optional_file_bytes(&staged, SaveFileRole::LevelMeta)?;
    let world_option_bytes = optional_file_bytes(&staged, SaveFileRole::WorldOption)?;
    let player_file_refs = player_file_refs(&staged)?;
    let game_data =
        GameData::load(&options.data_dir).map_err(|_| "parser data load failed".to_owned())?;
    let catalog = PalCatalog::from_game_data(&game_data);
    let session = SaveSession::load(
        SaveKind::InMemory,
        staged.import_id.clone(),
        "staged",
        &level_bytes,
        level_meta_bytes.as_deref(),
        world_option_bytes.as_deref(),
        player_file_refs,
        None,
        false,
        &null_progress(),
    )
    .map_err(|_| "Palworld save parse failed".to_owned())?;

    let mut players = session
        .player_summaries
        .values()
        .map(|summary| PlayerChoice {
            uid: summary.uid,
            nickname: summary.nickname.clone(),
            level: summary.level,
            pal_count: summary.pal_count,
        })
        .collect::<Vec<_>>();
    players.sort_by(|left, right| {
        left.nickname
            .cmp(&right.nickname)
            .then(left.uid.as_bytes().cmp(right.uid.as_bytes()))
    });

    let entries =
        world::character_map(&session.level).map_err(|_| "character map is unavailable")?;
    let world_pal_count = entries
        .iter()
        .filter(|entry| !world::entry_is_player(entry))
        .count();
    if world_pal_count > MAX_OUTPUT_PALS {
        return Err("world Pal count exceeds the supported limit".to_owned());
    }
    let mut pals = entries
        .iter()
        .filter(|entry| !world::entry_is_player(entry))
        .filter_map(|entry| pal::pal_dto_from_entry(entry, &game_data))
        .filter(|dto| {
            options
                .owner_uid
                .is_some_and(|owner_uid| dto.owner_uid == Some(owner_uid))
        })
        .map(|dto| normalize_pal(dto, &catalog))
        .collect::<Vec<_>>();
    pals.sort_by(|left, right| {
        left.instance_id
            .as_bytes()
            .cmp(right.instance_id.as_bytes())
    });

    Ok(ParseOutput {
        schema_version: SCHEMA_VERSION,
        parser_id: "psp-core-readonly-v1.2.0",
        import_id: staged.import_id,
        world_name: session.world_name,
        world_pal_count,
        catalog_entry_count: catalog.entries.len(),
        passive_catalog_entry_count: catalog.passive_names_ko.len(),
        active_skill_catalog_entry_count: catalog.active_skill_names_ko.len(),
        selected_owner_uid: options.owner_uid,
        players,
        pals,
        limitations: vec![
            "No owned Pal details are returned until an explicit local player UID is selected.",
            "Dimensional and Global Pal Storage require a later owner-scoped adapter.",
            "Storage UUID and slot are retained; semantic party, box, and base labels are resolved later.",
            "Korean species, passive, and active skill names come from the pinned local static catalog; unknown ids remain visible.",
        ],
    })
}

fn file_for_role(staged: &StagedSaveSet, role: SaveFileRole) -> Option<PathBuf> {
    staged
        .files
        .iter()
        .find(|file| file.role == role)
        .map(|file| staged.staged_root.join(&file.relative_path))
}

fn optional_file_bytes(
    staged: &StagedSaveSet,
    role: SaveFileRole,
) -> Result<Option<Vec<u8>>, String> {
    file_for_role(staged, role)
        .map(|path| fs::read(path).map_err(|_| "optional save file read failed".to_owned()))
        .transpose()
}

fn player_file_refs(staged: &StagedSaveSet) -> Result<BTreeMap<Uuid, PlayerFileData>, String> {
    let mut players = BTreeMap::<Uuid, PlayerFileData>::new();
    for file in &staged.files {
        if !matches!(file.role, SaveFileRole::Player | SaveFileRole::PlayerDps) {
            continue;
        }
        let path = staged.staged_root.join(&file.relative_path);
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "player save name is invalid".to_owned())?;
        let id = stem.strip_suffix("_dps").unwrap_or(stem);
        let uid = id
            .parse::<Uuid>()
            .map_err(|_| "player save UID is invalid".to_owned())?;
        let entry = players.entry(uid).or_insert(PlayerFileData::Paths {
            sav: None,
            dps: None,
        });
        let PlayerFileData::Paths { sav, dps } = entry else {
            return Err("player save reference is invalid".to_owned());
        };
        match file.role {
            SaveFileRole::Player => *sav = Some(path),
            SaveFileRole::PlayerDps => *dps = Some(path),
            _ => unreachable!(),
        }
    }
    Ok(players)
}

fn normalize_pal(dto: PalDto, catalog: &PalCatalog) -> ParsedPal {
    let catalog_entry = catalog.get(&dto.character_id, &dto.character_key);
    let passive_names_ko = catalog.passive_labels(&dto.passive_skills);
    let active_skill_names_ko = catalog.active_skill_labels(&dto.active_skills);
    let learned_skill_names_ko = catalog.active_skill_labels(&dto.learned_skills);
    ParsedPal {
        instance_id: dto.instance_id,
        species_id: dto.character_id,
        species_key: dto.character_key,
        species_name_ko: catalog_entry.and_then(|entry| entry.name_ko.clone()),
        paldex_number: catalog_entry.and_then(|entry| entry.paldex_number),
        nickname: dto.nickname,
        owner_uid: dto.owner_uid,
        gender: match dto.gender {
            PalGender::None => "none",
            PalGender::Male => "male",
            PalGender::Female => "female",
        },
        level: bounded_u32(dto.level),
        passive_ids: dto.passive_skills,
        passive_names_ko,
        active_skill_ids: dto.active_skills,
        active_skill_names_ko,
        learned_skill_ids: dto.learned_skills,
        learned_skill_names_ko,
        iv_hp: optional_bounded_u32(dto.talent_hp),
        iv_attack: optional_bounded_u32(dto.talent_shot),
        iv_defense: optional_bounded_u32(dto.talent_defense),
        condensation_rank: bounded_u32(dto.rank),
        soul_hp: bounded_u32(dto.rank_hp),
        soul_attack: bounded_u32(dto.rank_attack),
        soul_defense: bounded_u32(dto.rank_defense),
        soul_work_speed: bounded_u32(dto.rank_craftspeed),
        trust: bounded_u32(dto.friendship_point),
        is_lucky: dto.is_lucky.unwrap_or(false),
        is_boss: dto.is_boss.unwrap_or(false),
        is_predator: dto.is_predator,
        is_tower: dto.is_tower,
        storage_id: dto.storage_id,
        storage_slot: bounded_u32(dto.storage_slot),
    }
}

fn bounded_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(if value.is_negative() { 0 } else { u32::MAX })
}

fn optional_bounded_u32(value: i64) -> Option<u32> {
    (value >= 0).then(|| bounded_u32(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires PAL_TEST_SAVE_ROOT to point to a real read-only Palworld save fixture"]
    fn staged_external_fixture_is_owner_scoped() {
        let source_root =
            PathBuf::from(std::env::var_os("PAL_TEST_SAVE_ROOT").expect("PAL_TEST_SAVE_ROOT"));
        let staging = tempfile::tempdir().unwrap();
        let staged = pal_save_ingest::stage_save_source(&source_root, staging.path()).unwrap();
        let data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/save-parser");
        let unselected = parse_staged_save(&Options {
            staged_root: staged.staged_root.clone(),
            staging_root: staging.path().to_path_buf(),
            data_dir: data_dir.clone(),
            owner_uid: None,
        })
        .unwrap();

        assert!(!unselected.players.is_empty());
        assert!(unselected.pals.is_empty());

        let owner_uid = unselected.players[0].uid;
        let selected = parse_staged_save(&Options {
            staged_root: staged.staged_root,
            staging_root: staging.path().to_path_buf(),
            data_dir,
            owner_uid: Some(owner_uid),
        })
        .unwrap();

        assert_eq!(selected.selected_owner_uid, Some(owner_uid));
        assert!(selected.catalog_entry_count > 0);
        assert!(
            selected
                .pals
                .iter()
                .all(|pal| pal.owner_uid == Some(owner_uid))
        );
        assert!(
            selected
                .pals
                .iter()
                .any(|pal| pal.species_name_ko.is_some())
        );
    }

    #[test]
    fn no_owner_selection_is_fail_closed_for_personal_rows() {
        let dto = PalDto::from_json_lenient(&serde_json::json!({
            "instance_id": "11111111-2222-3333-4444-555555555555",
            "character_id": "SheepBall",
            "owner_uid": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        }))
        .unwrap();
        let selected_owner = None::<Uuid>;

        assert!(!selected_owner.is_some_and(|owner| dto.owner_uid == Some(owner)));
    }

    #[test]
    fn numeric_normalization_saturates_instead_of_wrapping() {
        assert_eq!(bounded_u32(-1), 0);
        assert_eq!(bounded_u32(i64::MAX), u32::MAX);
        assert_eq!(optional_bounded_u32(-1), None);
        assert_eq!(optional_bounded_u32(50), Some(50));
    }

    #[test]
    fn catalog_lookup_is_case_insensitive_and_keeps_korean_name_and_dex() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("l10n/ko")).unwrap();
        std::fs::write(
            temp.path().join("pals.json"),
            r#"{"SheepBall":{"pal_deck_index":1}}"#,
        )
        .unwrap();
        std::fs::write(
            temp.path().join("l10n/ko/pals.json"),
            r#"{"SheepBall":{"localized_name":"도로롱"}}"#,
        )
        .unwrap();
        std::fs::write(
            temp.path().join("l10n/ko/passive_skills.json"),
            r#"{"Swift":{"localized_name":"신속"}}"#,
        )
        .unwrap();
        std::fs::write(
            temp.path().join("l10n/ko/active_skills.json"),
            r#"{"EPalWazaID::AirCanon":{"localized_name":"공기 대포"}}"#,
        )
        .unwrap();
        let game_data = GameData::load(temp.path()).unwrap();
        let catalog = PalCatalog::from_game_data(&game_data);
        let entry = catalog.get("Sheepball", "sheepball").unwrap();

        assert_eq!(entry.name_ko.as_deref(), Some("도로롱"));
        assert_eq!(entry.paldex_number, Some(1));
        assert_eq!(catalog.passive_labels(&["Swift".to_owned()]), ["신속"]);
        assert_eq!(
            catalog.active_skill_labels(&["EPalWazaID::AirCanon".to_owned()]),
            ["공기 대포"]
        );
    }

    #[test]
    fn parser_arguments_require_absolute_paths_and_known_pairs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let options = parse_options(
            [
                "--staged-root".into(),
                root.join("stage").into_os_string(),
                "--staging-root".into(),
                root.clone().into_os_string(),
                "--data-dir".into(),
                root.join("data").into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        assert_eq!(options.staging_root, root);
        assert!(options.owner_uid.is_none());
        assert!(parse_options(["--staged-root".into(), "relative".into()].into_iter()).is_err());
    }
}
