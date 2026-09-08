import type cytoscape from 'cytoscape';

/**
 * Class applied to original edges that we hide when consolidating a collapsed
 * compound node's edges into a single meta-edge. It marks them as "hidden by
 * collapse" so other visibility passes (e.g. hideRedundantInformationalEdges)
 * leave them alone instead of re-showing them — mirroring the role the
 * 'namespace-filtered' class plays for the namespace filter.
 */
export const COLLAPSED_EDGE_CLASS = 'collapsed-consolidated';

/**
 * After a compound node is collapsed, the expand-collapse plugin re-points every
 * child edge at the parent, producing one visible edge per child even when they
 * share the same directed source/target pair. Consolidate those parallel edges
 * into a single meta-edge per directed pair so the collapsed node shows one edge
 * per relation to each external neighbour instead of one edge per hidden child.
 */
export function consolidateCollapsedEdges(cy: cytoscape.Core, node: any) {
	// Consider all VISIBLE connected edges. This includes meta-edges produced by
	// already-collapsed descendants (e.g. collapsed Deployments inside a Namespace),
	// so their groups merge upward into this node's meta-edge. It is also naturally
	// idempotent: after consolidation the underlying edges are hidden, so a repeat
	// call sees only the single visible meta-edge per pair (group size 1 → skipped).
	const connectedEdges = node.connectedEdges().filter((e: any) => e.visible());

	// Group by directed source->target pair
	const edgeGroups = new Map<string, any[]>();

	connectedEdges.forEach((edge: any) => {
		const sourceId = edge.source().id();
		const targetId = edge.target().id();
		if (sourceId === targetId) return; // skip self-loops
		const key = `${sourceId}->${targetId}`;
		if (!edgeGroups.has(key)) {
			edgeGroups.set(key, []);
		}
		edgeGroups.get(key)!.push(edge);
	});

	// Consolidate groups with multiple edges into a single meta-edge
	edgeGroups.forEach((edges, key) => {
		if (edges.length <= 1) return; // single edge, nothing to consolidate

		const separator = '->';
		const sepIndex = key.indexOf(separator);
		const sourceId = key.substring(0, sepIndex);
		const targetId = key.substring(sepIndex + separator.length);
		const metaEdgeId = `meta-${sourceId}-to-${targetId}`;

		// Remove a prior meta-edge for this pair if it exists
		const existing = cy.getElementById(metaEdgeId);
		if (existing.length > 0) existing.remove();

		// Hide all edges in this group. Tag them so later visibility passes
		// (hideRedundantInformationalEdges) know they were hidden by collapse
		// and must not re-show them.
		edges.forEach((e: any) => {
			e.addClass(COLLAPSED_EDGE_CLASS);
			e.hide();
		});

		// Build a descriptive label from unique edge names
		const uniqueNames = [...new Set(edges.map((e: any) => e.data('name')))].filter(Boolean);
		const label = uniqueNames.length === 1 ? uniqueNames[0] : `${edges.length} relations`;

		// Inherit the informational styling (subdued gray/dotted) only when every
		// consolidated edge was informational. If any underlying edge is actionable,
		// the group represents a meaningful relation and should look actionable.
		const informational = edges.every((e: any) => Boolean(e.data('informational')));

		cy.add({
			group: 'edges',
			data: {
				id: metaEdgeId,
				source: sourceId,
				target: targetId,
				name: label,
				informational,
				collapsedEdges: edges.map((e: any) => e.id()),
				isMetaEdge: true
			}
		});
	});
}

/**
 * Reverse of consolidateCollapsedEdges for a node about to be (or just) expanded:
 * remove our custom meta-edges touching this node and restore the original edges
 * we hid, clearing the collapse marker so normal visibility logic applies again.
 */
export function restoreConsolidatedEdges(cy: cytoscape.Core, node: any) {
	cy.edges('[?isMetaEdge]').forEach((metaEdge: any) => {
		const source = metaEdge.source().id();
		const target = metaEdge.target().id();

		if (source === node.id() || target === node.id()) {
			const collapsedEdgeIds: string[] = metaEdge.data('collapsedEdges') || [];

			// Show back the edges we hid and clear the collapse marker
			collapsedEdgeIds.forEach((edgeId: string) => {
				const edge = cy.getElementById(edgeId);
				if (edge.length > 0) {
					(edge as any).removeClass(COLLAPSED_EDGE_CLASS);
					(edge as any).show();
				}
			});

			metaEdge.remove();
		}
	});
}

/**
 * Idempotent safety net: consolidate edges for EVERY currently-collapsed compound
 * node in the graph. On fresh page load the compounds are restored as collapsed
 * without necessarily firing a clean `aftercollapse` for each (the mount effect
 * expands-then-recollapses, and events can be swallowed or skipped), so relying
 * on the event alone leaves child edges un-consolidated. Call this after the
 * mount/update reconciliation to guarantee every collapsed node shows one meta-edge
 * per external neighbour. Safe to call repeatedly.
 */
export function reconcileCollapsedEdges(cy: cytoscape.Core) {
	cy.nodes('.cy-expand-collapse-collapsed-node').forEach((node: any) => {
		consolidateCollapsedEdges(cy, node);
	});
}

/**
 * Hide informational edges between a node pair when a non-informational
 * (actionable/factual) edge already exists for that same pair in the same direction.
 * Additionally, always hide "runs-on" edges when ANY other edge (informational
 * or not) exists for that pair, since runs-on is purely structural noise.
 * Skips edges hidden by the namespace filter or by compound-node collapse, so
 * this pass never re-shows an edge another feature deliberately hid.
 */
export function hideRedundantInformationalEdges(cy: cytoscape.Core) {
	// Collect directed node-pairs that have at least one non-informational, non-filtered edge
	const hasActionableEdge = new Set<string>();
	// Collect directed node-pairs that have any non-filtered edge (keyed by pair + edge name)
	const pairEdgeNames = new Map<string, Set<string>>();

	cy.edges().forEach((e: any) => {
		if (e.hasClass('namespace-filtered') || e.hasClass(COLLAPSED_EDGE_CLASS)) return;
		const pair = `${e.source().id()}->${e.target().id()}`;
		if (!e.data('informational')) {
			hasActionableEdge.add(pair);
		}
		// Track all edge names per directed pair
		if (!pairEdgeNames.has(pair)) pairEdgeNames.set(pair, new Set());
		pairEdgeNames.get(pair)!.add(e.data('name'));
	});

	// Hide informational edges whose directed pair has an actionable edge.
	// For "runs-on", hide when ANY other edge exists for the same pair.
	cy.edges('[?informational]').forEach((e: any) => {
		if (e.hasClass('namespace-filtered')) return; // don't touch namespace-filtered edges
		// Edges hidden by a compound collapse are represented by a meta-edge; leave
		// them hidden so collapsing doesn't get undone by this pass.
		if (e.hasClass(COLLAPSED_EDGE_CLASS)) return;
		const pair = `${e.source().id()}->${e.target().id()}`;
		const name = e.data('name');

		if (name === 'runs-on') {
			// Hide runs-on if any other relation exists for this pair
			const names = pairEdgeNames.get(pair);
			if (names && names.size > 1) {
				e.hide();
			} else {
				e.show();
			}
		} else if (hasActionableEdge.has(pair)) {
			e.hide();
		} else {
			e.show();
		}
	});
}
