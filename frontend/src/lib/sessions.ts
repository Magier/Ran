import type { Edge } from '$lib/api/index';

/** A live reverse-shell session carried by a `c2.session` graph edge. */
export type SessionOption = {
	label: string;
	value: string;
};

/**
 * List live sessions owned by one C2. Broken edges stay out of the picker:
 * there is no backend left to close, even though the historical edge remains
 * visible for recovery context.
 */
export function sessionOptions(edges: Edge[] | undefined, c2Id: string): SessionOption[] {
	return (edges ?? [])
		.filter(
			(edge) =>
				edge.name === 'c2.session' &&
				edge.sourceId === c2Id &&
				!edge.broken &&
				typeof edge.sessionId === 'string' &&
				edge.sessionId !== ''
		)
		.map((edge) => ({
			label: `${edge.targetId} (${edge.sessionId})`,
			value: edge.sessionId!
		}))
		.sort((a, b) => a.label.localeCompare(b.label) || a.value.localeCompare(b.value));
}
