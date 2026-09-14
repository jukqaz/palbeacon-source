use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_RULES: usize = 1_048_576;
pub const MAX_OWNED_PALS: usize = 10_000;
pub const MAX_GENERATIONS: u32 = 32;
pub const MAX_NODE_BUDGET: u64 = 2_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedPalSeed {
    pub instance_id_hex: String,
    pub species_id: String,
    #[serde(default)]
    pub passive_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreedingRule {
    pub rule_id: String,
    pub parent_a_species_id: String,
    pub parent_b_species_id: String,
    pub child_species_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BreedingPlanSource {
    Owned {
        instance_id_hex: String,
    },
    Bred {
        rule_id: String,
        parent_a_species_id: String,
        parent_b_species_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreedingPlanNode {
    pub species_id: String,
    pub generation: u32,
    pub source: BreedingPlanSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassiveCoverage {
    pub passive_id: String,
    pub owned_donor_instance_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreedingPlan {
    pub target_species_id: String,
    pub generations: u32,
    pub nodes: Vec<BreedingPlanNode>,
    pub required_passive_coverage: Vec<PassiveCoverage>,
    pub node_visits: u64,
    /// Evidence quality for the narrowly declared calculation scope.
    pub quality: String,
    /// The exact scope is species-graph reachability. It does not claim that
    /// currently owned instance genders can execute every edge, nor that a
    /// passive will be inherited.
    pub scope: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum BreedingError {
    #[error("target species is invalid")]
    InvalidTarget,
    #[error("too many owned Pals")]
    TooManyOwnedPals,
    #[error("too many breeding rules")]
    TooManyRules,
    #[error("maximum generations must be between 0 and {MAX_GENERATIONS}")]
    InvalidGenerationLimit,
    #[error("node budget must be between 1 and {MAX_NODE_BUDGET}")]
    InvalidNodeBudget,
    #[error("breeding node budget exhausted")]
    BudgetExhausted,
    #[error("target is unreachable from the supplied owned species")]
    Unreachable,
    #[error("invalid identifier in breeding input")]
    InvalidIdentifier,
}

#[derive(Clone, Debug)]
struct KnownSpecies {
    generation: u32,
    source: BreedingPlanSource,
}

pub fn solve_owned_breeding(
    owned: &[OwnedPalSeed],
    rules: &[BreedingRule],
    target_species_id: &str,
    required_passive_ids: &[String],
    maximum_generations: u32,
    node_budget: u64,
) -> Result<BreedingPlan, BreedingError> {
    validate_inputs(
        owned,
        rules,
        target_species_id,
        required_passive_ids,
        maximum_generations,
        node_budget,
    )?;

    let mut owned_sorted = owned.to_vec();
    owned_sorted.sort_by(|left, right| {
        left.species_id
            .cmp(&right.species_id)
            .then(left.instance_id_hex.cmp(&right.instance_id_hex))
    });
    let mut known = BTreeMap::<String, KnownSpecies>::new();
    for pal in &owned_sorted {
        known
            .entry(pal.species_id.clone())
            .or_insert_with(|| KnownSpecies {
                generation: 0,
                source: BreedingPlanSource::Owned {
                    instance_id_hex: pal.instance_id_hex.clone(),
                },
            });
    }

    let mut rules_sorted = rules.to_vec();
    rules_sorted.sort_by(|left, right| {
        left.child_species_id
            .cmp(&right.child_species_id)
            .then(left.rule_id.cmp(&right.rule_id))
            .then(left.parent_a_species_id.cmp(&right.parent_a_species_id))
            .then(left.parent_b_species_id.cmp(&right.parent_b_species_id))
    });

    let mut visits = 0u64;
    let mut changed = true;
    while changed {
        changed = false;
        for rule in &rules_sorted {
            visits = visits
                .checked_add(1)
                .ok_or(BreedingError::BudgetExhausted)?;
            if visits > node_budget {
                return Err(BreedingError::BudgetExhausted);
            }

            let Some(parent_a) = known.get(&rule.parent_a_species_id) else {
                continue;
            };
            let Some(parent_b) = known.get(&rule.parent_b_species_id) else {
                continue;
            };
            let generation = parent_a.generation.max(parent_b.generation) + 1;
            if generation > maximum_generations {
                continue;
            }
            let candidate = KnownSpecies {
                generation,
                source: BreedingPlanSource::Bred {
                    rule_id: rule.rule_id.clone(),
                    parent_a_species_id: rule.parent_a_species_id.clone(),
                    parent_b_species_id: rule.parent_b_species_id.clone(),
                },
            };
            let replace = match known.get(&rule.child_species_id) {
                None => true,
                Some(current) => candidate_rank(&candidate) < candidate_rank(current),
            };
            if replace {
                known.insert(rule.child_species_id.clone(), candidate);
                changed = true;
            }
        }
    }

    let target = known
        .get(target_species_id)
        .ok_or(BreedingError::Unreachable)?;
    let mut required = BTreeSet::new();
    collect_required_nodes(target_species_id, &known, &mut required);
    let nodes = required
        .into_iter()
        .map(|species_id| {
            let known = known
                .get(&species_id)
                .expect("required nodes are collected only from known species");
            BreedingPlanNode {
                species_id,
                generation: known.generation,
                source: known.source.clone(),
            }
        })
        .collect();

    let mut requested = required_passive_ids.to_vec();
    requested.sort();
    requested.dedup();
    let coverage = requested
        .into_iter()
        .map(|passive_id| {
            let mut donors = owned_sorted
                .iter()
                .filter(|pal| pal.passive_ids.contains(&passive_id))
                .map(|pal| pal.instance_id_hex.clone())
                .collect::<Vec<_>>();
            donors.sort();
            donors.dedup();
            PassiveCoverage {
                passive_id,
                owned_donor_instance_ids: donors,
            }
        })
        .collect::<Vec<_>>();
    let mut warnings = Vec::new();
    if coverage
        .iter()
        .any(|entry| entry.owned_donor_instance_ids.is_empty())
    {
        warnings.push("MISSING_REQUIRED_PASSIVE_DONOR".to_owned());
    }
    if !coverage.is_empty() {
        warnings.push("PASSIVE_INHERITANCE_PROBABILITY_UNKNOWN".to_owned());
    }
    warnings.push("PARENT_GENDER_NOT_EVALUATED".to_owned());
    warnings.push("DISTINCT_PARENT_INSTANCE_NOT_EVALUATED".to_owned());

    Ok(BreedingPlan {
        target_species_id: target_species_id.to_owned(),
        generations: target.generation,
        nodes,
        required_passive_coverage: coverage,
        node_visits: visits,
        quality: "exact".to_owned(),
        scope: "species_graph_reachability".to_owned(),
        warnings,
    })
}

fn collect_required_nodes(
    species_id: &str,
    known: &BTreeMap<String, KnownSpecies>,
    output: &mut BTreeSet<String>,
) {
    if !output.insert(species_id.to_owned()) {
        return;
    }
    let Some(node) = known.get(species_id) else {
        return;
    };
    if let BreedingPlanSource::Bred {
        parent_a_species_id,
        parent_b_species_id,
        ..
    } = &node.source
    {
        collect_required_nodes(parent_a_species_id, known, output);
        collect_required_nodes(parent_b_species_id, known, output);
    }
}

fn candidate_rank(candidate: &KnownSpecies) -> (u32, String, String, String) {
    match &candidate.source {
        BreedingPlanSource::Owned { instance_id_hex } => (
            candidate.generation,
            String::new(),
            String::new(),
            instance_id_hex.clone(),
        ),
        BreedingPlanSource::Bred {
            rule_id,
            parent_a_species_id,
            parent_b_species_id,
        } => (
            candidate.generation,
            rule_id.clone(),
            parent_a_species_id.clone(),
            parent_b_species_id.clone(),
        ),
    }
}

fn validate_inputs(
    owned: &[OwnedPalSeed],
    rules: &[BreedingRule],
    target: &str,
    required_passive_ids: &[String],
    generations: u32,
    budget: u64,
) -> Result<(), BreedingError> {
    if owned.len() > MAX_OWNED_PALS {
        return Err(BreedingError::TooManyOwnedPals);
    }
    if rules.len() > MAX_RULES {
        return Err(BreedingError::TooManyRules);
    }
    if generations > MAX_GENERATIONS {
        return Err(BreedingError::InvalidGenerationLimit);
    }
    if budget == 0 || budget > MAX_NODE_BUDGET {
        return Err(BreedingError::InvalidNodeBudget);
    }
    if !valid_id(target) {
        return Err(BreedingError::InvalidTarget);
    }
    for pal in owned {
        if !valid_hex_id(&pal.instance_id_hex)
            || !valid_id(&pal.species_id)
            || pal.passive_ids.iter().any(|id| !valid_id(id))
        {
            return Err(BreedingError::InvalidIdentifier);
        }
    }
    for rule in rules {
        if !valid_id(&rule.rule_id)
            || !valid_id(&rule.parent_a_species_id)
            || !valid_id(&rule.parent_b_species_id)
            || !valid_id(&rule.child_species_id)
        {
            return Err(BreedingError::InvalidIdentifier);
        }
    }
    if required_passive_ids.iter().any(|id| !valid_id(id)) {
        return Err(BreedingError::InvalidIdentifier);
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.is_ascii()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn valid_hex_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pal(instance: u8, species: &str, passives: &[&str]) -> OwnedPalSeed {
        OwnedPalSeed {
            instance_id_hex: format!("{instance:032x}"),
            species_id: species.to_owned(),
            passive_ids: passives.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    fn rule(id: &str, a: &str, b: &str, child: &str) -> BreedingRule {
        BreedingRule {
            rule_id: id.to_owned(),
            parent_a_species_id: a.to_owned(),
            parent_b_species_id: b.to_owned(),
            child_species_id: child.to_owned(),
        }
    }

    #[test]
    fn finds_deterministic_multi_generation_plan() {
        let result = solve_owned_breeding(
            &[pal(1, "A", &["swift"]), pal(2, "B", &[]), pal(3, "C", &[])],
            &[rule("r2", "D", "C", "Target"), rule("r1", "A", "B", "D")],
            "Target",
            &["swift".to_owned()],
            4,
            100,
        )
        .unwrap();

        assert_eq!(result.generations, 2);
        assert_eq!(
            result
                .nodes
                .iter()
                .map(|node| node.species_id.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B", "C", "D", "Target"]
        );
        assert_eq!(
            result.required_passive_coverage[0].owned_donor_instance_ids,
            vec![format!("{:032x}", 1)]
        );
        assert!(
            result
                .warnings
                .contains(&"PASSIVE_INHERITANCE_PROBABILITY_UNKNOWN".to_owned())
        );
        assert_eq!(result.quality, "exact");
        assert_eq!(result.scope, "species_graph_reachability");
        assert!(
            result
                .warnings
                .contains(&"PARENT_GENDER_NOT_EVALUATED".to_owned())
        );
        assert!(
            result
                .warnings
                .contains(&"DISTINCT_PARENT_INSTANCE_NOT_EVALUATED".to_owned())
        );
    }

    #[test]
    fn direct_owned_target_is_zero_generation() {
        let result =
            solve_owned_breeding(&[pal(9, "Target", &[])], &[], "Target", &[], 0, 10).unwrap();
        assert_eq!(result.generations, 0);
        assert_eq!(result.nodes.len(), 1);
    }

    #[test]
    fn generation_limit_and_budget_are_enforced() {
        let owned = [pal(1, "A", &[]), pal(2, "B", &[]), pal(3, "C", &[])];
        let rules = [rule("r1", "A", "B", "D"), rule("r2", "D", "C", "Target")];
        assert_eq!(
            solve_owned_breeding(&owned, &rules, "Target", &[], 1, 100),
            Err(BreedingError::Unreachable)
        );
        assert_eq!(
            solve_owned_breeding(&owned, &rules, "Target", &[], 4, 1),
            Err(BreedingError::BudgetExhausted)
        );
    }

    #[test]
    fn missing_passive_donor_is_explicit_not_invented() {
        let result = solve_owned_breeding(
            &[pal(1, "Target", &[])],
            &[],
            "Target",
            &["legend".to_owned()],
            0,
            10,
        )
        .unwrap();
        assert!(
            result
                .warnings
                .contains(&"MISSING_REQUIRED_PASSIVE_DONOR".to_owned())
        );
        assert!(
            result.required_passive_coverage[0]
                .owned_donor_instance_ids
                .is_empty()
        );
    }
}
