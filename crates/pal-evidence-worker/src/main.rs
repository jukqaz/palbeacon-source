use std::{
    env,
    io::{Read, Write as _},
    process::ExitCode,
};

use pal_companion_service::{AssistantEvidence, AssistantEvidenceKind, GroundedAssistantRequest};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const SCHEMA_VERSION: u32 = 1;
const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_CATALOG_RECORDS: usize = 512;
const MAX_OWNED_PALS: usize = 5_000;
const MAX_SELECTED_CATALOG: usize = 12;
const MAX_SELECTED_OWNED: usize = 16;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EvidenceRequest {
    schema_version: u32,
    question: String,
    game_build_id: String,
    catalog_source_id: String,
    import_id: String,
    #[serde(default)]
    catalog_records: Vec<CatalogRecord>,
    #[serde(default)]
    owned_pals: Vec<OwnedPal>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CatalogRecord {
    species_id: String,
    #[serde(default)]
    name_ko: Option<String>,
    #[serde(default)]
    paldex_number: Option<u32>,
    #[serde(default)]
    elements: Vec<LocalizedValue>,
    #[serde(default)]
    hp: Option<i64>,
    #[serde(default)]
    attack: Option<i64>,
    #[serde(default)]
    defense: Option<i64>,
    #[serde(default)]
    ride_sprint_speed: Option<i64>,
    #[serde(default)]
    stamina: Option<i64>,
    #[serde(default)]
    work_suitability: Vec<WorkSuitability>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LocalizedValue {
    name_ko: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WorkSuitability {
    name_ko: String,
    level: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OwnedPal {
    instance_id: String,
    species_id: String,
    #[serde(default)]
    species_name_ko: Option<String>,
    #[serde(default)]
    nickname: Option<String>,
    level: i64,
    #[serde(default)]
    passive_ids: Vec<String>,
    #[serde(default)]
    passive_names_ko: Vec<String>,
    #[serde(default)]
    iv_hp: Option<i64>,
    #[serde(default)]
    iv_attack: Option<i64>,
    #[serde(default)]
    iv_defense: Option<i64>,
}

#[derive(Debug, Serialize)]
struct EvidenceResponse {
    schema_version: u32,
    worker_id: &'static str,
    status: &'static str,
    answer: String,
    grounded_request: GroundedAssistantRequest,
    evidence: Vec<AssistantEvidence>,
    evidence_count: usize,
    prompt_digest_sha256: String,
    model_used: bool,
    warnings: Vec<&'static str>,
}

fn main() -> ExitCode {
    if env::args_os().len() != 1 {
        eprintln!("pal-evidence-worker: no arguments are supported");
        return ExitCode::from(2);
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pal-evidence-worker: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let request = read_request(std::io::stdin().lock())?;
    let response = build_response(request)?;
    let bytes = serde_json::to_vec(&response)
        .map_err(|_| "근거 결과를 인코딩하지 못했습니다.".to_owned())?;
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err("근거 결과가 허용 크기를 초과했습니다.".to_owned());
    }
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|_| "근거 결과를 전달하지 못했습니다.".to_owned())
}

fn read_request(mut input: impl Read) -> Result<EvidenceRequest, String> {
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "근거 요청을 읽지 못했습니다.".to_owned())?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("근거 요청 크기가 올바르지 않습니다.".to_owned());
    }
    let request: EvidenceRequest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("근거 요청 형식이 올바르지 않습니다: {error}"))?;
    if request.schema_version != SCHEMA_VERSION {
        return Err("지원하지 않는 근거 요청 버전입니다.".to_owned());
    }
    if request.catalog_records.len() > MAX_CATALOG_RECORDS
        || request.owned_pals.len() > MAX_OWNED_PALS
    {
        return Err("근거 입력 행 수가 허용 범위를 초과했습니다.".to_owned());
    }
    Ok(request)
}

fn build_response(mut request: EvidenceRequest) -> Result<EvidenceResponse, String> {
    let question = request.question.trim();
    let tokens = query_tokens(question);
    request.owned_pals.sort_by(|left, right| {
        right
            .level
            .cmp(&left.level)
            .then(left.species_id.cmp(&right.species_id))
            .then(left.instance_id.cmp(&right.instance_id))
    });
    let owned_species = request
        .owned_pals
        .iter()
        .map(|pal| normalize(&pal.species_id))
        .collect::<std::collections::BTreeSet<_>>();
    let mut selected_catalog = request.catalog_records.iter().collect::<Vec<_>>();
    selected_catalog.sort_by(|left, right| {
        catalog_priority(right, &tokens, &owned_species, question)
            .cmp(&catalog_priority(left, &tokens, &owned_species, question))
            .then(left.species_id.cmp(&right.species_id))
    });
    selected_catalog.truncate(MAX_SELECTED_CATALOG);
    let catalog_species = selected_catalog
        .iter()
        .map(|record| normalize(&record.species_id))
        .collect::<std::collections::BTreeSet<_>>();
    let mut evidence = Vec::new();

    if !request.owned_pals.is_empty() {
        evidence.push(AssistantEvidence {
            evidence_id: "owned-pal:summary".to_owned(),
            kind: AssistantEvidenceKind::OwnedPal,
            title: "선택한 캐릭터의 보유 팰".to_owned(),
            detail: format!(
                "읽기 전용 세이브 스냅샷에서 총 {}마리를 확인했습니다.",
                request.owned_pals.len()
            ),
            quality: "exact".to_owned(),
            source_ids: vec!["save:projection".to_owned()],
        });
    }

    for (index, record) in selected_catalog.iter().enumerate() {
        evidence.push(AssistantEvidence {
            evidence_id: format!("catalog:{index:02}"),
            kind: AssistantEvidenceKind::Catalog,
            title: catalog_title(record),
            detail: catalog_detail(record),
            quality: "unknown".to_owned(),
            source_ids: vec!["catalog:pinned-alpha".to_owned()],
        });
    }

    let relevant_owned = request
        .owned_pals
        .iter()
        .filter(|pal| {
            catalog_species.contains(&normalize(&pal.species_id)) || owned_matches(pal, &tokens)
        })
        .take(MAX_SELECTED_OWNED)
        .collect::<Vec<_>>();
    for (index, pal) in relevant_owned.iter().enumerate() {
        evidence.push(AssistantEvidence {
            evidence_id: format!("owned-pal:{index:02}"),
            kind: AssistantEvidenceKind::OwnedPal,
            title: owned_title(pal),
            detail: owned_detail(pal),
            quality: "exact".to_owned(),
            source_ids: vec!["save:projection".to_owned()],
        });
    }

    if !request.catalog_records.is_empty() {
        evidence.push(AssistantEvidence {
            evidence_id: "warning:catalog-build".to_owned(),
            kind: AssistantEvidenceKind::Warning,
            title: "정적 데이터 Build 확인 필요".to_owned(),
            detail:
                "고정 해시는 확인했지만 현재 서버 Build와 정적 데이터의 정확한 일치는 아직 증명되지 않았습니다."
                    .to_owned(),
            quality: "unknown".to_owned(),
            source_ids: vec!["catalog:pinned-alpha".to_owned()],
        });
    }

    let grounded = GroundedAssistantRequest {
        question: request.question,
        game_build_id: normalized_build_id(&request.game_build_id),
        dataset_manifest_id_hex: digest_hex(request.catalog_source_id.as_bytes()),
        projection_id_hex: projection_digest(&request.import_id, &request.owned_pals),
        evidence,
    };
    let prompt = grounded
        .prompt()
        .map_err(|error| format!("근거 요청을 검증하지 못했습니다: {error}"))?;
    let answer = deterministic_answer(
        grounded.question.as_str(),
        &selected_catalog,
        &request.owned_pals,
        &relevant_owned,
    )
    .unwrap_or_else(|| {
        grounded
            .fallback()
            .map(|fallback| fallback.answer)
            .unwrap_or_else(|_| "확인된 근거를 요약하지 못했습니다.".to_owned())
    });
    let prompt_digest_sha256 = digest_hex(format!("{}\n{}", prompt.system, prompt.user).as_bytes());
    let status = if grounded.evidence.is_empty() {
        "no_evidence"
    } else {
        "evidence_ready"
    };
    Ok(EvidenceResponse {
        schema_version: SCHEMA_VERSION,
        worker_id: "pal-evidence-worker-v3",
        status,
        answer,
        grounded_request: grounded.clone(),
        evidence_count: grounded.evidence.len(),
        evidence: grounded.evidence,
        prompt_digest_sha256,
        model_used: false,
        warnings: vec![
            "DETERMINISTIC_LOCAL_COACH",
            "AI_MODEL_NOT_CONNECTED",
            "CATALOG_BUILD_MATCH_NOT_PROVEN",
            "PASSIVE_EFFECTS_ARE_ADDITIVE_ESTIMATES",
        ],
    })
}

fn catalog_priority(
    record: &CatalogRecord,
    tokens: &[String],
    owned_species: &std::collections::BTreeSet<String>,
    question: &str,
) -> i64 {
    let normalized_species = normalize(&record.species_id);
    let owned = owned_species.contains(&normalized_species);
    let mount_question = is_mount_question(question);
    let searchable = format!(
        "{} {} {} {}",
        record.species_id,
        record.name_ko.as_deref().unwrap_or_default(),
        record
            .elements
            .iter()
            .map(|value| value.name_ko.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        record
            .work_suitability
            .iter()
            .map(|work| work.name_ko.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    )
    .to_lowercase();
    let token_matches = tokens
        .iter()
        .filter(|token| searchable.contains(token.as_str()))
        .count() as i64;
    let mount_score = if mount_question && owned {
        record.ride_sprint_speed.unwrap_or_default().max(0)
    } else {
        0
    };
    let owned_context =
        is_mount_question(question) || is_owned_question(question) || is_passive_question(question);
    i64::from(owned && owned_context) * 100_000
        + i64::from(mount_question && record.ride_sprint_speed.is_some()) * 50_000
        + token_matches * 10_000
        + mount_score
}

fn deterministic_answer(
    question: &str,
    catalog: &[&CatalogRecord],
    all_owned: &[OwnedPal],
    relevant_owned: &[&OwnedPal],
) -> Option<String> {
    if is_mount_question(question) {
        return Some(mount_answer(catalog, all_owned));
    }
    if is_owned_question(question) || is_passive_question(question) {
        return Some(owned_answer(relevant_owned, all_owned.len()));
    }
    if !catalog.is_empty() {
        return Some(catalog_answer(catalog, relevant_owned));
    }
    None
}

#[derive(Debug)]
struct MountCandidate<'a> {
    catalog_index: usize,
    record: &'a CatalogRecord,
    owned_count: usize,
    speed_bonus_percent: i64,
    stamina_bonus_percent: i64,
    effective_sprint: i64,
    effective_stamina: i64,
}

fn mount_answer(catalog: &[&CatalogRecord], owned: &[OwnedPal]) -> String {
    let mut candidates = catalog
        .iter()
        .enumerate()
        .filter_map(|(catalog_index, record)| {
            let base_sprint = record.ride_sprint_speed.filter(|speed| *speed > 0)?;
            let matching = owned
                .iter()
                .filter(|pal| normalize(&pal.species_id) == normalize(&record.species_id))
                .collect::<Vec<_>>();
            if matching.is_empty() {
                return None;
            }
            let best = matching
                .iter()
                .map(|pal| {
                    let (speed_bonus_percent, stamina_bonus_percent) = passive_mount_modifiers(pal);
                    (
                        apply_percent_bonus(base_sprint, speed_bonus_percent),
                        apply_percent_bonus(
                            record.stamina.unwrap_or_default(),
                            stamina_bonus_percent,
                        ),
                        speed_bonus_percent,
                        stamina_bonus_percent,
                    )
                })
                .max()
                .unwrap_or_default();
            Some(MountCandidate {
                catalog_index,
                record,
                owned_count: matching.len(),
                speed_bonus_percent: best.2,
                stamina_bonus_percent: best.3,
                effective_sprint: best.0,
                effective_stamina: best.1,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .effective_sprint
            .cmp(&left.effective_sprint)
            .then(right.effective_stamina.cmp(&left.effective_stamina))
            .then(left.record.species_id.cmp(&right.record.species_id))
    });
    if candidates.is_empty() {
        return format!(
            "현재 읽은 보유 팰 {}마리와 도감 근거만으로는 탑승 질주 수치가 확인된 날탈을 찾지 못했습니다. 도감 Build 일치가 검증되기 전에는 임의로 후보를 만들지 않습니다. [owned-pal:summary]",
            owned.len()
        );
    }

    let mut lines = vec![
        "보유 팰 중 탑승 수치가 확인된 날탈을 비교했습니다. 번들 데이터에서 효과가 확인된 이동·기력 패시브를 단순 합산한 추정치이며, 파트너 스킬·농축·게임 내부 상한은 아직 반영하지 않았습니다."
            .to_owned(),
    ];
    for (rank, candidate) in candidates.iter().take(5).enumerate() {
        lines.push(format!(
            "{}. {} — 기본 질주 {}, 이동 +{}%, 추정 질주 {}, 기본 기력 {}, 기력 {:+}%, 추정 기력 {}, 보유 {}마리 [catalog:{:02}] [owned-pal:summary]",
            rank + 1,
            candidate
                .record
                .name_ko
                .as_deref()
                .unwrap_or(&candidate.record.species_id),
            number_or_dash(candidate.record.ride_sprint_speed),
            candidate.speed_bonus_percent,
            candidate.effective_sprint,
            number_or_dash(candidate.record.stamina),
            candidate.stamina_bonus_percent,
            candidate.effective_stamina,
            candidate.owned_count,
            candidate.catalog_index
        ));
    }
    if let Some(fastest) = candidates.first() {
        lines.push(format!(
            "현재 확인 가능한 패시브까지 반영한 속도 우선 후보는 {}입니다. 추정 질주 {}로 비교 후보 중 가장 높습니다. [catalog:{:02}]",
            fastest
                .record
                .name_ko
                .as_deref()
                .unwrap_or(&fastest.record.species_id),
            fastest.effective_sprint,
            fastest.catalog_index
        ));
    }
    if let Some(endurance) = candidates
        .iter()
        .max_by_key(|candidate| candidate.effective_stamina)
    {
        lines.push(format!(
            "장거리 체공 우선 후보는 {}입니다. 추정 기력 {}으로 비교 후보 중 가장 높습니다. [catalog:{:02}]",
            endurance
                .record
                .name_ko
                .as_deref()
                .unwrap_or(&endurance.record.species_id),
            endurance.effective_stamina,
            endurance.catalog_index
        ));
    }
    lines.join("\n")
}

fn passive_mount_modifiers(pal: &OwnedPal) -> (i64, i64) {
    pal.passive_ids
        .iter()
        .fold((0, 0), |(speed, stamina), passive_id| {
            let speed_delta = match passive_id.as_str() {
                "MoveSpeed_up_1" => 10,
                "MoveSpeed_up_2" => 20,
                "MoveSpeed_up_3" => 30,
                "Legend" => 20,
                "WorldTree_MoveSpeed" => 50,
                _ => 0,
            };
            let stamina_delta = match passive_id.as_str() {
                "Stamina_Down_1" => -25,
                "Stamina_Up_1" => 50,
                "Stamina_Up_2" => 25,
                "Stamina_Up_3" => 75,
                _ => 0,
            };
            (speed + speed_delta, stamina + stamina_delta)
        })
}

fn apply_percent_bonus(base: i64, bonus_percent: i64) -> i64 {
    base.saturating_mul(100 + bonus_percent).div_euclid(100)
}

fn owned_answer(owned: &[&OwnedPal], owned_total: usize) -> String {
    if owned.is_empty() {
        return format!(
            "선택한 캐릭터에서 총 {owned_total}마리를 읽었지만 질문과 일치하는 팰이나 패시브를 찾지 못했습니다. 팰 이름 또는 패시브 이름을 더 구체적으로 입력하세요. [owned-pal:summary]"
        );
    }
    let mut lines = vec![format!(
        "선택한 캐릭터의 보유 팰 {owned_total}마리 중 질문과 일치하는 개체를 확인했습니다. [owned-pal:summary]"
    )];
    for (index, pal) in owned.iter().take(10).enumerate() {
        lines.push(format!(
            "- {} — 레벨 {}, IV {}/{}/{}, 패시브 {} [owned-pal:{index:02}]",
            owned_title(pal),
            pal.level,
            number_or_dash(pal.iv_hp),
            number_or_dash(pal.iv_attack),
            number_or_dash(pal.iv_defense),
            value_or_dash(if pal.passive_names_ko.is_empty() {
                None
            } else {
                Some(pal.passive_names_ko.join(", "))
            })
        ));
    }
    if owned.len() > 10 {
        lines.push(format!(
            "일치 항목이 더 있어 상위 10개만 표시했습니다. 전체 {}개는 내 데이터 화면에서 확인할 수 있습니다.",
            owned.len()
        ));
    }
    lines.join("\n")
}

fn catalog_answer(catalog: &[&CatalogRecord], owned: &[&OwnedPal]) -> String {
    let mut lines = vec!["질문과 관련도가 높은 도감 수치를 정리했습니다.".to_owned()];
    for (index, record) in catalog.iter().take(5).enumerate() {
        lines.push(format!(
            "- {}: {} [catalog:{index:02}]",
            catalog_title(record),
            catalog_detail(record)
        ));
    }
    if !owned.is_empty() {
        lines.push(format!(
            "선택한 캐릭터에서도 관련 개체 {}마리를 확인했습니다. [owned-pal:summary]",
            owned.len()
        ));
    }
    lines.join("\n")
}

fn is_mount_question(question: &str) -> bool {
    let normalized = question.to_lowercase();
    [
        "날탈",
        "비행 탈것",
        "비행팰",
        "비행 팰",
        "탈것",
        "이동용",
        "제일 빠",
        "mount",
        "flying",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn is_owned_question(question: &str) -> bool {
    let normalized = question.to_lowercase();
    [
        "내 팰",
        "보유 팰",
        "가지고 있는 팰",
        "가진 팰",
        "owned pal",
        "my pal",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn is_passive_question(question: &str) -> bool {
    let normalized = question.to_lowercase();
    [
        "패시브",
        "특성",
        "신속",
        "달리기왕",
        "전설",
        "장인",
        "passive",
        "swift",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn catalog_title(record: &CatalogRecord) -> String {
    let name = record.name_ko.as_deref().unwrap_or(&record.species_id);
    match record.paldex_number {
        Some(dex) => format!("#{dex} {name}"),
        None => name.to_owned(),
    }
}

fn catalog_detail(record: &CatalogRecord) -> String {
    let elements = record
        .elements
        .iter()
        .map(|value| value.name_ko.as_str())
        .collect::<Vec<_>>()
        .join("/");
    let works = record
        .work_suitability
        .iter()
        .map(|work| format!("{} Lv.{}", work.name_ko, work.level))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "속성 {}; HP {}; 공격 {}; 방어 {}; 탑승 질주 {}; 스태미나 {}; 작업 적성 {}",
        value_or_dash(if elements.is_empty() {
            None
        } else {
            Some(elements)
        }),
        number_or_dash(record.hp),
        number_or_dash(record.attack),
        number_or_dash(record.defense),
        number_or_dash(record.ride_sprint_speed),
        number_or_dash(record.stamina),
        value_or_dash(if works.is_empty() { None } else { Some(works) })
    )
}

fn owned_title(pal: &OwnedPal) -> String {
    let species = pal.species_name_ko.as_deref().unwrap_or(&pal.species_id);
    match pal
        .nickname
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(nickname) => format!("{nickname} ({species})"),
        None => species.to_owned(),
    }
}

fn owned_detail(pal: &OwnedPal) -> String {
    format!(
        "레벨 {}; IV HP/공격/방어 {}/{}/{}; 패시브 {}",
        pal.level,
        number_or_dash(pal.iv_hp),
        number_or_dash(pal.iv_attack),
        number_or_dash(pal.iv_defense),
        value_or_dash(if pal.passive_names_ko.is_empty() {
            None
        } else {
            Some(pal.passive_names_ko.join(", "))
        })
    )
}

fn query_tokens(question: &str) -> Vec<String> {
    question
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric() && character != '#')
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
        .filter(|token| {
            !matches!(
                *token,
                "알려줘" | "추천해줘" | "비교해줘" | "뭐가" | "어떤" | "가장" | "좋은"
            )
        })
        .map(str::to_owned)
        .collect()
}

fn owned_matches(pal: &OwnedPal, tokens: &[String]) -> bool {
    let mut text = format!(
        "{} {} {} {}",
        pal.species_id,
        pal.species_name_ko.as_deref().unwrap_or_default(),
        pal.nickname.as_deref().unwrap_or_default(),
        pal.passive_names_ko.join(" ")
    )
    .to_lowercase();
    text.retain(|character| !character.is_control());
    tokens.iter().any(|token| text.contains(token))
}

fn normalized_build_id(value: &str) -> String {
    let value = value.trim();
    if value.starts_with("steam:") {
        value.to_owned()
    } else if value.is_empty() {
        "steam:unknown".to_owned()
    } else {
        format!("steam:{value}")
    }
}

fn projection_digest(import_id: &str, pals: &[OwnedPal]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pal-companion-projection-v1\0");
    hasher.update(import_id.as_bytes());
    for pal in pals {
        hasher.update([0]);
        hasher.update(pal.instance_id.as_bytes());
        hasher.update([0]);
        hasher.update(pal.species_id.as_bytes());
        hasher.update(pal.level.to_le_bytes());
    }
    hex_digest(hasher.finalize())
}

fn digest_hex(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value);
    hex_digest(hasher.finalize())
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn number_or_dash(value: Option<i64>) -> String {
    value.map_or_else(|| "-".to_owned(), |number| number.to_string())
}

fn value_or_dash(value: Option<String>) -> String {
    value.unwrap_or_else(|| "-".to_owned())
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(question: &str) -> EvidenceRequest {
        EvidenceRequest {
            schema_version: 1,
            question: question.to_owned(),
            game_build_id: "24181527".to_owned(),
            catalog_source_id: "psp-static-catalog:pinned-private-alpha".to_owned(),
            import_id: "fixture-import".to_owned(),
            catalog_records: vec![CatalogRecord {
                species_id: "SkyDragon".to_owned(),
                name_ko: Some("페스키".to_owned()),
                paldex_number: Some(124),
                elements: vec![LocalizedValue {
                    name_ko: "용".to_owned(),
                }],
                hp: Some(105),
                attack: Some(100),
                defense: Some(100),
                ride_sprint_speed: Some(950),
                stamina: Some(220),
                work_suitability: vec![WorkSuitability {
                    name_ko: "채굴".to_owned(),
                    level: 4,
                }],
            }],
            owned_pals: vec![OwnedPal {
                instance_id: "instance-private-01".to_owned(),
                species_id: "SkyDragon".to_owned(),
                species_name_ko: Some("페스키".to_owned()),
                nickname: Some("이동용".to_owned()),
                level: 38,
                passive_ids: vec!["MoveSpeed_up_3".to_owned()],
                passive_names_ko: vec!["신속".to_owned()],
                iv_hp: Some(90),
                iv_attack: Some(80),
                iv_defense: Some(70),
            }],
        }
    }

    #[test]
    fn combines_catalog_and_owned_pal_without_model() {
        let response = build_response(request("내 페스키 수치를 알려줘")).unwrap();
        assert_eq!(response.status, "evidence_ready");
        assert!(!response.model_used);
        assert!(
            response
                .evidence
                .iter()
                .any(|row| row.evidence_id.starts_with("catalog:"))
        );
        assert!(
            response
                .evidence
                .iter()
                .any(|row| row.evidence_id.starts_with("owned-pal:"))
        );
        assert_eq!(response.prompt_digest_sha256.len(), 64);
    }

    #[test]
    fn compares_only_owned_mounts_and_keeps_calculation_limits_explicit() {
        let mut request = request("내 날탈 중 뭐가 제일 좋아?");
        request.catalog_records.push(CatalogRecord {
            species_id: "NotOwnedFastMount".to_owned(),
            name_ko: Some("미보유 초고속팰".to_owned()),
            paldex_number: Some(999),
            elements: Vec::new(),
            hp: Some(100),
            attack: Some(100),
            defense: Some(100),
            ride_sprint_speed: Some(9_999),
            stamina: Some(999),
            work_suitability: Vec::new(),
        });

        let response = build_response(request).unwrap();

        assert!(response.answer.contains("페스키"));
        assert!(response.answer.contains("기본 질주 950"));
        assert!(response.answer.contains("이동 +30%, 추정 질주 1235"));
        assert!(!response.answer.contains("미보유 초고속팰"));
        assert!(
            response
                .answer
                .contains("파트너 스킬·농축·게임 내부 상한은 아직 반영하지 않았습니다")
        );
        assert!(response.warnings.contains(&"DETERMINISTIC_LOCAL_COACH"));
    }

    #[test]
    fn known_mount_passive_ids_adjust_speed_and_stamina_conservatively() {
        let mut request = request("내 날탈 중 뭐가 제일 좋아?");
        request.owned_pals[0].passive_ids = vec![
            "MoveSpeed_up_2".to_owned(),
            "Legend".to_owned(),
            "Stamina_Up_3".to_owned(),
        ];

        let response = build_response(request).unwrap();

        assert!(response.answer.contains("이동 +40%, 추정 질주 1330"));
        assert!(response.answer.contains("기력 +75%, 추정 기력 385"));
    }

    #[test]
    fn passive_question_lists_matching_owned_instances_without_private_ids() {
        let response = build_response(request("내 팰 중 신속 특성 가진 팰 알려줘")).unwrap();

        assert!(response.answer.contains("신속"));
        assert!(response.answer.contains("이동용 (페스키)"));
        assert!(!response.answer.contains("instance-private-01"));
        assert!(response.answer.contains("[owned-pal:00]"));
    }

    #[test]
    fn private_instance_identifier_is_not_used_as_an_evidence_id() {
        let response = build_response(request("페스키 알려줘")).unwrap();
        let encoded = serde_json::to_string(&response).unwrap();
        assert!(!encoded.contains("instance-private-01"));
    }

    #[test]
    fn request_bounds_fail_closed() {
        assert!(read_request(&b""[..]).is_err());
        let mut request = request("페스키");
        request.catalog_records = vec![request.catalog_records[0].clone(); MAX_CATALOG_RECORDS + 1];
        let encoded = serde_json::to_vec(&request).unwrap();
        assert!(read_request(encoded.as_slice()).is_err());
    }
}
