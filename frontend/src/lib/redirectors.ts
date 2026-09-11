import type { Node } from '$lib/api/index';

/**
 * A redirector, as carried on the C2 graph node's `redirectors` payload.
 *
 * Redirectors are real entities (`kind: Redirector`) - one `labctl port-forward`
 * process each - that the graph folds into the C2's payload rather than drawing
 * as nodes, the same treatment listeners get. `id` is the entity id, so clicking
 * a badge can select the redirector and scope the armory to "Stop Redirector".
 */
export type Redirector = {
	/** Entity id, e.g. `redirector/zn1kqxk3ykpvxp5x/1337`. */
	id: string;
	/** The tool that built the tunnel, e.g. `labctl`. */
	via: string;
	/** iximiuz playground id the tunnel is attached to. */
	playId: string;
	/** Port opened on the playground. */
	remotePort: number;
	/** Port of the local listener traffic lands on. */
	listenerPort: number;
	/** Canonical `playId/remotePort` - the identity, not the display name. */
	entry: string;
	/**
	 * What the operator reads, e.g. `labctl 9000→4444`.
	 *
	 * The tool comes first because that is the part that says what kind of
	 * redirector this is; the playground id is a random string and is
	 * deliberately not here.
	 */
	label: string;
};

/** The hop a redirector makes, in traffic direction, without the tool. */
export function redirectorHop(redirector: Redirector): string {
	return `${redirector.remotePort}→${redirector.listenerPort}`;
}

function parseRedirector(raw: unknown): Redirector | null {
	if (typeof raw !== 'object' || raw === null) return null;
	const redirector = raw as Record<string, unknown>;

	const id = typeof redirector.id === 'string' ? redirector.id : '';
	const via = typeof redirector.via === 'string' ? redirector.via : '';
	const playId = typeof redirector.playId === 'string' ? redirector.playId : '';
	const remotePort = typeof redirector.remotePort === 'number' ? redirector.remotePort : NaN;
	const listenerPort = typeof redirector.listenerPort === 'number' ? redirector.listenerPort : NaN;
	if (id === '' || !Number.isFinite(remotePort) || !Number.isFinite(listenerPort)) return null;

	const entry = typeof redirector.entry === 'string' ? redirector.entry : `${playId}/${remotePort}`;
	const hop = `${remotePort}→${listenerPort}`;
	const label =
		typeof redirector.label === 'string' && redirector.label !== ''
			? redirector.label
			: [via, hop].filter(Boolean).join(' ');
	return { id, via, playId, remotePort, listenerPort, entry, label };
}

/** The redirectors held by a single graph node, in payload order (by remote port). */
export function redirectorsOf(node: Node | undefined): Redirector[] {
	const raw = (node?.entity as Record<string, unknown> | undefined)?.redirectors;
	if (!Array.isArray(raw)) return [];

	const seen = new Set<string>();
	const redirectors: Redirector[] = [];
	for (const value of raw) {
		const redirector = parseRedirector(value);
		if (!redirector || seen.has(redirector.id)) continue;
		seen.add(redirector.id);
		redirectors.push(redirector);
	}
	return redirectors;
}

/** Every redirector in the graph, across all C2s. */
export function allRedirectors(nodes: Node[] | undefined): Redirector[] {
	return (nodes ?? []).flatMap((node) => redirectorsOf(node));
}

/**
 * Pickable options for a `Redirector` TTP parameter.
 *
 * Labels are the plain `labctl 9000→4444` form, except where two redirectors
 * would read identically - two playgrounds forwarding the same ports - in which
 * case the playground id is appended to those. It is noise everywhere else, so
 * it only appears where it is the thing that tells them apart.
 */
export function redirectorOptions(nodes: Node[] | undefined): { label: string; value: string }[] {
	const redirectors = allRedirectors(nodes);
	const seen = new Map<string, number>();
	for (const redirector of redirectors) {
		seen.set(redirector.label, (seen.get(redirector.label) ?? 0) + 1);
	}
	return redirectors.map((redirector) => ({
		label:
			(seen.get(redirector.label) ?? 0) > 1
				? `${redirector.label} (${redirector.playId})`
				: redirector.label,
		value: redirector.id
	}));
}
