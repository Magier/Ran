export type KnowledgeProvenance = 'scenario' | 'operator' | 'action' | 'inference';

type KnowledgeProvenanceBadge = Extract<KnowledgeProvenance, 'scenario' | 'inference'>;

const BADGE_LABELS: Record<KnowledgeProvenanceBadge, string> = {
	scenario: 'Scenario-provided',
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
		.filter((value): value is KnowledgeProvenanceBadge => value in BADGE_LABELS)
		.map((origin) => ({ origin, label: BADGE_LABELS[origin] }));
}
