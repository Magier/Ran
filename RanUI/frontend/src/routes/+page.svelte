<script lang="ts">
	import Armory from './components/armory.svelte';
	import store from '$lib/stores/store';
	import * as runtime from '$lib/wailsjs/runtime';
	import { StartEmulation } from '$lib/wailsjs/go/main/App.js';
	import { onMount } from 'svelte';
	function sendAction(event) {
		console.log(event);
	}

	onMount(() => {
		store.connectBackend(false);
	});
	function start(): void {
		StartEmulation(target);
		// StartEmulation(target).then((result) => (resultText = result));
	}
	let target: string = 'default/kubelet-reader-pod';
</script>

<div class="items-top mx-auto flex h-full justify-center">
	<h1>Ran</h1>
	<div class="input-box" id="input">
		<label for="target">Target: </label>
		<input autocomplete="off" bind:value={target} class="input" id="target" type="text" />
		<button class="btn" on:click={start}>Start</button>
	</div>
	<!-- {#await store.connect(false)}
		<div>loading...</div>
	{:then sessions}
		<Armory class="basis-1/4" on:action={sendAction} />
	{:catch err}
		<div class="justify-center">
			<figure>
				<section class="img-bg" />
				<IconRanLogo class="fill-token h-64 w-64 -scale-x-[100%]" />
			</figure>
			<h2 class="h2 text-center">Ran</h2>
			{err}
		</div>
	{/await} -->
	<!-- globalConditions={activeGlobalConditions}
		{selectedNode}
	/> -->
</div>
