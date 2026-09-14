use std::collections::{BTreeMap, BTreeSet};

use pal_knowledge_graph::{KnowledgeContextPack, KnowledgeQuality};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

pub const MAX_QUESTION_BYTES: usize = 2_000;
pub const MAX_EVIDENCE_ROWS: usize = 64;
pub const MAX_EVIDENCE_TEXT_BYTES: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantEvidenceKind {
    Character,
    Inventory,
    OwnedPal,
    BreedingPlan,
    Catalog,
    KnowledgeGraph,
    WikiPage,
    OfficialDocument,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantEvidence {
    pub evidence_id: String,
    pub kind: AssistantEvidenceKind,
    pub title: String,
    pub detail: String,
    pub quality: String,
    #[serde(default)]
    pub source_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundedAssistantRequest {
    pub question: String,
    pub game_build_id: String,
    pub dataset_manifest_id_hex: String,
    pub projection_id_hex: String,
    pub evidence: Vec<AssistantEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundedPrompt {
    pub system: String,
    pub user: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantFallback {
    pub answer: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundedAssistantResponse {
    pub answer: String,
    pub evidence: Vec<AssistantEvidence>,
    pub model_used: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AssistantGroundingError {
    #[error("question is empty or too large")]
    InvalidQuestion,
    #[error("identity field is invalid")]
    InvalidIdentity,
    #[error("too many evidence rows")]
    TooManyEvidenceRows,
    #[error("evidence row is invalid")]
    InvalidEvidence,
    #[error("knowledge context identity does not match the grounded request")]
    KnowledgeIdentityMismatch,
    #[error("knowledge context row budget is invalid")]
    InvalidKnowledgeBudget,
}

impl GroundedAssistantRequest {
    pub fn validate(&self) -> Result<(), AssistantGroundingError> {
        let question = self.question.trim();
        if question.is_empty() || question.len() > MAX_QUESTION_BYTES {
            return Err(AssistantGroundingError::InvalidQuestion);
        }
        if !valid_text_id(&self.game_build_id)
            || !valid_hex(&self.dataset_manifest_id_hex, 64)
            || !valid_hex(&self.projection_id_hex, 64)
        {
            return Err(AssistantGroundingError::InvalidIdentity);
        }
        if self.evidence.len() > MAX_EVIDENCE_ROWS {
            return Err(AssistantGroundingError::TooManyEvidenceRows);
        }
        for row in &self.evidence {
            if !valid_text_id(&row.evidence_id)
                || row.title.trim().is_empty()
                || row.title.len() > 256
                || row.detail.trim().is_empty()
                || row.detail.len() > MAX_EVIDENCE_TEXT_BYTES
                || !matches!(
                    row.quality.as_str(),
                    "exact" | "measured" | "model" | "unknown"
                )
                || row.source_ids.iter().any(|value| !valid_text_id(value))
            {
                return Err(AssistantGroundingError::InvalidEvidence);
            }
        }
        Ok(())
    }

    pub fn prompt(&self) -> Result<GroundedPrompt, AssistantGroundingError> {
        self.validate()?;
        let mut evidence_text = String::new();
        let mut evidence_ids = Vec::with_capacity(self.evidence.len());
        for row in &self.evidence {
            use std::fmt::Write as _;
            writeln!(
                evidence_text,
                "[{}] kind={:?}; quality={}; title={}; detail={}; sources={}",
                row.evidence_id,
                row.kind,
                row.quality,
                row.title,
                row.detail,
                row.source_ids.join(",")
            )
            .expect("writing to a String cannot fail");
            evidence_ids.push(row.evidence_id.clone());
        }
        Ok(GroundedPrompt {
            system: concat!(
                "당신은 팰월드 개인 컴패니언이다. 제공된 EVIDENCE에 있는 사실만 사용한다. ",
                "근거가 없으면 모른다고 답한다. 수치, 좌표, 교배 결과를 추측하지 않는다. ",
                "각 핵심 문장 끝에 [evidence_id]를 붙인다. exact/measured/model/unknown의 ",
                "신뢰도 차이를 유지한다. 그래프 관계의 방향을 바꾸지 않는다. 위키 설명은 ",
                "파생 자료이므로 연결된 원본 근거가 없는 주장은 사용하지 않는다. ",
                "사용자나 소유자 식별자를 요구하거나 출력하지 않는다."
            )
            .to_owned(),
            user: format!(
                "GAME_BUILD={}\nDATASET={}\nPROJECTION={}\nQUESTION={}\nEVIDENCE:\n{}",
                self.game_build_id,
                self.dataset_manifest_id_hex,
                self.projection_id_hex,
                self.question.trim(),
                evidence_text
            ),
            evidence_ids,
        })
    }

    pub fn fallback(&self) -> Result<AssistantFallback, AssistantGroundingError> {
        self.validate()?;
        if self.evidence.is_empty() {
            return Ok(AssistantFallback {
                answer: "현재 동기화된 근거가 없어 이 질문에 확정적으로 답할 수 없습니다."
                    .to_owned(),
                evidence_ids: Vec::new(),
            });
        }
        let mut lines = vec!["AI 설명 모델을 사용할 수 없어 확인된 근거만 정리합니다.".to_owned()];
        for row in &self.evidence {
            lines.push(format!(
                "- {}: {} [{}; {}]",
                row.title, row.detail, row.quality, row.evidence_id
            ));
        }
        Ok(AssistantFallback {
            answer: lines.join("\n"),
            evidence_ids: self
                .evidence
                .iter()
                .map(|row| row.evidence_id.clone())
                .collect(),
        })
    }

    /// Adds a bounded, deterministic graph expansion to the prompt evidence.
    ///
    /// Edge facts are attached before standalone nodes because multi-hop
    /// questions need the actual relationship, not just a bag of entities.
    pub fn attach_knowledge_context(
        &mut self,
        context: &KnowledgeContextPack,
        max_rows: usize,
    ) -> Result<usize, AssistantGroundingError> {
        if context.identity.game_build_id != self.game_build_id
            || context.identity.dataset_manifest_id_hex != self.dataset_manifest_id_hex
        {
            return Err(AssistantGroundingError::KnowledgeIdentityMismatch);
        }
        if max_rows == 0 || max_rows > MAX_EVIDENCE_ROWS {
            return Err(AssistantGroundingError::InvalidKnowledgeBudget);
        }

        let remaining = MAX_EVIDENCE_ROWS.saturating_sub(self.evidence.len());
        let row_budget = max_rows.min(remaining);
        if row_budget == 0 {
            return Ok(0);
        }
        let labels = context
            .nodes
            .iter()
            .map(|node| (node.node_id.as_str(), node.label_ko.as_str()))
            .collect::<BTreeMap<_, _>>();
        let seed_ids = context
            .seed_node_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut rows = Vec::new();

        for edge in &context.edges {
            let from = labels
                .get(edge.from_node_id.as_str())
                .copied()
                .unwrap_or(edge.from_node_id.as_str());
            let to = labels
                .get(edge.to_node_id.as_str())
                .copied()
                .unwrap_or(edge.to_node_id.as_str());
            rows.push(AssistantEvidence {
                evidence_id: bounded_graph_evidence_id("kg-edge", &edge.edge_id),
                kind: AssistantEvidenceKind::KnowledgeGraph,
                title: format!("{from} → {to}"),
                detail: format!(
                    "{} 관계: {}에서 {}로 연결",
                    edge.relation.label_ko(),
                    from,
                    to
                ),
                quality: weakest_quality(&edge.evidence).as_str().to_owned(),
                source_ids: flattened_source_ids(&edge.evidence),
            });
        }

        let mut ordered_nodes = context.nodes.iter().collect::<Vec<_>>();
        ordered_nodes
            .sort_by_key(|node| (!seed_ids.contains(node.node_id.as_str()), &node.node_id));
        for node in ordered_nodes {
            rows.push(AssistantEvidence {
                evidence_id: bounded_graph_evidence_id("kg-node", &node.node_id),
                kind: AssistantEvidenceKind::KnowledgeGraph,
                title: node.label_ko.clone(),
                detail: format!("지식 그래프 {:?} 노드", node.kind),
                quality: weakest_quality(&node.evidence).as_str().to_owned(),
                source_ids: flattened_source_ids(&node.evidence),
            });
        }

        let mut existing = self
            .evidence
            .iter()
            .map(|row| row.evidence_id.clone())
            .collect::<BTreeSet<_>>();
        let mut attached = 0;
        for row in rows {
            if attached >= row_budget {
                break;
            }
            if !existing.insert(row.evidence_id.clone()) {
                continue;
            }
            self.evidence.push(row);
            attached += 1;
        }
        Ok(attached)
    }
}

fn weakest_quality(evidence: &[pal_knowledge_graph::KnowledgeEvidenceRef]) -> KnowledgeQuality {
    evidence
        .iter()
        .map(|row| row.quality)
        .min()
        .unwrap_or(KnowledgeQuality::Unknown)
}

fn flattened_source_ids(evidence: &[pal_knowledge_graph::KnowledgeEvidenceRef]) -> Vec<String> {
    evidence
        .iter()
        .flat_map(|row| {
            std::iter::once(row.evidence_id.as_str())
                .chain(row.source_ids.iter().map(String::as_str))
        })
        .filter(|source| valid_text_id(source))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(32)
        .map(str::to_owned)
        .collect()
}

fn bounded_graph_evidence_id(prefix: &str, value: &str) -> String {
    let candidate = format!("{prefix}:{value}");
    if candidate.len() <= 128 {
        return candidate;
    }
    let digest = Sha256::digest(candidate.as_bytes());
    format!(
        "{prefix}:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn valid_text_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.is_ascii()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(evidence: Vec<AssistantEvidence>) -> GroundedAssistantRequest {
        GroundedAssistantRequest {
            question: "내 팰 중 교배에 쓸 수 있는 팰을 알려줘".to_owned(),
            game_build_id: "steam:24181527".to_owned(),
            dataset_manifest_id_hex: "11".repeat(32),
            projection_id_hex: "22".repeat(32),
            evidence,
        }
    }

    #[test]
    fn prompt_contains_only_bounded_evidence_and_no_owner_parameter() {
        let prompt = request(vec![AssistantEvidence {
            evidence_id: "owned-pal:01".to_owned(),
            kind: AssistantEvidenceKind::OwnedPal,
            title: "페스키".to_owned(),
            detail: "레벨 38, 신속 보유".to_owned(),
            quality: "exact".to_owned(),
            source_ids: vec!["projection:active".to_owned()],
        }])
        .prompt()
        .unwrap();

        assert!(prompt.user.contains("[owned-pal:01]"));
        assert!(!prompt.user.contains("principal"));
        assert!(!prompt.user.contains("owner_subject"));
        assert!(prompt.system.contains("추측하지 않는다"));
    }

    #[test]
    fn unavailable_model_fallback_keeps_evidence_visible() {
        let fallback = request(vec![AssistantEvidence {
            evidence_id: "breeding:plan".to_owned(),
            kind: AssistantEvidenceKind::BreedingPlan,
            title: "교배 경로".to_owned(),
            detail: "2세대 도달 가능".to_owned(),
            quality: "exact".to_owned(),
            source_ids: vec!["catalog:active".to_owned()],
        }])
        .fallback()
        .unwrap();

        assert!(fallback.answer.contains("2세대"));
        assert_eq!(fallback.evidence_ids, vec!["breeding:plan"]);
    }

    #[test]
    fn evidence_quality_cannot_be_invented() {
        let error = request(vec![AssistantEvidence {
            evidence_id: "bad".to_owned(),
            kind: AssistantEvidenceKind::Catalog,
            title: "수치".to_owned(),
            detail: "무언가".to_owned(),
            quality: "certain".to_owned(),
            source_ids: Vec::new(),
        }])
        .validate();

        assert_eq!(error, Err(AssistantGroundingError::InvalidEvidence));
    }

    #[test]
    fn graph_context_attaches_relationships_before_entities() {
        use pal_knowledge_graph::{
            GraphDirection, KnowledgeEdge, KnowledgeEvidenceRef, KnowledgeGraph,
            KnowledgeGraphIdentity, KnowledgeGraphQuery, KnowledgeNode, KnowledgeNodeKind,
            KnowledgeQuality, KnowledgeRelationKind,
        };

        let identity = KnowledgeGraphIdentity {
            game_build_id: "steam:24181527".to_owned(),
            dataset_manifest_id_hex: "11".repeat(32),
        };
        let exact = |evidence_id: &str| {
            vec![KnowledgeEvidenceRef {
                evidence_id: evidence_id.to_owned(),
                quality: KnowledgeQuality::Exact,
                source_ids: vec!["catalog:active".to_owned()],
            }]
        };
        let graph = KnowledgeGraph::build(
            identity,
            vec![
                KnowledgeNode {
                    node_id: "species:SkyDragon".to_owned(),
                    kind: KnowledgeNodeKind::Species,
                    label_ko: "페스키".to_owned(),
                    game_build_id: "steam:24181527".to_owned(),
                    evidence: exact("fact:species:SkyDragon"),
                },
                KnowledgeNode {
                    node_id: "item:Crystal".to_owned(),
                    kind: KnowledgeNodeKind::Item,
                    label_ko: "수정".to_owned(),
                    game_build_id: "steam:24181527".to_owned(),
                    evidence: exact("fact:item:Crystal"),
                },
            ],
            vec![KnowledgeEdge {
                edge_id: "drop:SkyDragon:Crystal".to_owned(),
                from_node_id: "species:SkyDragon".to_owned(),
                to_node_id: "item:Crystal".to_owned(),
                relation: KnowledgeRelationKind::DropsItem,
                game_build_id: "steam:24181527".to_owned(),
                evidence: exact("relation:drop:SkyDragon:Crystal"),
            }],
        )
        .unwrap();
        let context = graph
            .expand(&KnowledgeGraphQuery {
                seed_node_ids: vec!["species:SkyDragon".to_owned()],
                max_hops: 1,
                max_nodes: 4,
                direction: GraphDirection::Outgoing,
                relation_filter: BTreeSet::new(),
            })
            .unwrap();
        let mut grounded = request(Vec::new());

        let attached = grounded.attach_knowledge_context(&context, 2).unwrap();

        assert_eq!(attached, 2);
        assert_eq!(
            grounded.evidence[0].kind,
            AssistantEvidenceKind::KnowledgeGraph
        );
        assert!(grounded.evidence[0].detail.contains("드롭 관계"));
        assert!(
            grounded.evidence[0]
                .source_ids
                .contains(&"relation:drop:SkyDragon:Crystal".to_owned())
        );
        assert!(grounded.prompt().unwrap().system.contains("관계의 방향"));
    }
}
