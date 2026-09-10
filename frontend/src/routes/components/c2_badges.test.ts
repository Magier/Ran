import { describe, expect, it } from 'vitest';
import type { Node } from '$lib/api/index';
import { c2Badges } from './c2_badges';

function listener(protocol: string, port: number) {
	return {
		id: `listener/${protocol}/${port}`,
		kind: 'Listener',
		entry: `${protocol}/${port}`,
		protocol,
		port
	};
}

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

function c2Node(listeners: unknown, id = 'c2/ran', redirectors: unknown = []): Node {
	return {
		id,
		entityId: id,
		kind: 'C2',
		name: 'Ran',
		entity: { name: 'Ran', listeners, redirectors }
	};
}

describe('c2Badges', () => {
	it('returns one group per C2 node that has listeners', () => {
		const groups = c2Badges([c2Node([listener('tcp', 4444), listener('http', 8080)])]);

		expect(groups).toHaveLength(1);
		expect(groups[0].nodeId).toBe('c2/ran');
		expect(groups[0].listeners.visible.map((badge) => badge.listener.entry)).toEqual([
			'tcp/4444',
			'http/8080'
		]);
		expect(groups[0].listeners.overflowCount).toBe(0);
	});

	it('collapses listeners past the cap into an overflow count', () => {
		const groups = c2Badges([
			c2Node([
				listener('tcp', 1),
				listener('tcp', 2),
				listener('tcp', 3),
				listener('tcp', 4),
				listener('http', 5)
			])
		]);

		expect(groups[0].listeners.visible.map((badge) => badge.listener.port)).toEqual([1, 2, 3]);
		expect(groups[0].listeners.overflowCount).toBe(2);
		expect(groups[0].listeners.overflowTitle).toBe('tcp/4\nhttp/5');
	});

	it('honors a custom cap', () => {
		const groups = c2Badges([c2Node([listener('tcp', 1), listener('tcp', 2)])], 1);

		expect(groups[0].listeners.visible.map((badge) => badge.listener.port)).toEqual([1]);
		expect(groups[0].listeners.overflowCount).toBe(1);
	});

	it('ignores nodes that are not C2 and C2 nodes with neither kind of badge', () => {
		const pod: Node = {
			id: 'pod/one',
			entityId: 'pod/one',
			kind: 'Pod',
			name: 'one',
			entity: { listeners: [listener('tcp', 9999)] }
		};

		expect(c2Badges([pod, c2Node([]), c2Node(undefined, 'c2/other')])).toEqual([]);
	});

	it('tolerates missing graph data', () => {
		expect(c2Badges(undefined)).toEqual([]);
	});

	it('attaches a redirector to the listener it forwards into', () => {
		const groups = c2Badges([
			c2Node([listener('tcp', 4444), listener('http', 8080)], 'c2/ran', [
				redirector('play1', 1337, 4444)
			])
		]);

		const [first, second] = groups[0].listeners.visible;
		expect(first.listener.port).toBe(4444);
		expect(first.adapters.map((a) => a.id)).toEqual(['redirector/play1/1337']);
		// The other listener is untouched — matching is by port, not by position.
		expect(second.adapters).toEqual([]);
		expect(groups[0].orphans.visible).toEqual([]);
	});

	it('attaches several redirectors to the same listener', () => {
		const groups = c2Badges([
			c2Node([listener('tcp', 4444)], 'c2/ran', [
				redirector('play1', 1337, 4444),
				redirector('play2', 1337, 4444)
			])
		]);

		expect(groups[0].listeners.visible[0].adapters.map((a) => a.playId)).toEqual([
			'play1',
			'play2'
		]);
	});

	it('keeps a redirector whose listener is gone as an orphan', () => {
		// Its tunnel is still up, so it has to stay selectable and stoppable.
		const groups = c2Badges([
			c2Node([listener('tcp', 4444)], 'c2/ran', [redirector('play1', 1337, 9999)])
		]);

		expect(groups[0].listeners.visible[0].adapters).toEqual([]);
		expect(groups[0].orphans.visible.map((o) => o.id)).toEqual(['redirector/play1/1337']);
	});

	it('groups a C2 that has only an orphaned redirector', () => {
		const groups = c2Badges([c2Node([], 'c2/ran', [redirector('play1', 1337, 4444)])]);

		expect(groups).toHaveLength(1);
		expect(groups[0].listeners.visible).toEqual([]);
		expect(groups[0].orphans.visible).toHaveLength(1);
	});

	it('names the adapters in a collapsed listener tooltip', () => {
		const groups = c2Badges(
			[
				c2Node([listener('tcp', 1), listener('tcp', 2)], 'c2/ran', [
					redirector('play1', 1337, 2)
				])
			],
			1
		);

		// The collapsed listener still says what forwards into it, and which tool
		// built it.
		expect(groups[0].listeners.overflowTitle).toBe('tcp/2 ← labctl 1337');
	});

	it('names orphans by tool and hop in the overflow tooltip', () => {
		const groups = c2Badges(
			[
				c2Node([], 'c2/ran', [
					redirector('play1', 9000, 4444),
					redirector('play2', 9001, 4444)
				])
			],
			1
		);

		expect(groups[0].orphans.overflowTitle).toBe('labctl 9001→4444');
	});
});
