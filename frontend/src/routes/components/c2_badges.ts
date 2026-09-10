import type { Node } from '$lib/api/index';
import { listenersOf, type Listener } from '$lib/listeners';
import { redirectorsOf, type Redirector } from '$lib/redirectors';

/** Number of chips of one kind rendered before the rest collapse into a `+k` chip. */
export const MAX_VISIBLE_BADGES = 3;

/** One capped run of chips: what fits, plus what the `+k` chip stands for. */
export type BadgeStack<T> = {
	visible: T[];
	/** How many entries the `+k` chip stands for; 0 when all fit. */
	overflowCount: number;
	/** Tooltip for the `+k` chip: the collapsed entries, one per line. */
	overflowTitle: string;
};

/**
 * A listener and the redirectors that adapt it.
 *
 * They are one badge rather than two because that is what they are: a redirector
 * is not a peer of the listener, it is a remote entry point onto it. Rendering
 * them as a single chip says so; two stacked chips left it to the operator to
 * infer which redirector belonged to which port.
 */
export type ListenerBadge = {
	listener: Listener;
	adapters: Redirector[];
};

export type C2BadgeGroup = {
	/** Graph node the chips are anchored to. */
	nodeId: string;
	listeners: BadgeStack<ListenerBadge>;
	/**
	 * Redirectors whose listener is no longer in the payload - the listener was
	 * stopped while the tunnel stayed up. They get their own chip so they remain
	 * selectable, and so "Stop Redirector" stays reachable.
	 */
	orphans: BadgeStack<Redirector>;
};

function stack<T>(entries: T[], maxVisible: number, title: (entry: T) => string): BadgeStack<T> {
	const visible = entries.slice(0, Math.max(maxVisible, 0));
	const overflow = entries.slice(visible.length);
	return {
		visible,
		overflowCount: overflow.length,
		overflowTitle: overflow.map(title).join('\n')
	};
}

/** Label for a listener chip including its adapters, used in overflow tooltips. */
function listenerBadgeTitle(badge: ListenerBadge): string {
	if (badge.adapters.length === 0) return badge.listener.entry;
	// Name the tool, not the playground: a collapsed entry still has to say what
	// kind of redirector is feeding the listener.
	const hops = badge.adapters
		.map((adapter) => [adapter.via, adapter.remotePort].filter(Boolean).join(' '))
		.join(', ');
	return `${badge.listener.entry} ← ${hops}`;
}

/**
 * Collect the chips to draw for every C2 node in the graph.
 *
 * Redirectors are matched to their listener by port: a port is bound once on the
 * operator host, so the port alone identifies which listener a tunnel lands on.
 * Only `kind === 'C2'` nodes carry either today; nodes with neither are omitted
 * entirely so the overlay renders nothing for them.
 */
export function c2Badges(
	nodes: Node[] | undefined,
	maxVisible: number = MAX_VISIBLE_BADGES
): C2BadgeGroup[] {
	if (!nodes) return [];

	const groups: C2BadgeGroup[] = [];
	for (const node of nodes) {
		if (node.kind !== 'C2') continue;

		const listeners = listenersOf(node);
		const redirectors = redirectorsOf(node);
		if (listeners.length === 0 && redirectors.length === 0) continue;

		const ports = new Set(listeners.map((listener) => listener.port));
		const badges: ListenerBadge[] = listeners.map((listener) => ({
			listener,
			adapters: redirectors.filter((r) => r.listenerPort === listener.port)
		}));
		const orphans = redirectors.filter((r) => !ports.has(r.listenerPort));

		groups.push({
			nodeId: node.id,
			listeners: stack(badges, maxVisible, listenerBadgeTitle),
			orphans: stack(orphans, maxVisible, (orphan) => orphan.label)
		});
	}

	return groups;
}
