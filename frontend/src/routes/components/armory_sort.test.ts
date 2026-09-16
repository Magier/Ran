import { describe, expect, it } from 'vitest';
import type { TTP } from '$lib/api';
import { sortTtpsByName } from './armory_sort';

function ttp(id: string, name: string): TTP {
	return {
		id,
		name,
		description: '',
		tactic: 'Execution',
		techniques: [],
		status: 'enabled',
		params: [],
		requires: {},
		effects: [],
		procedures: []
	};
}

describe('sortTtpsByName', () => {
	it('sorts actions alphabetically by their displayed name without mutating the source', () => {
		const source = [ttp('zulu', 'Zulu'), ttp('alpha-lower', 'alpha'), ttp('bravo', 'Bravo')];

		expect(sortTtpsByName(source).map(({ id }) => id)).toEqual(['alpha-lower', 'bravo', 'zulu']);
		expect(source.map(({ id }) => id)).toEqual(['zulu', 'alpha-lower', 'bravo']);
	});

	it('uses the action id to make duplicate names deterministic', () => {
		const source = [ttp('second', 'Duplicate'), ttp('first', 'Duplicate')];

		expect(sortTtpsByName(source).map(({ id }) => id)).toEqual(['first', 'second']);
	});
});
