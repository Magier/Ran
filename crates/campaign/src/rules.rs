use crate::{Campaign, FactsUpdate, KnowledgeProvenance};

pub trait InferenceRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate;
}

pub fn run_rules_fixpoint(
    campaign: &Campaign,
    rules: &[Box<dyn InferenceRule>],
    initial: FactsUpdate,
) -> FactsUpdate {
    let mut acc = initial;
    let mut iteration = 0;
    let max_iterations = 8;

    loop {
        if iteration >= max_iterations {
            break;
        }

        let mut changed = false;
        let mut next = FactsUpdate::default();

        for rule in rules {
            let mut inferred = rule.infer(campaign, &acc);
            inferred.attribute_unattributed(KnowledgeProvenance::Inference);
            let has_output = !inferred.new_entities.is_empty()
                || !inferred.new_relations.is_empty()
                || !inferred.entity_aliases.is_empty();
            if has_output {
                changed = true;
                next.merge(inferred);
            }
        }

        if !changed {
            break;
        }

        acc.merge(next);
        iteration += 1;
    }

    acc
}
