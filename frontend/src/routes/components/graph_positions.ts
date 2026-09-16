import type cytoscape from 'cytoscape';
import type { PosMap } from './graph_nodes';

/**
 * Record the node the user explicitly dragged. Call this from `dragfreeon`,
 * which fires only for the grabbed node rather than every carried descendant.
 */
export function captureManualPosition(node: cytoscape.NodeSingular, manualPositions: PosMap): void {
	const { x, y } = node.position();
	// Cytoscape reuses and mutates its position object during layouts. Store a
	// value snapshot so ELK cannot mutate the manual coordinate through a shared
	// reference before it is restored.
	manualPositions[node.id()] = { x, y };
}

/**
 * Expand manually positioned compounds into leaf constraints for ELK.
 * Cytoscape lays out only leaf nodes, so fixing their current coordinates keeps
 * the compound stable during the layout calculation. The compound itself is
 * restored afterward to account for any newly discovered children.
 */
export function fixedPositionsForLayout(cy: cytoscape.Core, manualPositions: PosMap): PosMap {
	const fixedPositions = { ...manualPositions };
	Object.keys(manualPositions).forEach((id) => {
		const node = cy.getElementById(id);
		if (node.empty() || !node.isParent()) return;
		node.descendants().forEach((descendant) => {
			if (!descendant.isParent()) {
				const { x, y } = descendant.position();
				fixedPositions[descendant.id()] = { x, y };
			}
		});
	});
	return fixedPositions;
}

/** Reapply hard user choices after an automatic layout has positioned the graph. */
export function restoreManualPositions(cy: cytoscape.Core, manualPositions: PosMap): void {
	const nodesToRestore = Object.entries(manualPositions)
		.map(([id, position]) => ({ node: cy.getElementById(id), position }))
		.filter(({ node }) => node.nonempty())
		// Restore children first and compounds last, so an explicitly moved
		// compound remains the final anchor even when it gained new children.
		.sort(({ node: a }, { node: b }) => b.ancestors().length - a.ancestors().length);

	cy.batch(() => {
		nodesToRestore.forEach(({ node, position }) => node.position(position));
	});
}
