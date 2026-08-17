import { describe, expect, it } from 'vitest';

import { isInformational } from './edge_categories';

describe('isInformational', () => {
	it('classifies authentication context without treating execution edges as informational', () => {
		expect(isInformational('authenticates-to')).toBe(true);
		expect(isInformational('controls')).toBe(false);
	});
});
