export type KnowledgeProvenance = 'scenario' | 'operator' | 'action' | 'inference';

const LABELS: Record<KnowledgeProvenance, string> = {
	scenario: 'Scenario-provided',
	operator: 'Operator-provided',
	action: 'Action-discovered',
	inference: 'Inferred'
};

export function hasKnowledgeProvenance(
	values: readonly string[] | undefined,
	origin: string
): boolean {
	return values?.includes(origin) ?? false;
}

export function knowledgeProvenanceBadges(values: readonly string[] | undefined) {
	return (values ?? [])
		.filter((value): value is KnowledgeProvenance => value in LABELS)
		.map((origin) => ({ origin, label: LABELS[origin] }));
}
