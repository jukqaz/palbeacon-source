use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

const MAX_SAMPLES_PER_FINDING: usize = 20;

#[derive(Debug)]
pub(super) struct ReferenceIndex {
    exact: BTreeSet<String>,
    case_folded: BTreeMap<String, Vec<String>>,
}

impl ReferenceIndex {
    pub(super) fn new(ids: impl IntoIterator<Item = String>) -> Self {
        let exact = ids.into_iter().collect::<BTreeSet<_>>();
        let mut case_folded = BTreeMap::<String, Vec<String>>::new();
        for id in &exact {
            case_folded
                .entry(id.to_ascii_lowercase())
                .or_default()
                .push(id.clone());
        }
        Self { exact, case_folded }
    }

    pub(super) fn resolve(
        &self,
        raw_id: &str,
        removable_namespace: Option<&str>,
    ) -> ReferenceResolution {
        if self.exact.contains(raw_id) {
            return ReferenceResolution::exact(raw_id);
        }

        let trimmed = raw_id.trim();
        if trimmed != raw_id && self.exact.contains(trimmed) {
            return ReferenceResolution::canonicalized(trimmed, "trimmed_exact");
        }

        let namespaced = removable_namespace
            .and_then(|namespace| trimmed.strip_prefix(namespace))
            .unwrap_or(trimmed);
        if namespaced != trimmed && self.exact.contains(namespaced) {
            return ReferenceResolution::canonicalized(namespaced, "namespace_exact");
        }

        let folded = namespaced.to_ascii_lowercase();
        match self.case_folded.get(&folded).map(Vec::as_slice) {
            Some([canonical]) => {
                ReferenceResolution::canonicalized(canonical, "unique_ascii_case_fold")
            }
            Some(candidates) => ReferenceResolution {
                output_id: raw_id.to_owned(),
                state: ResolutionState::Ambiguous,
                method: "ambiguous_ascii_case_fold",
                candidates: candidates.to_vec(),
            },
            None => ReferenceResolution {
                output_id: raw_id.to_owned(),
                state: ResolutionState::Missing,
                method: "target_absent",
                candidates: Vec::new(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResolutionState {
    Exact,
    Canonicalized,
    Missing,
    Ambiguous,
}

#[derive(Debug)]
pub(super) struct ReferenceResolution {
    pub(super) output_id: String,
    state: ResolutionState,
    method: &'static str,
    candidates: Vec<String>,
}

impl ReferenceResolution {
    fn exact(id: &str) -> Self {
        Self {
            output_id: id.to_owned(),
            state: ResolutionState::Exact,
            method: "exact",
            candidates: Vec::new(),
        }
    }

    fn canonicalized(id: &str, method: &'static str) -> Self {
        Self {
            output_id: id.to_owned(),
            state: ResolutionState::Canonicalized,
            method,
            candidates: Vec::new(),
        }
    }

    pub(super) fn method(&self) -> &'static str {
        self.method
    }
}

#[derive(Debug)]
pub(super) struct QualityReportBuilder {
    dataset_version: String,
    game_build_id: String,
    source_hash: String,
    exact_match_count: usize,
    canonicalized_match_count: usize,
    duplicate_reference_count: usize,
    unresolved_reference_count: usize,
    ambiguous_reference_count: usize,
    findings: BTreeMap<FindingKey, FindingAccumulator>,
}

impl QualityReportBuilder {
    pub(super) fn new(dataset_version: &str, game_build_id: &str, source_hash: &str) -> Self {
        Self {
            dataset_version: dataset_version.to_owned(),
            game_build_id: game_build_id.to_owned(),
            source_hash: source_hash.to_owned(),
            exact_match_count: 0,
            canonicalized_match_count: 0,
            duplicate_reference_count: 0,
            unresolved_reference_count: 0,
            ambiguous_reference_count: 0,
            findings: BTreeMap::new(),
        }
    }

    pub(super) fn record_resolution(
        &mut self,
        source_kind: &'static str,
        target_kind: &'static str,
        subject_id: &str,
        raw_reference_id: &str,
        resolution: &ReferenceResolution,
    ) {
        match resolution.state {
            ResolutionState::Exact => {
                self.exact_match_count += 1;
            }
            ResolutionState::Canonicalized => {
                self.canonicalized_match_count += 1;
                self.record_finding(
                    FindingKey {
                        status: "canonicalized",
                        severity: "info",
                        source_kind,
                        target_kind,
                        cause: resolution.method,
                    },
                    subject_id,
                    raw_reference_id,
                    Some(&resolution.output_id),
                    &resolution.candidates,
                );
            }
            ResolutionState::Missing => {
                self.unresolved_reference_count += 1;
                let (cause, severity) = missing_cause(source_kind, raw_reference_id);
                self.record_finding(
                    FindingKey {
                        status: "unresolved",
                        severity,
                        source_kind,
                        target_kind,
                        cause,
                    },
                    subject_id,
                    raw_reference_id,
                    None,
                    &resolution.candidates,
                );
            }
            ResolutionState::Ambiguous => {
                self.ambiguous_reference_count += 1;
                self.record_finding(
                    FindingKey {
                        status: "ambiguous",
                        severity: "error",
                        source_kind,
                        target_kind,
                        cause: resolution.method,
                    },
                    subject_id,
                    raw_reference_id,
                    None,
                    &resolution.candidates,
                );
            }
        }
    }

    pub(super) fn record_duplicate(
        &mut self,
        source_kind: &'static str,
        target_kind: &'static str,
        subject_id: &str,
        raw_reference_id: &str,
        canonical_reference_id: &str,
    ) {
        self.duplicate_reference_count += 1;
        self.record_finding(
            FindingKey {
                status: "duplicate_removed",
                severity: "warning",
                source_kind,
                target_kind,
                cause: "exact_duplicate_after_canonicalization",
            },
            subject_id,
            raw_reference_id,
            Some(canonical_reference_id),
            &[],
        );
    }

    pub(super) fn finish(self, verified_error_book_count: usize) -> KnowledgeDataQualityReport {
        let distinct_reference_count = self.exact_match_count
            + self.canonicalized_match_count
            + self.unresolved_reference_count
            + self.ambiguous_reference_count;
        let input_reference_count = distinct_reference_count + self.duplicate_reference_count;
        let predicted_error_book_count_before_normalization = self.canonicalized_match_count
            + self.unresolved_reference_count
            + self.ambiguous_reference_count;
        let mut unresolved_by_source = BTreeMap::<String, usize>::new();
        let mut unresolved_by_severity = BTreeMap::<String, usize>::new();
        let mut unresolved_by_cause = BTreeMap::<String, usize>::new();
        let findings = self
            .findings
            .into_iter()
            .map(|(key, accumulator)| {
                if matches!(key.status, "unresolved" | "ambiguous") {
                    *unresolved_by_source
                        .entry(key.source_kind.to_owned())
                        .or_default() += accumulator.occurrence_count;
                    *unresolved_by_severity
                        .entry(key.severity.to_owned())
                        .or_default() += accumulator.occurrence_count;
                    *unresolved_by_cause.entry(key.cause.to_owned()).or_default() +=
                        accumulator.occurrence_count;
                }
                QualityFinding {
                    status: key.status,
                    severity: key.severity,
                    source_kind: key.source_kind,
                    target_kind: key.target_kind,
                    cause: key.cause,
                    occurrence_count: accumulator.occurrence_count,
                    affected_subject_count: accumulator.subject_ids.len(),
                    distinct_reference_count: accumulator.reference_ids.len(),
                    samples: accumulator
                        .samples
                        .into_iter()
                        .take(MAX_SAMPLES_PER_FINDING)
                        .collect(),
                }
            })
            .collect();
        let publication_recommendation = if verified_error_book_count == 0 {
            "eligible_for_exact_build_review"
        } else {
            "relationship_review_required"
        };

        KnowledgeDataQualityReport {
            schema: "pal-companion-knowledge-data-quality-v1",
            dataset_version: self.dataset_version,
            game_build_id: self.game_build_id,
            source_sha256: self.source_hash,
            authority_policy: "Only exact-build sources are authoritative. Canonicalization is limited to deterministic token normalization with exactly one catalog target; unresolved relations are never invented.",
            classifier_version: 1,
            summary: QualitySummary {
                input_reference_count,
                distinct_reference_count_after_deduplication: distinct_reference_count,
                exact_match_count: self.exact_match_count,
                canonicalized_match_count: self.canonicalized_match_count,
                duplicate_reference_count: self.duplicate_reference_count,
                unresolved_reference_count: self.unresolved_reference_count,
                ambiguous_reference_count: self.ambiguous_reference_count,
                predicted_error_book_count_before_normalization,
                verified_error_book_count_after_normalization: verified_error_book_count,
                error_book_reduction_count: predicted_error_book_count_before_normalization
                    .saturating_sub(verified_error_book_count),
            },
            unresolved_by_source,
            unresolved_by_severity,
            unresolved_by_cause,
            findings,
            publication_recommendation,
        }
    }

    fn record_finding(
        &mut self,
        key: FindingKey,
        subject_id: &str,
        reference_id: &str,
        canonical_reference_id: Option<&str>,
        candidates: &[String],
    ) {
        let accumulator = self.findings.entry(key).or_default();
        accumulator.occurrence_count += 1;
        accumulator.subject_ids.insert(subject_id.to_owned());
        accumulator.reference_ids.insert(reference_id.to_owned());
        accumulator.samples.insert(QualitySample {
            subject_id: subject_id.to_owned(),
            reference_id: reference_id.to_owned(),
            canonical_reference_id: canonical_reference_id.map(str::to_owned),
            candidates: candidates.to_vec(),
        });
    }
}

#[derive(Debug, Serialize)]
pub(super) struct KnowledgeDataQualityReport {
    schema: &'static str,
    dataset_version: String,
    game_build_id: String,
    source_sha256: String,
    authority_policy: &'static str,
    classifier_version: u32,
    pub(super) summary: QualitySummary,
    unresolved_by_source: BTreeMap<String, usize>,
    unresolved_by_severity: BTreeMap<String, usize>,
    unresolved_by_cause: BTreeMap<String, usize>,
    findings: Vec<QualityFinding>,
    publication_recommendation: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct QualitySummary {
    pub(super) input_reference_count: usize,
    pub(super) distinct_reference_count_after_deduplication: usize,
    pub(super) exact_match_count: usize,
    pub(super) canonicalized_match_count: usize,
    pub(super) duplicate_reference_count: usize,
    pub(super) unresolved_reference_count: usize,
    pub(super) ambiguous_reference_count: usize,
    pub(super) predicted_error_book_count_before_normalization: usize,
    pub(super) verified_error_book_count_after_normalization: usize,
    pub(super) error_book_reduction_count: usize,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FindingKey {
    status: &'static str,
    severity: &'static str,
    source_kind: &'static str,
    target_kind: &'static str,
    cause: &'static str,
}

#[derive(Debug, Default)]
struct FindingAccumulator {
    occurrence_count: usize,
    subject_ids: BTreeSet<String>,
    reference_ids: BTreeSet<String>,
    samples: BTreeSet<QualitySample>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct QualitySample {
    subject_id: String,
    reference_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_reference_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    candidates: Vec<String>,
}

#[derive(Debug, Serialize)]
struct QualityFinding {
    status: &'static str,
    severity: &'static str,
    source_kind: &'static str,
    target_kind: &'static str,
    cause: &'static str,
    occurrence_count: usize,
    affected_subject_count: usize,
    distinct_reference_count: usize,
    samples: Vec<QualitySample>,
}

fn missing_cause(source_kind: &str, reference_id: &str) -> (&'static str, &'static str) {
    if source_kind != "item_catalog.pal_drops" {
        return ("target_absent_from_catalog", "error");
    }
    let folded = reference_id.to_ascii_lowercase();
    if folded.starts_with("boss_") {
        ("boss_variant_absent_from_species_catalog", "warning")
    } else if folded.starts_with("predator_") {
        ("predator_variant_absent_from_species_catalog", "warning")
    } else if folded.contains("_quest_") || folded.starts_with("quest_") {
        ("quest_variant_absent_from_species_catalog", "warning")
    } else if folded.contains("_oilrig") || folded.contains("_seabase") {
        ("area_variant_absent_from_species_catalog", "warning")
    } else {
        ("source_absent_from_species_catalog", "warning")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_only_canonicalizes_unique_deterministic_matches() {
        let index = ReferenceIndex::new([
            "BlueDragon_Ice".to_owned(),
            "DragonWave".to_owned(),
            "Case".to_owned(),
            "CASE".to_owned(),
        ]);

        let exact = index.resolve("DragonWave", Some("EPalWazaID::"));
        assert_eq!(exact.output_id, "DragonWave");
        assert_eq!(exact.state, ResolutionState::Exact);

        let namespaced = index.resolve("EPalWazaID::DragonWave", Some("EPalWazaID::"));
        assert_eq!(namespaced.output_id, "DragonWave");
        assert_eq!(namespaced.method, "namespace_exact");

        let case_only = index.resolve("BlueDragon_ice", None);
        assert_eq!(case_only.output_id, "BlueDragon_Ice");
        assert_eq!(case_only.method, "unique_ascii_case_fold");

        let ambiguous = index.resolve("case", None);
        assert_eq!(ambiguous.output_id, "case");
        assert_eq!(ambiguous.state, ResolutionState::Ambiguous);

        let missing = index.resolve("BOSS_Unknown", None);
        assert_eq!(missing.output_id, "BOSS_Unknown");
        assert_eq!(missing.state, ResolutionState::Missing);
    }

    #[test]
    fn report_is_deterministic_and_aggregates_unresolved_causes() {
        let index = ReferenceIndex::new(["BlueDragon_Ice".to_owned()]);
        let mut builder = QualityReportBuilder::new("fixture", "24181527", &"f".repeat(64));
        for (subject, reference) in [
            ("drop:2", "BOSS_Alpaca"),
            ("drop:1", "BlueDragon_ice"),
            ("drop:3", "PREDATOR_Anubis"),
        ] {
            let resolution = index.resolve(reference, None);
            builder.record_resolution(
                "item_catalog.pal_drops",
                "species",
                subject,
                reference,
                &resolution,
            );
        }
        builder.record_duplicate(
            "catalog_species.guaranteed_passives",
            "passive",
            "Anubis",
            "Swift",
            "Swift",
        );
        let report = builder.finish(2);

        assert_eq!(report.summary.input_reference_count, 4);
        assert_eq!(report.summary.canonicalized_match_count, 1);
        assert_eq!(report.summary.duplicate_reference_count, 1);
        assert_eq!(report.summary.unresolved_reference_count, 2);
        assert_eq!(
            report
                .summary
                .predicted_error_book_count_before_normalization,
            3
        );
        assert_eq!(
            report.summary.verified_error_book_count_after_normalization,
            2
        );
        assert_eq!(report.summary.error_book_reduction_count, 1);
        assert_eq!(
            report.unresolved_by_source.get("item_catalog.pal_drops"),
            Some(&2)
        );
        assert_eq!(
            report
                .unresolved_by_cause
                .get("boss_variant_absent_from_species_catalog"),
            Some(&1)
        );
        assert_eq!(
            report
                .unresolved_by_cause
                .get("predator_variant_absent_from_species_catalog"),
            Some(&1)
        );

        let first = serde_json::to_vec_pretty(&report).unwrap();
        let second = serde_json::to_vec_pretty(&report).unwrap();
        assert_eq!(first, second);
    }
}
