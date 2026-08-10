import { describe, expect, it } from 'vitest';
import { hasKnowledgeProvenance, knowledgeProvenanceBadges } from './knowledgeProvenance';

describe('knowledge provenance presentation', () => {
	it('builds scenario and inference badges in source order', () => {
		expect(knowledgeProvenanceBadges(['inference', 'scenario'])).toEqual([
			{ origin: 'inference', label: 'Inferred' },
			{ origin: 'scenario', label: 'Scenario-provided' }
		]);
	});

	it('does not build operator or action badges', () => {
		expect(knowledgeProvenanceBadges(['operator', 'action'])).toEqual([]);
	});

	it('detects scenario styling without accepting absent provenance', () => {
		expect(hasKnowledgeProvenance(['scenario'], 'scenario')).toBe(true);
		expect(hasKnowledgeProvenance(undefined, 'scenario')).toBe(false);
	});
});
