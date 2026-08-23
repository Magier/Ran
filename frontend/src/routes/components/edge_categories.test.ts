import { describe, expect, it } from 'vitest';

import { isInformational } from './edge_categories';

describe('isInformational', () => {
	it('classifies contextual and reachability facts without treating execution edges as informational', () => {
		expect(isInformational('authenticates-to')).toBe(true);
		expect(isInformational('can-reach')).toBe(true);
		expect(isInformational('k8s.can-exec')).toBe(false);
		expect(isInformational('controls')).toBe(false);
	});
});
