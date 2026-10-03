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

        let mut next = FactsUpdate::default();

        for rule in rules {
            let mut inferred = rule.infer(campaign, &acc);
            inferred.attribute_unattributed(KnowledgeProvenance::Inference);
            next.merge(inferred);
        }

        // Rules are intentionally allowed to be idempotent and re-emit facts
        // they can already see. Only facts that survive deduplication should
        // drive another iteration.
        if !acc.merge(next) {
            break;
        }

        iteration += 1;
    }

    acc
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use ran_domain::{Contains, K8sCluster, Namespace};

    use super::*;

    struct RepeatingRule {
        calls: Arc<AtomicUsize>,
    }

    impl InferenceRule for RepeatingRule {
        fn name(&self) -> &'static str {
            "test.repeating"
        }

        fn infer(&self, _campaign: &Campaign, _update: &FactsUpdate) -> FactsUpdate {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut inferred = FactsUpdate::default();
            inferred
                .new_relations
                .push(Box::new(Contains::new("k8s/cluster/test", "ns/default")));
            inferred
        }
    }

    #[test]
    fn fixpoint_stops_when_rules_only_reemit_known_facts() {
        let campaign = Campaign::bootstrap("ran", K8sCluster::new("test"));
        let calls = Arc::new(AtomicUsize::new(0));
        let rules: Vec<Box<dyn InferenceRule>> = vec![Box::new(RepeatingRule {
            calls: Arc::clone(&calls),
        })];
        let mut initial = FactsUpdate::default();
        initial
            .new_entities
            .push(Box::new(Namespace::new("default")));

        let inferred = run_rules_fixpoint(&campaign, &rules, initial);

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(inferred.new_relations.len(), 1);
    }
}
