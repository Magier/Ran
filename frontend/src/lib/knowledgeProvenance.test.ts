import { describe, expect, it } from 'vitest';
import { hasKnowledgeProvenance, knowledgeProvenanceBadges } from './knowledgeProvenance';

describe('knowledge provenance presentation', () => {
	it('builds scenario and operator badges in source order', () => {
		expect(knowledgeProvenanceBadges(['scenario', 'operator'])).toEqual([
			{ origin: 'scenario', label: 'Scenario-provided' },
			{ origin: 'operator', label: 'Operator-provided' }
		]);
	});

	it('detects scenario styling without accepting absent provenance', () => {
		expect(hasKnowledgeProvenance(['scenario'], 'scenario')).toBe(true);
		expect(hasKnowledgeProvenance(undefined, 'scenario')).toBe(false);
	});
});
