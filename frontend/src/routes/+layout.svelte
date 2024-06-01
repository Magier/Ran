<script lang="ts">
	import '../app.postcss';
	import { AppShell, AppBar, Modal } from '@skeletonlabs/skeleton';
	import { LightSwitch } from '@skeletonlabs/skeleton';
	// import IconRanLogo from '~icons/game-icons/monster-grasp';
	import IconRanLogo from '~icons/game-icons/fishing-net';
	import IconReset from '~icons/fluent/arrow-reset-20-filled';
	import { initializeStores } from '@skeletonlabs/skeleton';
	import { Toast, getToastStore } from '@skeletonlabs/skeleton';
	import type { ToastSettings, ToastStore } from '@skeletonlabs/skeleton';
	import { Drawer, getDrawerStore } from '@skeletonlabs/skeleton';

	import  Console  from './components/console.svelte';

	// find icons via https://icon-sets.iconify.design

	// Floating UI for Popups
	import { computePosition, autoUpdate, flip, shift, offset, arrow } from '@floating-ui/dom';
	import { storePopup } from '@skeletonlabs/skeleton';
	import { onMount } from 'svelte';
	import store from '$lib/stores/store';
	storePopup.set({ computePosition, autoUpdate, flip, shift, offset, arrow });

	initializeStores();

	const toastStore = getToastStore();

	onMount(() => {
		store.onAlert((msg: string) => {
			if (msg.length > 0) {
				const t: ToastSettings = { message: msg, background: 'variant-filled-error' };
				toastStore.trigger(t);
			}
		});
	});
	function onReset(event: MouseEvent) {
		store.sendMessage('reset_campaign', {});
	}
</script>

<Toast />
<Drawer><Console/></Drawer>

<!-- App Shell -->
<AppShell>
	<Modal />
	<svelte:fragment slot="header">
		<!-- App Bar -->
		<AppBar>
			<svelte:fragment slot="lead">
				<IconRanLogo />
				<strong class="ml-3 text-xl uppercase">Ran</strong>
			</svelte:fragment>

			<svelte:fragment slot="trail">
				<button class="btn variant-fille" on:click={onReset}>
					<span> <IconReset /> </span>
					<span>Reset</span>
				</button>
				<LightSwitch />
			</svelte:fragment>
		</AppBar>
	</svelte:fragment>
	<!-- <svelte:fragment slot="sidebarRight" /> -->
	<!-- Page Route Content -->
	<slot />
</AppShell>
