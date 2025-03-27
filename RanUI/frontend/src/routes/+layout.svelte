<script lang="ts">
	import { AppBar, Navigation } from '@skeletonlabs/skeleton-svelte';
	import HomeIcon from '~icons/mdi/home';
	// import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	// import Paperclip from '@lucide/svelte/icons/paperclip';
	// import Calendar from '@lucide/svelte/icons/calendar';
	// import CircleUser from '@lucide/svelte/icons/circle-user';
	import '../app.css';
	let page = $state('graph');
	let { children } = $props();
</script>

<AppBar>
	{#snippet lead()}
		<!-- <ArrowLeft size={24} /> -->
	{/snippet}
	{#snippet trail()}
		<!-- <Paperclip size={20} />
		<Calendar size={20} />
		<CircleUser size={20} /> -->
	{/snippet}
	<span>Ran</span>
</AppBar>

<div class="card border-surface-100-900 grid h-[640px] w-full grid-cols-[auto_1fr] border-[1px]">
	<!-- Component -->
	<Navigation.Rail value={page} onValueChange={(newValue: string) => (page = newValue)}>
		{#snippet tiles()}
			<Navigation.Tile label="Graph" href="#/graph"><HomeIcon /></Navigation.Tile>
			<Navigation.Tile label="Trace" href="#/trace"><HomeIcon /></Navigation.Tile>
		{/snippet}
	</Navigation.Rail>
	<!-- Content -->
	<div class="flex items-center justify-center">
		{@render children()}
	</div>
</div>

<style>
	* {
		color: #b6b6b6;
	}
</style>
