import type { Node } from '$lib/api/index';

/**
 * A C2 listener, as carried on the C2 graph node's `listeners` payload.
 *
 * Listeners are real entities (`kind: Listener`) that the graph folds into their
 * host's payload rather than drawing as nodes — the same treatment AppServices
 * get. `id` is the entity id, so clicking a badge can select the listener and
 * scope the armory to the actions that target it.
 */
export type Listener = {
	/** Entity id, e.g. `listener/tcp/4444`. */
	id: string;
	/** Lowercased protocol, e.g. `tcp`. */
	protocol: string;
	port: number;
	/** Canonical `protocol/port`, as shown on the badge. */
	entry: string;
};

function parseListener(raw: unknown): Listener | null {
	if (typeof raw !== 'object' || raw === null) return null;
	const listener = raw as Record<string, unknown>;

	const id = typeof listener.id === 'string' ? listener.id : '';
	const port = typeof listener.port === 'number' ? listener.port : NaN;
	const protocol = typeof listener.protocol === 'string' ? listener.protocol : '';
	if (id === '' || !Number.isFinite(port)) return null;

	const entry = typeof listener.entry === 'string' ? listener.entry : `${protocol}/${port}`;
	return { id, protocol, port, entry };
}

/** The listeners held by a single graph node, in payload order (by port). */
export function listenersOf(node: Node | undefined): Listener[] {
	const raw = (node?.entity as Record<string, unknown> | undefined)?.listeners;
	if (!Array.isArray(raw)) return [];

	const seen = new Set<string>();
	const listeners: Listener[] = [];
	for (const value of raw) {
		const listener = parseListener(value);
		if (!listener || seen.has(listener.id)) continue;
		seen.add(listener.id);
		listeners.push(listener);
	}
	return listeners;
}

/** Every listener in the graph, across all C2s. */
export function allListeners(nodes: Node[] | undefined): Listener[] {
	return (nodes ?? []).flatMap((node) => listenersOf(node));
}
