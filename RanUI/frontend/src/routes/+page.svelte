<script lang="ts">
	import Armory from './components/armory.svelte';
	import store from '$lib/stores/store';
	import * as runtime from '$lib/wailsjs/runtime';
	// import { StartEmulation } from '$lib/wailsjs/go/main/App.js';
	import { onMount } from 'svelte';
	import type { TTP } from '$lib/model';
	function sendAction(ttp: TTP) {
		console.log(ttp);
	}

	// onMount(() => {
	// 	store.connectBackend();
	// });
	function start(): void {
		console.warn('StartEmulation not implemented');
		// StartEmulation(target);
		// StartEmulation(target).then((result) => (resultText = result));
	}
	let target: string = 'default/kubelet-reader-pod';
</script>

<div class="items-top mx-auto flex h-full justify-center">
	<div class="input-box" id="input">
		<label for="target">Target: </label>
		<input autocomplete="off" bind:value={target} class="input" id="target" type="text" />
		<button class="btn" onclick={start}>Start</button>
	</div>
	{#await store.connect(false)}
		<div>loading...</div>
	{:then sessions}
		<div class="basis-3/4"></div>
		<Armory class="basis-1/4" action={sendAction} />
	{:catch err}
		<div class="justify-center">
			<figure>
				<section class="img-bg"></section>
				<IconRanLogo class="fill-token h-64 w-64 -scale-x-[100%]" />
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
	.input-box .btn {
		width: 60px;
		height: 30px;
		line-height: 30px;
		border-radius: 3px;
		border: none;
		margin: 0 0 0 20px;
		padding: 0 8px;
		cursor: pointer;
	}

	.input-box .btn:hover {
		/* background-image: linear-gradient(to top, #cfd9df 0%, #e2ebf0 100%); */
		color: #333333;
	}

	.input-box .input {
		border: none;
		border-radius: 3px;
		outline: none;
		height: 30px;
		line-height: 30px;
		padding: 0 10px;
		/* background-color: rgba(240, 240, 240, 1); */
		-webkit-font-smoothing: antialiased;
	}

	.input-box .input:hover {
		border: none;
		background-color: rgba(255, 255, 255, 1);
	}

	.input-box .input:focus {
		border: none;
		background-color: rgba(255, 255, 255, 1);
	}
</style>
