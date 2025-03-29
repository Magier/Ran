<script lang="ts">
	import { GetTrace } from '$lib/wailsjs/go/main/App';
	import { main } from '$lib/wailsjs/go/models';
	import Graph from '../components/graph.svelte';

	let nodes: main.Node[] = $state([]);
	let selectedNodeId: string = $state('');
	GetTrace()
		.then((result: main.Graph) => {
			nodes = result.nodes;
			console.log('trace');
			console.log(result);
		})
		.catch((err) => {
			console.error(err);
		});
</script>

<div class="items-top mx-auto flex h-full w-full justify-center">
	{#each nodes as node}
		<div class="border-primary-950-50 m-4 rounded-lg border-2 p-6 shadow-lg">
			{node.name}
		</div>
	{/each}
	<!-- <Graph bind:selectedNodeId /> -->
</div>
