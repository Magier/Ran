import { describe, expect, it } from 'vitest';
import type { Node } from '$lib/api/index';
import { c2ListenerBadges } from './listener_badges';

function listener(protocol: string, port: number) {
	return {
		id: `listener/${protocol}/${port}`,
		kind: 'Listener',
		entry: `${protocol}/${port}`,
		protocol,
		port
	};
}

function c2Node(listeners: unknown, id = 'c2/ran'): Node {
	return { id, entityId: id, kind: 'C2', name: 'Ran', entity: { name: 'Ran', listeners } };
}

describe('c2ListenerBadges', () => {
	it('returns one group per C2 node that has listeners', () => {
		const groups = c2ListenerBadges([c2Node([listener('tcp', 4444), listener('http', 8080)])]);

		expect(groups).toHaveLength(1);
		expect(groups[0].nodeId).toBe('c2/ran');
		expect(groups[0].visible.map((badge) => badge.entry)).toEqual(['tcp/4444', 'http/8080']);
		expect(groups[0].overflowCount).toBe(0);
	});

	it('collapses listeners past the cap into an overflow count', () => {
		const groups = c2ListenerBadges([
			c2Node([
				listener('tcp', 1),
				listener('tcp', 2),
				listener('tcp', 3),
				listener('tcp', 4),
				listener('http', 5)
			])
		]);

		expect(groups[0].visible.map((badge) => badge.port)).toEqual([1, 2, 3]);
		expect(groups[0].overflowCount).toBe(2);
		expect(groups[0].overflowTitle).toBe('tcp/4\nhttp/5');
	});

	it('honors a custom cap', () => {
		const groups = c2ListenerBadges([c2Node([listener('tcp', 1), listener('tcp', 2)])], 1);

		expect(groups[0].visible.map((badge) => badge.port)).toEqual([1]);
		expect(groups[0].overflowCount).toBe(1);
	});

	it('ignores nodes that are not C2 and C2 nodes without listeners', () => {
		const pod: Node = {
			id: 'pod/one',
			entityId: 'pod/one',
			kind: 'Pod',
			name: 'one',
			entity: { listeners: [listener('tcp', 9999)] }
		};

		expect(c2ListenerBadges([pod, c2Node([]), c2Node(undefined, 'c2/other')])).toEqual([]);
	});

	it('tolerates missing graph data', () => {
		expect(c2ListenerBadges(undefined)).toEqual([]);
	});
});
