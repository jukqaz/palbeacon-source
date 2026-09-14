//! Deterministic, evidence-backed knowledge graph and compiled wiki contracts.
//!
//! The graph is a derived projection of exact-build catalog data. LLM output
//! may propose or phrase wiki claims, but it never creates canonical nodes,
//! edges, evidence quality, or game mechanics.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use petgraph::{
    Direction as PetDirection,
    stable_graph::{NodeIndex, StableDiGraph},
    visit::EdgeRef as _,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_GRAPH_NODES: usize = 100_000;
const MAX_GRAPH_EDGES: usize = 1_000_000;
const MAX_QUERY_SEEDS: usize = 16;
const MAX_QUERY_HOPS: u8 = 4;
const MAX_QUERY_NODES: usize = 128;
const MAX_WIKI_CLAIMS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeGraphIdentity {
    pub game_build_id: String,
    pub dataset_manifest_id_hex: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeNodeKind {
    Species,
    Item,
    ActiveSkill,
    PassiveSkill,
    Building,
    Technology,
    Shop,
    PointOfInterest,
    Recipe,
    BreedingRule,
    OfficialDocument,
    WikiPage,
    Concept,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeRelationKind {
    LearnsSkill,
    HasGuaranteedPassive,
    DropsItem,
    IngredientFor,
    ProducesItem,
    SoldBy,
    Unlocks,
    LocatedAt,
    Requires,
    BreedingParent,
    BreedsInto,
    References,
    RelatedTo,
}

impl KnowledgeRelationKind {
    pub fn label_ko(self) -> &'static str {
        match self {
            Self::LearnsSkill => "습득 스킬",
            Self::HasGuaranteedPassive => "확정 패시브",
            Self::DropsItem => "드롭",
            Self::IngredientFor => "제작 재료",
            Self::ProducesItem => "제작 결과",
            Self::SoldBy => "판매",
            Self::Unlocks => "해금",
            Self::LocatedAt => "위치",
            Self::Requires => "필요 조건",
            Self::BreedingParent => "교배 부모",
            Self::BreedsInto => "교배 결과",
            Self::References => "근거 문서",
            Self::RelatedTo => "관련 항목",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeQueryIntent {
    EntityLookup,
    Relationship,
    Comparison,
    Overview,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRetrievalPlan {
    pub intent: KnowledgeQueryIntent,
    pub search_first: bool,
    pub maximum_seed_nodes: usize,
    pub maximum_graph_hops: u8,
    pub maximum_graph_nodes: usize,
    pub maximum_wiki_pages: usize,
    pub maximum_source_documents: usize,
}

impl KnowledgeRetrievalPlan {
    /// Selects a bounded retrieval route before any model call.
    ///
    /// This is deliberately deterministic. The model receives the retrieved
    /// evidence, but it cannot silently increase graph depth or context size.
    pub fn for_korean_question(question: &str) -> Self {
        let normalized = question
            .chars()
            .flat_map(char::to_lowercase)
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        let contains_any =
            |needles: &[&str]| needles.iter().any(|needle| normalized.contains(needle));

        if contains_any(&["비교", "차이", "대비", "vs", "어느쪽", "뭐가더"]) {
            return Self {
                intent: KnowledgeQueryIntent::Comparison,
                search_first: true,
                maximum_seed_nodes: 4,
                maximum_graph_hops: 2,
                maximum_graph_nodes: 40,
                maximum_wiki_pages: 4,
                maximum_source_documents: 3,
            };
        }
        if contains_any(&[
            "전체",
            "종류",
            "목록",
            "한눈에",
            "개요",
            "무엇이있",
            "어떤것",
        ]) {
            return Self {
                intent: KnowledgeQueryIntent::Overview,
                search_first: false,
                maximum_seed_nodes: 6,
                maximum_graph_hops: 1,
                maximum_graph_nodes: 48,
                maximum_wiki_pages: 6,
                maximum_source_documents: 4,
            };
        }
        if contains_any(&[
            "필요", "재료", "만들", "제작", "드롭", "획득", "어디", "교배", "배우", "연결", "관계",
            "경로",
        ]) {
            return Self {
                intent: KnowledgeQueryIntent::Relationship,
                search_first: true,
                maximum_seed_nodes: 4,
                maximum_graph_hops: 2,
                maximum_graph_nodes: 48,
                maximum_wiki_pages: 4,
                maximum_source_documents: 4,
            };
        }
        Self {
            intent: KnowledgeQueryIntent::EntityLookup,
            search_first: true,
            maximum_seed_nodes: 3,
            maximum_graph_hops: 1,
            maximum_graph_nodes: 24,
            maximum_wiki_pages: 3,
            maximum_source_documents: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeQuality {
    Unknown,
    Model,
    Measured,
    Exact,
}

impl KnowledgeQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Model => "model",
            Self::Measured => "measured",
            Self::Exact => "exact",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeEvidenceRef {
    pub evidence_id: String,
    pub quality: KnowledgeQuality,
    #[serde(default)]
    pub source_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeNode {
    pub node_id: String,
    pub kind: KnowledgeNodeKind,
    pub label_ko: String,
    pub game_build_id: String,
    pub evidence: Vec<KnowledgeEvidenceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeEdge {
    pub edge_id: String,
    pub from_node_id: String,
    pub to_node_id: String,
    pub relation: KnowledgeRelationKind,
    pub game_build_id: String,
    pub evidence: Vec<KnowledgeEvidenceRef>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphDirection {
    Incoming,
    Outgoing,
    #[default]
    Both,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeGraphQuery {
    pub seed_node_ids: Vec<String>,
    pub max_hops: u8,
    pub max_nodes: usize,
    #[serde(default)]
    pub direction: GraphDirection,
    #[serde(default)]
    pub relation_filter: BTreeSet<KnowledgeRelationKind>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeContextPack {
    pub identity: KnowledgeGraphIdentity,
    pub seed_node_ids: Vec<String>,
    pub max_hops: u8,
    pub nodes: Vec<KnowledgeNode>,
    pub edges: Vec<KnowledgeEdge>,
    pub complete_within_budget: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct KnowledgeGraph {
    identity: KnowledgeGraphIdentity,
    graph: StableDiGraph<KnowledgeNode, KnowledgeEdge>,
    node_indices: BTreeMap<String, NodeIndex>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WikiPageStatus {
    Draft,
    Reviewed,
    Superseded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiClaim {
    pub claim_id: String,
    pub text_ko: String,
    pub quality: KnowledgeQuality,
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub related_node_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeWikiPage {
    pub page_id: String,
    pub title_ko: String,
    pub summary_ko: String,
    pub game_build_id: String,
    pub dataset_manifest_id_hex: String,
    pub status: WikiPageStatus,
    pub claims: Vec<WikiClaim>,
    #[serde(default)]
    pub related_node_ids: Vec<String>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum KnowledgeGraphError {
    #[error("graph identity is invalid")]
    InvalidIdentity,
    #[error("graph is too large")]
    GraphTooLarge,
    #[error("node is invalid: {0}")]
    InvalidNode(String),
    #[error("edge is invalid: {0}")]
    InvalidEdge(String),
    #[error("duplicate node id: {0}")]
    DuplicateNode(String),
    #[error("duplicate edge id: {0}")]
    DuplicateEdge(String),
    #[error("edge endpoint is missing: {0}")]
    MissingEndpoint(String),
    #[error("entity build does not match graph build: {0}")]
    BuildMismatch(String),
    #[error("graph query is invalid")]
    InvalidQuery,
    #[error("seed node is missing: {0}")]
    MissingSeed(String),
    #[error("wiki page is invalid: {0}")]
    InvalidWikiPage(String),
    #[error("wiki claim references unknown evidence: {0}")]
    UnknownEvidence(String),
    #[error("wiki claim references unknown node: {0}")]
    UnknownNode(String),
    #[error("wiki claim quality exceeds its evidence: {0}")]
    UnsupportedClaimQuality(String),
}

impl KnowledgeGraph {
    pub fn build(
        identity: KnowledgeGraphIdentity,
        nodes: Vec<KnowledgeNode>,
        edges: Vec<KnowledgeEdge>,
    ) -> Result<Self, KnowledgeGraphError> {
        validate_identity(&identity)?;
        if nodes.len() > MAX_GRAPH_NODES || edges.len() > MAX_GRAPH_EDGES {
            return Err(KnowledgeGraphError::GraphTooLarge);
        }

        let mut ordered_nodes = BTreeMap::new();
        for node in nodes {
            validate_node(&node)?;
            if node.game_build_id != identity.game_build_id {
                return Err(KnowledgeGraphError::BuildMismatch(node.node_id));
            }
            let node_id = node.node_id.clone();
            if ordered_nodes.insert(node_id.clone(), node).is_some() {
                return Err(KnowledgeGraphError::DuplicateNode(node_id));
            }
        }

        let mut ordered_edges = BTreeMap::new();
        for edge in edges {
            validate_edge(&edge)?;
            if edge.game_build_id != identity.game_build_id {
                return Err(KnowledgeGraphError::BuildMismatch(edge.edge_id));
            }
            if !ordered_nodes.contains_key(&edge.from_node_id)
                || !ordered_nodes.contains_key(&edge.to_node_id)
            {
                return Err(KnowledgeGraphError::MissingEndpoint(edge.edge_id));
            }
            let edge_id = edge.edge_id.clone();
            if ordered_edges.insert(edge_id.clone(), edge).is_some() {
                return Err(KnowledgeGraphError::DuplicateEdge(edge_id));
            }
        }

        let mut graph = StableDiGraph::new();
        let mut node_indices = BTreeMap::new();
        for (node_id, node) in ordered_nodes {
            let index = graph.add_node(node);
            node_indices.insert(node_id, index);
        }
        for edge in ordered_edges.into_values() {
            let from = node_indices[&edge.from_node_id];
            let to = node_indices[&edge.to_node_id];
            graph.add_edge(from, to, edge);
        }

        Ok(Self {
            identity,
            graph,
            node_indices,
        })
    }

    pub fn identity(&self) -> &KnowledgeGraphIdentity {
        &self.identity
    }

    pub fn node(&self, node_id: &str) -> Option<&KnowledgeNode> {
        self.node_indices
            .get(node_id)
            .and_then(|index| self.graph.node_weight(*index))
    }

    pub fn expand(
        &self,
        query: &KnowledgeGraphQuery,
    ) -> Result<KnowledgeContextPack, KnowledgeGraphError> {
        validate_query(query)?;
        let mut visited = BTreeSet::new();
        for seed in &query.seed_node_ids {
            if !self.node_indices.contains_key(seed) {
                return Err(KnowledgeGraphError::MissingSeed(seed.clone()));
            }
            visited.insert(seed.clone());
        }
        let mut frontier = visited.clone();
        let mut complete_within_budget = true;
        let mut warnings = Vec::new();

        'walk: for _ in 0..query.max_hops {
            let mut next = BTreeSet::new();
            for node_id in &frontier {
                for (_, neighbor_id) in self.neighbor_candidates(node_id, query) {
                    if visited.contains(&neighbor_id) {
                        continue;
                    }
                    if visited.len() >= query.max_nodes {
                        complete_within_budget = false;
                        warnings.push("GRAPH_NODE_BUDGET_EXHAUSTED".to_owned());
                        break 'walk;
                    }
                    visited.insert(neighbor_id.clone());
                    next.insert(neighbor_id);
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }

        let nodes = visited
            .iter()
            .filter_map(|node_id| self.node(node_id).cloned())
            .collect::<Vec<_>>();
        let mut edges = self
            .graph
            .edge_indices()
            .filter_map(|edge_index| {
                let (from, to) = self.graph.edge_endpoints(edge_index)?;
                let edge = self.graph.edge_weight(edge_index)?;
                let from_id = &self.graph.node_weight(from)?.node_id;
                let to_id = &self.graph.node_weight(to)?.node_id;
                (visited.contains(from_id)
                    && visited.contains(to_id)
                    && relation_allowed(edge.relation, &query.relation_filter))
                .then(|| edge.clone())
            })
            .collect::<Vec<_>>();
        edges.sort_by(|left, right| left.edge_id.cmp(&right.edge_id));

        Ok(KnowledgeContextPack {
            identity: self.identity.clone(),
            seed_node_ids: query.seed_node_ids.clone(),
            max_hops: query.max_hops,
            nodes,
            edges,
            complete_within_budget,
            warnings,
        })
    }

    pub fn validate_wiki_page(&self, page: &KnowledgeWikiPage) -> Result<(), KnowledgeGraphError> {
        if page.game_build_id != self.identity.game_build_id
            || page.dataset_manifest_id_hex != self.identity.dataset_manifest_id_hex
        {
            return Err(KnowledgeGraphError::BuildMismatch(page.page_id.clone()));
        }
        if !valid_id(&page.page_id)
            || !valid_human_text(&page.title_ko, 256)
            || !valid_human_text(&page.summary_ko, 2_000)
            || page.claims.len() > MAX_WIKI_CLAIMS
        {
            return Err(KnowledgeGraphError::InvalidWikiPage(page.page_id.clone()));
        }

        let mut evidence_quality = BTreeMap::new();
        for evidence in self
            .graph
            .node_weights()
            .flat_map(|node| &node.evidence)
            .chain(self.graph.edge_weights().flat_map(|edge| &edge.evidence))
        {
            evidence_quality
                .entry(evidence.evidence_id.as_str())
                .and_modify(|quality: &mut KnowledgeQuality| {
                    if evidence.quality < *quality {
                        *quality = evidence.quality;
                    }
                })
                .or_insert(evidence.quality);
        }

        for node_id in page
            .related_node_ids
            .iter()
            .chain(page.claims.iter().flat_map(|claim| &claim.related_node_ids))
        {
            if !self.node_indices.contains_key(node_id) {
                return Err(KnowledgeGraphError::UnknownNode(node_id.clone()));
            }
        }

        let mut claim_ids = BTreeSet::new();
        for claim in &page.claims {
            if !valid_id(&claim.claim_id)
                || !valid_human_text(&claim.text_ko, 1_024)
                || claim.evidence_refs.is_empty()
                || !claim_ids.insert(claim.claim_id.as_str())
            {
                return Err(KnowledgeGraphError::InvalidWikiPage(page.page_id.clone()));
            }
            let mut weakest = KnowledgeQuality::Exact;
            for evidence_id in &claim.evidence_refs {
                let Some(quality) = evidence_quality.get(evidence_id.as_str()) else {
                    return Err(KnowledgeGraphError::UnknownEvidence(evidence_id.clone()));
                };
                weakest = weakest.min(*quality);
            }
            if claim.quality > weakest {
                return Err(KnowledgeGraphError::UnsupportedClaimQuality(
                    claim.claim_id.clone(),
                ));
            }
        }
        Ok(())
    }

    fn neighbor_candidates(
        &self,
        node_id: &str,
        query: &KnowledgeGraphQuery,
    ) -> Vec<(String, String)> {
        let index = self.node_indices[node_id];
        let mut candidates = BTreeSet::new();
        if matches!(
            query.direction,
            GraphDirection::Outgoing | GraphDirection::Both
        ) {
            for edge in self.graph.edges_directed(index, PetDirection::Outgoing) {
                if relation_allowed(edge.weight().relation, &query.relation_filter) {
                    candidates.insert((
                        edge.weight().edge_id.clone(),
                        self.graph[edge.target()].node_id.clone(),
                    ));
                }
            }
        }
        if matches!(
            query.direction,
            GraphDirection::Incoming | GraphDirection::Both
        ) {
            for edge in self.graph.edges_directed(index, PetDirection::Incoming) {
                if relation_allowed(edge.weight().relation, &query.relation_filter) {
                    candidates.insert((
                        edge.weight().edge_id.clone(),
                        self.graph[edge.source()].node_id.clone(),
                    ));
                }
            }
        }
        candidates.into_iter().collect()
    }
}

impl KnowledgeWikiPage {
    pub fn render_markdown(&self) -> String {
        let mut output = format!(
            "---\npage_id: {}\ngame_build_id: {}\ndataset_manifest_id: {}\nstatus: {:?}\n---\n\n# {}\n\n{}\n",
            self.page_id,
            self.game_build_id,
            self.dataset_manifest_id_hex,
            self.status,
            self.title_ko,
            self.summary_ko
        );
        if !self.claims.is_empty() {
            output.push_str("\n## 확인된 내용\n\n");
            for claim in &self.claims {
                output.push_str(&format!(
                    "- {} [{}; {}]\n",
                    claim.text_ko,
                    claim.quality.as_str(),
                    claim.evidence_refs.join(", ")
                ));
            }
        }
        if !self.related_node_ids.is_empty() {
            output.push_str("\n## 관련 항목\n\n");
            for node_id in &self.related_node_ids {
                output.push_str(&format!("- [[{node_id}]]\n"));
            }
        }
        output
    }
}

fn validate_identity(identity: &KnowledgeGraphIdentity) -> Result<(), KnowledgeGraphError> {
    if !valid_id(&identity.game_build_id) || !valid_hex(&identity.dataset_manifest_id_hex, 64) {
        return Err(KnowledgeGraphError::InvalidIdentity);
    }
    Ok(())
}

fn validate_node(node: &KnowledgeNode) -> Result<(), KnowledgeGraphError> {
    if !valid_id(&node.node_id)
        || !valid_human_text(&node.label_ko, 256)
        || !valid_id(&node.game_build_id)
        || node.evidence.is_empty()
        || node
            .evidence
            .iter()
            .any(|evidence| !valid_evidence(evidence))
    {
        return Err(KnowledgeGraphError::InvalidNode(node.node_id.clone()));
    }
    Ok(())
}

fn validate_edge(edge: &KnowledgeEdge) -> Result<(), KnowledgeGraphError> {
    if !valid_id(&edge.edge_id)
        || !valid_id(&edge.from_node_id)
        || !valid_id(&edge.to_node_id)
        || !valid_id(&edge.game_build_id)
        || edge.evidence.is_empty()
        || edge
            .evidence
            .iter()
            .any(|evidence| !valid_evidence(evidence))
    {
        return Err(KnowledgeGraphError::InvalidEdge(edge.edge_id.clone()));
    }
    Ok(())
}

fn valid_evidence(evidence: &KnowledgeEvidenceRef) -> bool {
    valid_id(&evidence.evidence_id)
        && evidence.source_ids.len() <= 32
        && evidence.source_ids.iter().all(|source| valid_id(source))
}

fn validate_query(query: &KnowledgeGraphQuery) -> Result<(), KnowledgeGraphError> {
    let unique_seeds = query.seed_node_ids.iter().collect::<BTreeSet<_>>();
    if query.seed_node_ids.is_empty()
        || query.seed_node_ids.len() > MAX_QUERY_SEEDS
        || unique_seeds.len() != query.seed_node_ids.len()
        || query.seed_node_ids.iter().any(|seed| !valid_id(seed))
        || query.max_hops > MAX_QUERY_HOPS
        || query.max_nodes < query.seed_node_ids.len()
        || query.max_nodes > MAX_QUERY_NODES
    {
        return Err(KnowledgeGraphError::InvalidQuery);
    }
    Ok(())
}

fn relation_allowed(
    relation: KnowledgeRelationKind,
    filter: &BTreeSet<KnowledgeRelationKind>,
) -> bool {
    filter.is_empty() || filter.contains(&relation)
}

fn valid_id(value: &str) -> bool {
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

fn valid_human_text(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_bytes && !value.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> KnowledgeGraphIdentity {
        KnowledgeGraphIdentity {
            game_build_id: "steam:24425675".to_owned(),
            dataset_manifest_id_hex: "11".repeat(32),
        }
    }

    fn evidence(id: &str, quality: KnowledgeQuality) -> Vec<KnowledgeEvidenceRef> {
        vec![KnowledgeEvidenceRef {
            evidence_id: id.to_owned(),
            quality,
            source_ids: vec!["catalog:fixture".to_owned()],
        }]
    }

    fn node(id: &str, label: &str) -> KnowledgeNode {
        KnowledgeNode {
            node_id: id.to_owned(),
            kind: if id.starts_with("species:") {
                KnowledgeNodeKind::Species
            } else {
                KnowledgeNodeKind::Item
            },
            label_ko: label.to_owned(),
            game_build_id: identity().game_build_id,
            evidence: evidence(&format!("fact:{id}"), KnowledgeQuality::Exact),
        }
    }

    fn edge(id: &str, from: &str, to: &str) -> KnowledgeEdge {
        KnowledgeEdge {
            edge_id: id.to_owned(),
            from_node_id: from.to_owned(),
            to_node_id: to.to_owned(),
            relation: KnowledgeRelationKind::DropsItem,
            game_build_id: identity().game_build_id,
            evidence: evidence(&format!("relation:{id}"), KnowledgeQuality::Exact),
        }
    }

    fn fixture_graph() -> KnowledgeGraph {
        KnowledgeGraph::build(
            identity(),
            vec![
                node("species:a", "팰 A"),
                node("item:b", "재료 B"),
                node("item:c", "재료 C"),
                node("item:d", "재료 D"),
            ],
            vec![
                edge("edge:ab", "species:a", "item:b"),
                edge("edge:bc", "item:b", "item:c"),
                edge("edge:cd", "item:c", "item:d"),
            ],
        )
        .unwrap()
    }

    #[test]
    fn graph_rejects_missing_endpoint_and_build_mismatch() {
        let missing = KnowledgeGraph::build(
            identity(),
            vec![node("species:a", "팰 A")],
            vec![edge("edge:missing", "species:a", "item:missing")],
        );
        assert_eq!(
            missing.unwrap_err(),
            KnowledgeGraphError::MissingEndpoint("edge:missing".to_owned())
        );

        let mut wrong_build = node("species:a", "팰 A");
        wrong_build.game_build_id = "steam:old".to_owned();
        let mismatch = KnowledgeGraph::build(identity(), vec![wrong_build], vec![]);
        assert_eq!(
            mismatch.unwrap_err(),
            KnowledgeGraphError::BuildMismatch("species:a".to_owned())
        );
    }

    #[test]
    fn expansion_is_stable_bounded_and_directional() {
        let graph = fixture_graph();
        let pack = graph
            .expand(&KnowledgeGraphQuery {
                seed_node_ids: vec!["species:a".to_owned()],
                max_hops: 3,
                max_nodes: 3,
                direction: GraphDirection::Outgoing,
                relation_filter: BTreeSet::new(),
            })
            .unwrap();

        assert_eq!(
            pack.nodes
                .iter()
                .map(|node| node.node_id.as_str())
                .collect::<Vec<_>>(),
            vec!["item:b", "item:c", "species:a"]
        );
        assert_eq!(
            pack.edges
                .iter()
                .map(|edge| edge.edge_id.as_str())
                .collect::<Vec<_>>(),
            vec!["edge:ab", "edge:bc"]
        );
        assert!(!pack.complete_within_budget);
        assert_eq!(pack.warnings, vec!["GRAPH_NODE_BUDGET_EXHAUSTED"]);
    }

    #[test]
    fn wiki_claim_cannot_upgrade_unknown_evidence_to_exact() {
        let mut uncertain = node("item:unknown", "미확인 재료");
        uncertain.evidence = evidence("fact:unknown", KnowledgeQuality::Unknown);
        let graph = KnowledgeGraph::build(identity(), vec![uncertain], vec![]).unwrap();
        let page = KnowledgeWikiPage {
            page_id: "wiki:unknown-item".to_owned(),
            title_ko: "미확인 재료".to_owned(),
            summary_ko: "검증되지 않은 항목입니다.".to_owned(),
            game_build_id: identity().game_build_id,
            dataset_manifest_id_hex: identity().dataset_manifest_id_hex,
            status: WikiPageStatus::Draft,
            claims: vec![WikiClaim {
                claim_id: "claim:one".to_owned(),
                text_ko: "확정 재료입니다.".to_owned(),
                quality: KnowledgeQuality::Exact,
                evidence_refs: vec!["fact:unknown".to_owned()],
                related_node_ids: vec!["item:unknown".to_owned()],
            }],
            related_node_ids: vec!["item:unknown".to_owned()],
        };

        assert_eq!(
            graph.validate_wiki_page(&page),
            Err(KnowledgeGraphError::UnsupportedClaimQuality(
                "claim:one".to_owned()
            ))
        );
    }

    #[test]
    fn reviewed_wiki_page_renders_claim_evidence_and_links() {
        let graph = fixture_graph();
        let page = KnowledgeWikiPage {
            page_id: "wiki:pal-a".to_owned(),
            title_ko: "팰 A 획득 정보".to_owned(),
            summary_ko: "검증된 드롭 관계를 정리합니다.".to_owned(),
            game_build_id: identity().game_build_id,
            dataset_manifest_id_hex: identity().dataset_manifest_id_hex,
            status: WikiPageStatus::Reviewed,
            claims: vec![WikiClaim {
                claim_id: "claim:drop".to_owned(),
                text_ko: "팰 A는 재료 B와 연결됩니다.".to_owned(),
                quality: KnowledgeQuality::Exact,
                evidence_refs: vec!["relation:edge:ab".to_owned()],
                related_node_ids: vec!["species:a".to_owned(), "item:b".to_owned()],
            }],
            related_node_ids: vec!["species:a".to_owned(), "item:b".to_owned()],
        };

        graph.validate_wiki_page(&page).unwrap();
        let markdown = page.render_markdown();
        assert!(markdown.contains("relation:edge:ab"));
        assert!(markdown.contains("[[species:a]]"));
        assert!(markdown.contains("[[item:b]]"));
    }

    #[test]
    fn retrieval_plan_is_bounded_and_routes_overviews_differently() {
        let relationship =
            KnowledgeRetrievalPlan::for_korean_question("페스키가 드롭하는 재료는 뭐야?");
        assert_eq!(relationship.intent, KnowledgeQueryIntent::Relationship);
        assert!(relationship.search_first);
        assert_eq!(relationship.maximum_graph_hops, 2);
        assert!(relationship.maximum_graph_nodes <= MAX_QUERY_NODES);

        let overview =
            KnowledgeRetrievalPlan::for_korean_question("모든 기술 종류를 한눈에 보여줘");
        assert_eq!(overview.intent, KnowledgeQueryIntent::Overview);
        assert!(!overview.search_first);
        assert!(overview.maximum_wiki_pages > relationship.maximum_wiki_pages);
    }
}
