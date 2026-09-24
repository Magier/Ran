import { describe, expect, it } from 'vitest';
import type { Edge } from '$lib/api/index';
import { sessionOptions } from './sessions';

const session = (sessionId: string, targetId = 'node/victim', broken = false): Edge => ({
	id: `c2/ran-[c2.session]->${targetId}`,
	sourceId: 'c2/ran',
	targetId,
	name: 'c2.session',
	sessionId,
	broken
});

describe('sessionOptions', () => {
	it('lists only live sessions belonging to the selected C2', () => {
		expect(
			sessionOptions(
				[
					session('session/z', 'node/z'),
					session('session/broken', 'node/broken', true),
					{ ...session('session/other'), sourceId: 'c2/other' },
					{ ...session('session/not-session'), name: 'can-reach' }
				],
				'c2/ran'
			)
		).toEqual([{ label: 'node/z (session/z)', value: 'session/z' }]);
	});
});
