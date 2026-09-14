import type cytoscape from 'cytoscape';
import type { Node } from '$lib/api/index';
import { hasKnowledgeProvenance } from '$lib/knowledgeProvenance';

export type Pos = { x: number; y: number };
export type PosMap = Record<string, Pos>;

/** A graph node as cytoscape holds it: the API node plus the flags our styles key off. */
export type CyNode = {
	id: string;
	label: string;
	data: Node & { scenarioProvided: boolean };
	position?: Pos;
};

/**
 * Build the cytoscape element data for one graph node.
 *
 * The `{ ...n }` spread carries every field the backend sent, `parent`
 * included, so no field is written behind a guard. That matters because
 * cytoscape's `data(obj)` merges rather than replaces and the backend drops
 * optional fields once they go falsy (`skip_serializing_if = "Option::is_none"`),
 * so a guarded assignment can only ever latch a field on - the shape that
 * pinned session edges to the broken style in #80.
 *
 * Two fields are deliberately NOT given a spelled-out falsy default:
 *
 * - `compromised` / `isRunning`: the backend omits them only for kinds that
 *   never carry them and a node never changes kind, so neither can latch. The
 *   entity panel reads a missing key as "not applicable", so a spelled-out
 *   `false` would show `isRunning: false` on every Service.
 * - `parent`: it cannot latch through the merge, because cytoscape ignores a
 *   `parent` key handed to `data()` outright. Compound membership is structure,
 *   not data, so reparenting needs `syncNodeParent` instead.
 */
export function toCyNode(n: Node, nodePos: PosMap): CyNode {
	const cyNode: CyNode = {
		id: n.id,
		label: n.name,
		data: { ...n, scenarioProvided: hasKnowledgeProvenance(n.provenance, 'scenario') }
	};

	if (Object.hasOwn(nodePos, n.id)) {
		cyNode.position = nodePos[n.id];
	} else if (n.name === 'Ran' || n.id === 'c2/Ran') {
		// Seed the operator node at a fixed spot so the first layout has an anchor.
		cyNode.position = { x: -100, y: 0 };
		nodePos[n.id] = cyNode.position;
	}

	return cyNode;
}

/**
 * Reconcile the compound membership of a node already in the graph.
 *
 * Cytoscape treats a node's parent as structure rather than data: it silently
 * drops a `parent` key passed to `data()`, leaving both the membership and the
 * value in `data` untouched. So the refresh in `graph.svelte`, which updates
 * existing nodes with `data(n.data)`, can never reparent anything - a node that
 * moved between compounds keeps rendering under the old one, and a node that
 * lost its parent stays inside a compound it no longer belongs to.
 *
 * `move()` is the only way to reparent, and it takes `null` to detach. It
 * rebuilds the element, so call it only when the parent actually changed;
 * classes, position and connected edges survive.
 *
 * Returns true when the node was moved.
 */
export function syncNodeParent(cy: cytoscape.Core, node: CyNode): boolean {
	const el = cy.getElementById(node.data.id);
	if (el.length === 0) return false;

	const current = el.isChild() ? el.parent().first().id() : null;
	const next = node.data.parent ?? null;
	if (current === next) return false;

	// Moving into a parent cytoscape does not hold yet would dangle the node.
	// Leave it put; the next refresh retries once the parent has been added.
	if (next !== null && cy.getElementById(next).length === 0) {
		tripwire(`blocked, parent not in graph: ${node.data.id} ${current} -> ${next}`);
		return false;
	}

	tripwire(`reparent: ${node.data.id} ${current} -> ${next}`);
	el.move({ parent: next });
	return true;
}

/**
 * Temporary tripwire for #84. Nothing in the backend changes a node's parent
 * today, so this helper should never reach a move: any output means the parent
 * cytoscape holds disagrees with the one the backend sent, which is a bug in
 * the comparison above rather than a real reparenting.
 *
 * The blocked case is worth hearing too. It only retries on the next refresh,
 * so a parent that never arrives leaves the node under the wrong compound
 * silently, which looks exactly like the bug this fixes.
 *
 * Remove once something actually reparents nodes and the noise stops being a
 * signal. Muted under test, where the specs drive reparenting on purpose.
 */
function tripwire(message: string) {
	if (import.meta.env.DEV && !import.meta.env.TEST) {
		console.warn(`[#84 tripwire] ${message}`);
	}
}
