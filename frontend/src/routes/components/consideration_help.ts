export type ConsiderationHelp = {
	label: string;
	kind: 'utility' | 'belief';
	summary: string;
	formula: string;
	details: string[];
	constants?: string[];
};

/**
 * Developer documentation for the raw measurements implemented by utility-ai.
 * Keep this in sync with crates/utility-ai/src/considerations.rs. Profile-level
 * curve and weight controls are documented separately in the tuner.
 */
export const considerationHelp: Record<string, ConsiderationHelp> = {
	epistemic_value: {
		label: 'Epistemic value',
		kind: 'utility',
		summary: 'How much useful, currently unknown information the action is expected to reveal.',
		formula: 'raw = discovery magnitude × freshness',
		details: [
			'Magnitude = 1 − 0.5^(sum of the generality of declared Discovery effects).',
			'Discovery and Reconnaissance tactics receive a minimum magnitude of 0.30.',
			'Freshness is 1.00 until this exact TTP × target succeeds, then immediately becomes 0.00.',
			'For volatile effects, freshness later recovers as 1 − 1/(1 + successful actions since the check). Stable effects remain at zero.'
		],
		constants: [
			'Effect generality: foundational 1.0; standard 0.6; specialized 0.3.',
			'SelfSubjectRulesReview is specialized and volatile: first raw score 0.30, then 0.00 immediately after success.'
		]
	},
	reliability: {
		label: 'Reliability',
		kind: 'belief',
		summary: 'Estimated probability that the technique works and its required tool is available.',
		formula: 'raw = ((successes + prior × 2) / (runs + 2)) × tool readiness',
		details: [
			'Execution history is aggregated across all targets for this TTP; cleanup runs are excluded.',
			'Tool readiness is evaluated for this target. Known-present is 1.0; unknown is discounted; known-absent candidates are filtered before scoring.'
		],
		constants: [
			'Status prior: stable 0.90; enabled 0.70; unknown 0.50.',
			'Prior strength: 2 pseudo-runs.'
		]
	},
	cost: {
		label: 'Cost',
		kind: 'utility',
		summary: 'Preference for the cheapest available procedure variant.',
		formula: 'raw = 1 / minimum procedure cost',
		details: ['Each procedure starts at cost 1. The cheapest procedure determines the TTP score.'],
		constants: ['Multi-step +2; HTTP request +1; Kubernetes request +1; each chained “&&” +1.']
	},
	stealth: {
		label: 'Stealth',
		kind: 'utility',
		summary: 'How quietly the action is expected to blend into the environment.',
		formula: 'raw = 1 − max(procedure risk, effect risk)',
		details: [
			'Procedure risk is inferred from shell commands, Kubernetes API verbs/resources, HTTP use, and multi-step execution.',
			'Effect risk is a static prior: ordinary inventory is low; secret access, execution, escape, and RBAC mutation are high.'
		]
	},
	privilege_gain: {
		label: 'Privilege gain',
		kind: 'utility',
		summary: 'Value of new execution or escape capabilities declared by the TTP.',
		formula: 'raw = (1 − 0.5^(privilege-effect count)) × pragmatic freshness',
		details: [
			'Pragmatic freshness is 1.00 before this TTP × target succeeds and permanently 0.00 afterward.'
		]
	},
	reachability: {
		label: 'Reachability',
		kind: 'utility',
		summary: 'Value of new sessions, routes, or operating positions declared by the TTP.',
		formula: 'raw = (1 − 0.5^(reachability-effect count)) × pragmatic freshness',
		details: [
			'Pragmatic freshness is 1.00 before this TTP × target succeeds and permanently 0.00 afterward.'
		]
	}
};

export function helpFor(name: string): ConsiderationHelp | undefined {
	return considerationHelp[name];
}
