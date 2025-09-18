<script lang="ts">
	import { onMount } from 'svelte';
	import { AppBar, Navigation, Toaster } from '@skeletonlabs/skeleton-svelte';
	import {setContext} from 'svelte';
	import { page } from '$app/state';
	import IconMap from '~icons/game-icons/treasure-map';
	import IconSteps from '~icons/game-icons/footsteps';
	import { browser } from '$app/environment';
	import { toaster } from '$lib/components/toaster';
	import '../app.css';
	import { getCampaignState, setCampaignState } from '$lib/components/CampaignState.svelte';
	let { children } = $props();

	setCampaignState();
	const campaignState = getCampaignState();

	let isDark: boolean = $state(false)
	function toggle() { isDark = !isDark; }
	setContext('theme', { get isDark() { return isDark }, toggle });

	let mediaQuery: MediaQueryList | null = $state(null);
	onMount(() => {
		if (browser) {
			mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
			isDark = mediaQuery.matches;
			const handler = (event: MediaQueryListEvent) => {
				isDark = event.matches;
			};
			mediaQuery.addEventListener("change", handler);
			return () => {
				mediaQuery?.removeEventListener("change", handler);
			};
		}
	});
</script>

<AppBar>
	{#snippet lead()}
		<!-- <ArrowLeft size={24} /> -->
		<span>Ran</span>
		<button class="reset-button" onclick={() => campaignState.reset()}>Reset</button>
	{/snippet}

	{#snippet trail()}
		<nav>
			<a class="{page.url.pathname === '/' ? 'selected' : ''} pr-3" href="/"
				><IconMap class="inline-block text-xl" />Graph</a
			>
			<a class={page.url.pathname === '/flow' ? 'selected' : ''} href="/flow"
				><IconSteps class="inline-block text-xl" />Flow</a
			>
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
