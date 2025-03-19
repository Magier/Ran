<script lang="ts">
	import Armory from './components/armory.svelte';
	import store from '$lib/stores/store';
	import * as runtime from '$lib/wailsjs/runtime';
	import { ActionSelected, StartEmulation } from '$lib/wailsjs/go/main/App.js';
	// import { onMount } from 'svelte';
	import Icon from '@iconify/svelte';
	import type { TTP } from '$lib/model';
	import Graph from './components/graph.svelte';
	import { type main } from '$lib/wailsjs/go/models';

	// onMount(() => {
	// 	store.connectBackend();
	// });
	function start(): void {
		StartEmulation(target);
		// StartEmulation(target).then((result) => (resultText = result));
	}
	let target: string = $state('default/kubelet-reader-pod');

	let selectedNodeId: string = $state('');
	let selectedNode: main.Node | undefined = $state();
	let activeGlobalConditions: Object = {};

	function sendAction(ttp: TTP) {
		// const ttp = event.detail;
		console.log(ttp);

	let target: string = 'default/kubelet-reader-pod';
			ActionSelected(ttp.id, selectedNodeId, '');
			// store.sendMessage('execute_ttp', {
			// 	target: selectedNodeId,
			// 	ttp_id: ttp.id,
			// 	technique: ttp.technique,
			// 	action: ttp.action,
			// 	cmd_args: ttp.cmd_args
			// });
	}
</script>

<div class="items-top mx-auto flex h-full justify-center">
	{#await store.connect(false)}
		<Icon icon="game-icons:fishing-net" rotate={90} class="fill-token h-64 w-64 -scale-x-[100%]" />
		<div>loading...</div>
	{:then sessions}
		<div class="basis-3/4">
			<div class="flex items-center">
				<input
					autocomplete="off"
					bind:value={target}
					id="target"
					type="text"
					class="mr-2 rounded-l p-2"
				/>
				<button onclick={start} class="btn preset-filled-primary-500">Start</button>
				<span>{selectedNodeId}</span>
			</div>
			<Graph bind:selectedNodeId bind:selectedNode />
		</div>
		<Armory class="basis-1/4" action={sendAction} />
	{:catch err}
		<div class="justify-center">
			<figure>
				<section class="img-bg"></section>
				<Icon
					icon="game-icons:fishing-net"
					rotate={90}
					class="fill-token h-64 w-64 -scale-x-[100%]"
				/>
			</figure>
			<h2 class="h2 text-center">Ran</h2>
			{err}
		</div>
	{/await}
	<!-- globalConditions={activeGlobalConditions}
		{selectedNode}
	/> -->
</div>

<style>
	* {
		color: white;
	}
</style>
