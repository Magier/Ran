import { describe, expect, it } from 'vitest';
import type { Node } from '$lib/api/index';
import {
	allRedirectors,
	redirectorHop,
	redirectorOptions,
	redirectorsOf
} from './redirectors';

function redirector(playId: string, remotePort: number, listenerPort: number, via = 'labctl') {
	return {
		id: `redirector/${playId}/${remotePort}`,
		kind: 'Redirector',
		entry: `${playId}/${remotePort}`,
		label: `${via} ${remotePort}→${listenerPort}`,
		via,
		playId,
		remotePort,
		listenerPort
	};
}

function c2Node(redirectors: unknown, id = 'c2/ran'): Node {
	return { id, entityId: id, kind: 'C2', name: 'Ran', entity: { name: 'Ran', redirectors } };
}

describe('redirectorsOf', () => {
	it('reads the redirectors folded into the node payload', () => {
		const redirectors = redirectorsOf(
			c2Node([redirector('play1', 1337, 4444), redirector('play2', 1338, 8080)])
		);

		expect(redirectors).toEqual([
			{
				id: 'redirector/play1/1337',
				via: 'labctl',
				playId: 'play1',
				remotePort: 1337,
				listenerPort: 4444,
				entry: 'play1/1337',
				label: 'labctl 1337→4444'
			},
			{
				id: 'redirector/play2/1338',
				via: 'labctl',
				playId: 'play2',
				remotePort: 1338,
				listenerPort: 8080,
				entry: 'play2/1338',
				label: 'labctl 1338→8080'
			}
		]);
	});

	it('falls back to playId/remotePort when the payload carries no entry', () => {
		const redirectors = redirectorsOf(
			c2Node([
				{
					id: 'redirector/play1/1337',
					via: 'labctl',
					playId: 'play1',
					remotePort: 1337,
					listenerPort: 4444
				}
			])
		);

		expect(redirectors[0].entry).toBe('play1/1337');
	});

	it('rebuilds the label from the tool and the hop when the payload has none', () => {
		const redirectors = redirectorsOf(
			c2Node([
				{
					id: 'redirector/play1/1337',
					via: 'labctl',
					playId: 'play1',
					remotePort: 1337,
					listenerPort: 4444
				}
			])
		);

		expect(redirectors[0].label).toBe('labctl 1337→4444');
	});

	it('still labels a redirector whose tool is unknown', () => {
		const redirectors = redirectorsOf(
			c2Node([{ id: 'redirector/play1/1337', playId: 'play1', remotePort: 1337, listenerPort: 4444 }])
		);

		expect(redirectors[0].via).toBe('');
		expect(redirectors[0].label).toBe('1337→4444');
	});

	it('drops entries that could not identify a redirector', () => {
		const redirectors = redirectorsOf(
			c2Node([
				redirector('play1', 1337, 4444),
				// A duplicate id is the same tunnel reported twice.
				redirector('play1', 1337, 4444),
				// No id, so nothing to select and nothing to stop.
				{ playId: 'play1', remotePort: 1337, listenerPort: 4444 },
				// A redirector with no listener port cannot describe its own hop.
				{ id: 'redirector/play1/1400', playId: 'play1', remotePort: 1400 },
				'play1/1337',
				null
			])
		);

		expect(redirectors.map((r) => r.id)).toEqual(['redirector/play1/1337']);
	});

	it('tolerates a node with no redirectors at all', () => {
		expect(redirectorsOf(undefined)).toEqual([]);
		expect(redirectorsOf(c2Node(undefined))).toEqual([]);
		expect(redirectorsOf({ id: 'c2/x', entityId: 'c2/x', kind: 'C2', name: 'x' })).toEqual([]);
	});
});

describe('allRedirectors', () => {
	it('collects redirectors across every node', () => {
		const nodes = [
			c2Node([redirector('play1', 1337, 4444)]),
			c2Node([redirector('play2', 1338, 8080)], 'c2/other'),
			{ id: 'pod/one', entityId: 'pod/one', kind: 'Pod', name: 'one' } as Node
		];

		expect(allRedirectors(nodes).map((r) => r.id)).toEqual([
			'redirector/play1/1337',
			'redirector/play2/1338'
		]);
	});

	it('tolerates missing graph data', () => {
		expect(allRedirectors(undefined)).toEqual([]);
	});
});

describe('redirectorHop', () => {
	it('reads in traffic direction', () => {
		expect(redirectorHop(redirector('play1', 1337, 4444))).toBe('1337→4444');
	});
});

describe('redirectorOptions', () => {
	it('labels by tool and hop, leaving the playground id out', () => {
		const options = redirectorOptions([
			c2Node([redirector('zn1kqxk3ykpvxp5x', 9000, 4444)])
		]);

		expect(options).toEqual([
			{ label: 'labctl 9000→4444', value: 'redirector/zn1kqxk3ykpvxp5x/9000' }
		]);
	});

	it('adds the playground id only where two would otherwise read alike', () => {
		const options = redirectorOptions([
			c2Node([
				redirector('play1', 9000, 4444),
				redirector('play2', 9000, 4444),
				redirector('play3', 9001, 4444)
			])
		]);

		expect(options.map((o) => o.label)).toEqual([
			'labctl 9000→4444 (play1)',
			'labctl 9000→4444 (play2)',
			// Unambiguous on its own, so it stays clean.
			'labctl 9001→4444'
		]);
	});

	it('tolerates missing graph data', () => {
		expect(redirectorOptions(undefined)).toEqual([]);
	});
});
