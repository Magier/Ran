// src/types/cytoscape-expand-collapse.d.ts
//
// cytoscape-expand-collapse ships no type declarations. These are written from
// the plugin source (node_modules/cytoscape-expand-collapse/src/index.js): the
// options block is its documented defaults object, and the methods are the
// `api.*` assignments in createExtensionAPI. Hand-written types drift from the
// library, so on a version bump re-read that file before trusting this one.
import cytoscape, { NodeCollection, NodeSingular, EdgeCollection } from 'cytoscape';

declare module 'cytoscape' {
	/**
	 * Options accepted by `cy.expandCollapse()`. Every field is optional; the
	 * plugin merges what it is given over its own defaults.
	 */
	interface ExpandCollapseOptions {
		/** Layout to re-run after an expand or collapse. `null` disables it. */
		layoutBy?: cytoscape.LayoutOptions | (() => void) | null;
		/** Fisheye view around the node being expanded or collapsed. */
		fisheye?: boolean | (() => boolean);
		animate?: boolean | (() => boolean);
		animationDuration?: number;
		/** Called once the extension has initialised. */
		ready?: () => void;
		/** Register operations with the undo-redo extension, when present. */
		undoable?: boolean;

		/** The clickable plus/minus cue drawn on collapsible nodes. */
		cueEnabled?: boolean;
		expandCollapseCuePosition?:
			| 'top-left'
			| 'top-right'
			| 'bottom-left'
			| 'bottom-right'
			| ((node: NodeSingular) => { x: number; y: number });
		expandCollapseCueSize?: number;
		expandCollapseCueLineSize?: number;
		/** Custom cue artwork. Undefined draws the built-in icon. */
		expandCueImage?: string;
		collapseCueImage?: string;
		expandCollapseCueSensitivity?: number;

		/** `edge.data()` field naming the edge type, used to group edges. */
		edgeTypeInfo?: string | ((edge: cytoscape.EdgeSingular) => string);
		groupEdgesOfSameTypeOnCollapse?: boolean;
		allowNestedEdgeCollapse?: boolean;
		/** z-index of the canvas the cues are drawn on. */
		zIndex?: number;
	}

	/** Per-call overrides. The plugin accepts the same shape as the initialiser. */
	type ExpandCollapseCallOptions = ExpandCollapseOptions;

	/** The object returned by `cy.expandCollapse(options)`. */
	interface ExpandCollapseApi {
		setOptions(options: ExpandCollapseOptions): void;
		extendOptions(options: ExpandCollapseOptions): void;
		setOption<K extends keyof ExpandCollapseOptions>(
			name: K,
			value: ExpandCollapseOptions[K]
		): void;

		collapse(eles: NodeCollection | NodeSingular, options?: ExpandCollapseCallOptions): void;
		collapseRecursively(
			eles: NodeCollection | NodeSingular,
			options?: ExpandCollapseCallOptions
		): void;
		expand(eles: NodeCollection | NodeSingular, options?: ExpandCollapseCallOptions): void;
		expandRecursively(
			eles: NodeCollection | NodeSingular,
			options?: ExpandCollapseCallOptions
		): void;
		collapseAll(options?: ExpandCollapseCallOptions): void;
		expandAll(options?: ExpandCollapseCallOptions): void;

		isExpandable(node: NodeSingular): boolean;
		isCollapsible(node: NodeSingular): boolean;
		collapsibleNodes(nodes?: NodeCollection): NodeCollection;
		expandableNodes(nodes?: NodeCollection): NodeCollection;

		getCollapsedChildren(node: NodeSingular): cytoscape.Collection;
		getCollapsedChildrenRecursively(node: NodeSingular): cytoscape.Collection;
		getAllCollapsedChildrenRecursively(): cytoscape.Collection;
		getParent(nodeId: string): NodeSingular | undefined;

		clearVisualCue(node: NodeSingular): void;
		disableCue(): void;
		enableCue(): void;

		collapseEdges(edges: EdgeCollection, options?: ExpandCollapseCallOptions): void;
		expandEdges(edges: EdgeCollection): void;
		collapseEdgesBetweenNodes(nodes: NodeCollection, options?: ExpandCollapseCallOptions): void;
		expandEdgesBetweenNodes(nodes: NodeCollection): void;
		collapseAllEdges(options?: ExpandCollapseCallOptions): void;
		expandAllEdges(): void;

		loadJson(json: string): void;
		saveJson(eles: cytoscape.Collection, filename: string): void;
	}

	interface Core {
		/**
		 * Initialise the extension, or pass the literal string `'get'` to
		 * retrieve the API of an already-initialised instance.
		 */
		expandCollapse(options: ExpandCollapseOptions | 'get'): ExpandCollapseApi;
	}
}

declare module 'cytoscape-expand-collapse' {
	const register: (cy: typeof cytoscape) => void;
	export default register;
}
