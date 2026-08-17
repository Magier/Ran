/**
 * Edge hierarchy categories. Informational edges are structural/contextual
 * and should be hidden when a more useful (actionable or factual) edge
 * exists between the same pair of nodes.
 *
 * These names follow the relation semantics exposed by the Rust domain model.
 */

/** Purely structural / contextual edges (high cost in the backend). */
export const INFORMATIONAL_EDGES: ReadonlySet<string> = new Set([
	'runs-on',
	'runs',
	'contains',
	'owns',
	'created',
	'manages-node',
	'uses',
	'has-session',
	'operates',
	'references',
	'authenticates-to'
]);

/** Check whether a relation name is informational. */
export function isInformational(name: string): boolean {
	return INFORMATIONAL_EDGES.has(name);
}
