import { describe, expect, it } from 'vitest';
import type { Node } from '$lib/api/index';
import { allListeners, listenersOf } from './listeners';

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

describe('listenersOf', () => {
	it('reads the listeners folded into the node payload', () => {
		const listeners = listenersOf(c2Node([listener('tcp', 4444), listener('http', 8080)]));

		expect(listeners).toEqual([
			{ id: 'listener/tcp/4444', protocol: 'tcp', port: 4444, entry: 'tcp/4444' },
			{ id: 'listener/http/8080', protocol: 'http', port: 8080, entry: 'http/8080' }
		]);
	});

	it('falls back to protocol/port when the payload carries no entry', () => {
		const listeners = listenersOf(
			c2Node([{ id: 'listener/tcp/4444', protocol: 'tcp', port: 4444 }])
		);

		expect(listeners[0].entry).toBe('tcp/4444');
	});

	it('drops entries that could not identify a listener', () => {
		const listeners = listenersOf(
			c2Node([
				listener('tcp', 4444),
				listener('tcp', 4444),
				{ protocol: 'tcp', port: 22 },
				{ id: 'listener/tcp/nope', protocol: 'tcp' },
				'tcp/9999',
				null
			])
		);

		expect(listeners.map((l) => l.id)).toEqual(['listener/tcp/4444']);
	});

	it('tolerates a node with no listeners at all', () => {
		expect(listenersOf(undefined)).toEqual([]);
		expect(listenersOf(c2Node(undefined))).toEqual([]);
		expect(listenersOf({ id: 'c2/x', entityId: 'c2/x', kind: 'C2', name: 'x' })).toEqual([]);
	});
});

describe('allListeners', () => {
	it('collects listeners across every node', () => {
		const nodes = [
			c2Node([listener('tcp', 4444)]),
			c2Node([listener('http', 8080)], 'c2/other'),
			{ id: 'pod/one', entityId: 'pod/one', kind: 'Pod', name: 'one' } as Node
		];

		expect(allListeners(nodes).map((l) => l.id)).toEqual([
			'listener/tcp/4444',
			'listener/http/8080'
		]);
	});

	it('tolerates missing graph data', () => {
		expect(allListeners(undefined)).toEqual([]);
	});
});
