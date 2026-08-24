<script lang="ts">
    import { type FlowEdge, type AttackFlow, type AttackStep } from '$lib/api/index';
    import AttackStepDrawer from '$lib/components/AttackStepDrawer.svelte';
    import ActionNode from '$lib/components/flow/attack_node.svelte';
    import {
        SvelteFlow,
        Position,
        Controls,
        Background,
        BackgroundVariant,
        type Node,
        type Edge,
        type NodeTypes
    } from '@xyflow/svelte';
    import dagre from '@dagrejs/dagre';
    import '@xyflow/svelte/dist/style.css';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
	import { ranAPI } from '$lib/ran_api';
    import { getContext } from 'svelte';

    let campaignState = getCampaignState();
    const theme = getContext<{ isDark: boolean }>('theme');
    const dagreGraph = new dagre.graphlib.Graph();
    dagreGraph.setDefaultEdgeLabel(() => ({}));

    let nodes: Node[] = $state.raw([]);
    let edges: Edge[] = $state.raw([]);

    const nodeTypes: NodeTypes = {
        actionNode: ActionNode
    };

    // const snapGrid = [25, 25];
    const nodeWidth = 240; // values are manually read from rendered nodes
    const nodeHeight = 95;
    let selectedStep: AttackStep | null = $state(null);

    function convertStep(step: AttackStep): Node {
        return {
            id: step.id,
            type: 'actionNode',
            data: { label: step.TTP.name, step: step },
            position: { x: 0, y: 0 } // will be replaced by the layout algorithm
        };
    }

    function convertEdge(edge: FlowEdge): Edge {
        return {
            id: edge.id,
            type: 'default',
            source: edge.sourceId,
            target: edge.targetId,
        };
    }

    function layOutElements(nodes: Node[], edges: Edge[], direction = 'TB') {
        const isHorizontal = direction === 'LR';
        dagreGraph.setGraph({ rankdir: direction });

        nodes.forEach((node) => {
            dagreGraph.setNode(node.id, {
                ...node,
                width: node.measured?.width ?? nodeWidth,
                height: node.measured?.height ?? nodeHeight,
            });
        });

        edges.forEach((edge) => {
            dagreGraph.setEdge(edge.source, edge.target);
        });

        dagre.layout(dagreGraph);

        nodes = nodes.map((node) => {
            // set the position of the input/output ports
            const nodeWithPosition = dagreGraph.node(node.id);
            // We are shifting the dagre node position (anchor=center center) to the top left
            // so it matches the React Flow node anchor point (top left).
            const x = nodeWithPosition.x - (node.measured?.width ?? nodeWidth) / 2;
            const y = nodeWithPosition.y - (node.measured?.height ?? nodeHeight) / 2;
            return {
                ...node,
                position: { x, y },
                sourcePosition: isHorizontal ? Position.Right : Position.Bottom,
                targetPosition: isHorizontal ? Position.Left : Position.Top
            }
        });
        return { nodes, edges };
    }

    ranAPI.GetFlow()
        .then((result: AttackFlow) => {
            console.log('Campaign flow:', result);
            const { steps, edges: es } = result;
            const laidOutElements = layOutElements(steps.map(convertStep), es.map(convertEdge), 'TB');

            nodes = laidOutElements.nodes;
            edges = laidOutElements.edges;
        })
        .catch((err) => {
            console.error(err);
        });

</script>

<div class="items-top mx-auto flex h-dvh w-full justify-center">
    <!-- {#each nodes as node}
        <div class="border-primary-950-50 m-4 rounded-lg border-2 p-6 shadow-lg">
            {node.name}
        </div>
    {/each} -->
    <!-- <Graph bind:selectedNodeId /> -->

    <SvelteFlow
        bind:nodes
        bind:edges
        {nodeTypes}
        fitView
        colorMode={theme.isDark ? 'dark' : 'light'}
        onnodeclick={(event) => {
            console.log('on node click', event, event.node);
            selectedStep = event.node.data.step as AttackStep;
        }}
        onpaneclick={(event) => {
            console.log('on pane click', event);
            selectedStep = null;
        }}
        onselectionclick={(event) => console.log('on selection click', event)}
        minZoom={0.1}
        maxZoom={2.5}
    >
        <Controls />
        <Background variant={BackgroundVariant.Dots} />
        <!-- <MiniMap /> -->
    </SvelteFlow>
</div>

<AttackStepDrawer step={selectedStep} onclose={() => (selectedStep = null)} />

<style>
    :global(.svelte-flow__node) {
        text-align: center;
    }
</style>
