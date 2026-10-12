import { describe, expect, it } from 'vitest';
import type { Edge } from '#lib/api/index.js';
import { sessionOptions } from './sessions';

const session = (sessionId: string, targetId = 'node/victim', broken = false): Edge => ({
	id: `c2/ran-[c2.session]->${targetId}#${sessionId}`,
	sourceId: 'c2/ran',
	targetId,
	name: 'c2.session',
	sessionId,
	broken
});

describe('sessionOptions', () => {
	it('keeps both same-host callbacks selectable and retains the survivor after loss', () => {
		const first = session('session/listener/1');
		const second = session('session/listener/2');
		expect(sessionOptions([first, second], 'c2/ran').map((option) => option.value)).toEqual([
			first.sessionId,
			second.sessionId
		]);
		expect(sessionOptions([first, { ...second, broken: true }], 'c2/ran')).toEqual([
			{ label: `node/victim (${first.sessionId})`, value: first.sessionId }
		]);
	});
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
