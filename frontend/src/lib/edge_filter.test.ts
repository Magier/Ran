import { describe, expect, it } from 'vitest';
import { parseHiddenEdgeTypes, toggleHiddenEdgeType } from './edge_filter';

describe('edge graph filtering', () => {
	it('restores stored edge types', () => {
		expect(parseHiddenEdgeTypes('["can-reach","runs-on","can-reach"]')).toEqual(
			new Set(['can-reach', 'runs-on'])
		);
	});

	it('falls back safely for malformed stored values', () => {
		expect(parseHiddenEdgeTypes('{not json')).toEqual(new Set());
		expect(parseHiddenEdgeTypes('["can-reach", 3]')).toEqual(new Set());
	});

	it('toggles an edge type without mutating the current selection', () => {
		const current = new Set(['can-reach']);
		const shown = toggleHiddenEdgeType(current, 'can-reach');
		const hidden = toggleHiddenEdgeType(shown, 'runs-on');

		expect(current).toEqual(new Set(['can-reach']));
		expect(shown).toEqual(new Set());
		expect(hidden).toEqual(new Set(['runs-on']));
	});
});
