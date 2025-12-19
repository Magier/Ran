<script lang="ts">
    import { GetFlow } from '$lib/wailsjs/go/main/App';
    import { campaign, main } from '$lib/domain/models';
    import AttackStepDetails from '$lib/components/attack_step_details.svelte';
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
    import { Modal } from '@skeletonlabs/skeleton-svelte';

    const dagreGraph = new dagre.graphlib.Graph();
    dagreGraph.setDefaultEdgeLabel(() => ({}));

    let nodes = $state.raw([]);
    let edges = $state.raw([]);

    const nodeTypes: NodeTypes = {
        actionNode: ActionNode
    };

    // const snapGrid = [25, 25];
    const nodeWidth = 240; // values are manually read from rendered nodes
    const nodeHeight = 95;
    let selectedStep: campaign.AttackStep | null = $state(null);

    function convertStep(step: campaign.AttackStep): Node {
        return {
            id: step.ID,
            type: 'actionNode',
            data: { label: step.TTP.name, step: step },
            position: { x: 0, y: 0 } // will be replaced by the layout algorithm
        };
    }

    function convertEdge(edge: main.Edge): Edge {
        return {
            id: edge.id,
            type: 'default',
            source: edge.sourceId,
            target: edge.targetId,
            label: edge.name
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

    GetFlow()
        .then((result: main.AttackFlow) => {
            console.log('Graph:', result);
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
        colorMode="dark"
        onnodeclick={(event) => {
            console.log('on node click', event, event.node);
            selectedStep = event.node.data.step as campaign.AttackStep;
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

<Modal
    open={selectedStep !== null}
    onOpenChange={(e) => {
        console.log('Modal open:', e.open);
        if (!e.open) {
            selectedStep = null;
        }
    }}
    contentBase="bg-surface-100-900 p-4 space-y-4 shadow-xl w-[480px] h-screen flex flex-col"
    positionerJustify="justify-end"
    positionerAlign=""
    positionerPadding=""
    transitionsPositionerIn={{
        x: 480,
        duration: 200
    }}
    transitionsPositionerOut={{ x: 480, duration: 200 }}
>
    <!-- {#snippet trigger()}Open Drawer{/snippet} -->
    {#snippet content()}
        <AttackStepDetails step={selectedStep!} />
    {/snippet}
</Modal>

<style>
    :global(.svelte-flow__node) {
        text-align: center;
    }
</style>
