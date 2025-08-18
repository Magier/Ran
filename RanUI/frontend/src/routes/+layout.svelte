<script lang="ts">
	import { AppBar, Navigation, Toaster } from '@skeletonlabs/skeleton-svelte';
	import { page } from '$app/state';
	import IconMap from '~icons/game-icons/treasure-map';
	import IconSteps from '~icons/game-icons/footsteps';
	import { toaster } from '$lib/components/toaster';
	import '../app.css';
	import { getCampaignState, setCampaignState } from '$lib/components/CampaignState.svelte';
	let { children } = $props();

	setCampaignState();
	const campaignState = getCampaignState();
</script>

<AppBar>
	{#snippet lead()}
		<!-- <ArrowLeft size={24} /> -->
		<span>Ran</span>
	{/snippet}

	{#snippet trail()}
		<nav>
			<a class="{page.url.pathname === '/' ? 'selected' : ''} pr-3" href="/"
				><IconMap class="inline-block text-xl" />Graph</a
			>
			<a class={page.url.pathname === '/flow' ? 'selected' : ''} href="/flow"
				><IconSteps class="inline-block text-xl" />Flow</a
			>
			<button class="reset-button" onclick={() => campaignState.reset()}>Reset</button>
		</nav>
	{/snippet}
</AppBar>

<Toaster {toaster}></Toaster>
<main class="">
	{@render children()}
</main>

<style>
	* {
		color: #b6b6b6;
	}

	.selected {
		color: white;
	}
</style>
