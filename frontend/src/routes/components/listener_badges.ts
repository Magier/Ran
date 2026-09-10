import type { Node } from '$lib/api/index';
import { listenersOf, type Listener } from '$lib/listeners';

/** Number of port chips rendered before the rest collapse into a `+k` chip. */
export const MAX_VISIBLE_BADGES = 3;

export type ListenerBadgeGroup = {
	/** Graph node the chips are anchored to. */
	nodeId: string;
	visible: Listener[];
	/** How many listeners the `+k` chip stands for; 0 when all fit. */
	overflowCount: number;
	/** Tooltip for the `+k` chip: the collapsed entries, one per line. */
	overflowTitle: string;
};

/**
 * Collect the listener chips to draw for every C2 node in the graph.
 *
 * Only `kind === 'C2'` nodes carry listeners today; nodes without any are
 * omitted entirely so the overlay renders nothing for them.
 */
export function c2ListenerBadges(
	nodes: Node[] | undefined,
	maxVisible: number = MAX_VISIBLE_BADGES
): ListenerBadgeGroup[] {
	if (!nodes) return [];

	const groups: ListenerBadgeGroup[] = [];
	for (const node of nodes) {
		if (node.kind !== 'C2') continue;

		const badges = listenersOf(node);
		if (badges.length === 0) continue;

		const visible = badges.slice(0, Math.max(maxVisible, 0));
		const overflow = badges.slice(visible.length);
		groups.push({
			nodeId: node.id,
			visible,
			overflowCount: overflow.length,
			overflowTitle: overflow.map((badge) => badge.entry).join('\n')
		});
	}

	return groups;
}
