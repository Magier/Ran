// src/types/cytoscape-fcose.d.ts
import cytoscape, { LayoutOptions, EdgeSingular, NodeSingular } from "cytoscape";

declare module "cytoscape" {
  /** Options for the "fcose" layout */
  interface FCoSELayoutOptions extends LayoutOptions {
    name: "fcose";

    // General
    quality?: "draft" | "default" | "proof";
    randomize?: boolean;
    animate?: boolean | "end";
    animationDuration?: number;
    animationEasing?: string;
    fit?: boolean;
    padding?: number;
    nodeDimensionsIncludeLabels?: boolean;
    uniformNodeDimensions?: boolean;
    packComponents?: boolean;

    // Forces / physics (common in CoSE/ fCoSE)
    nodeSeparation?: number;
    idealEdgeLength?: number | ((edge: EdgeSingular) => number);
    nodeRepulsion?: number | ((node: NodeSingular) => number);
    edgeElasticity?: number | ((edge: EdgeSingular) => number);
    nestingFactor?: number;
    gravity?: number;
    gravityRangeCompound?: number;
    gravityCompound?: number;
    gravityRange?: number;

    // Constraints & incremental
    numIter?: number;
    tile?: boolean;
    initialEnergyOnIncremental?: number;

    fixedNodeConstraint?: Array<{
      nodeId: string;
      position: { x: number; y: number };
    }>;

    alignmentConstraint?: {
      vertical?: string[][];
      horizontal?: string[][];
    };

    relativePlacementConstraint?: Array<{
      top?: string;
      bottom?: string;
      left?: string;
      right?: string;
      gap?: number;
    }>;

    /** Allow any extra plugin-specific options you may use */
    [key: string]: unknown;
  }
}

declare module "cytoscape-fcose" {
  const register: (cy: typeof cytoscape) => void;
  export default register;
}
