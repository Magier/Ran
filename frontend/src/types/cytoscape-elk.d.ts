// src/types/cytoscape-elk.d.ts
//
// cytoscape-elk ships no type declarations. These are written from the plugin
// source (node_modules/cytoscape-elk/src/defaults.js for the options,
// src/layout.js for how they are consumed). Hand-written types drift from the
// library, so on a version bump re-read those files before trusting this one.
import cytoscape, { NodeSingular, Position } from 'cytoscape';

declare module 'cytoscape' {
	/**
	 * Layout options for ELK's own algorithms, passed straight through as ELK
	 * `layoutOptions`. Keys are ELK identifiers and must be quoted, in either
	 * the short (`elk.direction`) or fully qualified
	 * (`org.eclipse.elk.hierarchyHandling`) form, and ELK reads every value as
	 * a string. See https://www.eclipse.org/elk/reference.html
	 */
	interface ElkOptions {
		/** The ELK algorithm to run. Effectively mandatory. */
		'elk.algorithm'?:
			| 'box'
			| 'disco'
			| 'force'
			| 'layered'
			| 'mrtree'
			| 'radial'
			| 'random'
			| 'stress';
		[option: string]: string | undefined;
	}

	/** Options for the "elk" layout. */
	interface ElkLayoutOptions extends cytoscape.BaseLayoutOptions {
		name: 'elk';

		/** Include label dimensions when measuring nodes. */
		nodeDimensionsIncludeLabels?: boolean;
		fit?: boolean;
		/** Padding applied when `fit` is true. */
		padding?: number;

		animate?: boolean;
		/** Nodes returning false jump straight to their final position. */
		animateFilter?: (node: NodeSingular, index: number) => boolean;
		animationDuration?: number;
		animationEasing?: string;

		/** Applies a transform to each final node position. */
		transform?: (node: NodeSingular, position: Position) => Position;
		ready?: () => void;
		stop?: () => void;

		/** Per-node ELK options, merged over `elk` for that node. */
		nodeLayoutOptions?: ElkOptions | ((node: NodeSingular) => Record<string, string> | ElkOptions);

		/** Options handed to ELK for the graph as a whole. */
		elk?: ElkOptions;

		/**
		 * Edges with a non-nil priority are skipped when greedy edge cycle
		 * breaking is enabled.
		 */
		priority?: (edge: cytoscape.EdgeSingular) => number | null;
	}
}

declare module 'cytoscape-elk' {
	const register: (cy: typeof cytoscape) => void;
	export default register;
}
