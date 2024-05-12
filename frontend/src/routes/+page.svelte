<script lang="ts">
	import IconRanLogo from '~icons/game-icons/monster-grasp';

	import Graph from './components/graph.svelte';
	import Armory from './components/armory.svelte';
	import ExploitAppModal from '$lib/modals/ExploitAppModal.svelte';
	import DeployPodModal from '$lib/modals/DeployPodModal.svelte';

	import { Modal, getModalStore } from '@skeletonlabs/skeleton';
	import type { ModalSettings, ModalComponent, ModalStore, DrawerSettings } from '@skeletonlabs/skeleton';

	import { getDrawerStore } from "@skeletonlabs/skeleton";

	import type { TTP } from './model.js';
	import store from '$lib/stores/store';

	const modalStore = getModalStore();
	const drawerStore = getDrawerStore();

	let selected_node_id: string | null = null;
	let selectedNode: Object | null = null;
	let activeGlobalConditions: Object = {};

	function sendAction(event: CustomEvent<TTP>) {
		const ttp = event.detail;

		if (ttp.name === 'Deploy Container') {
			const modalComponent: ModalComponent = { ref: DeployPodModal };
			const modal: ModalSettings = {
				type: 'component',
				component: modalComponent,
				// Data
				title: 'Exploit Application',
				valueAttr: ttp.params,
				response: (params: boolean) => {
					if (params) {
						store.sendMessage('execute_ttp', {
							target: selected_node_id,
							ttp_id: ttp.id || ttp.technique,
							technique: ttp.technique,
							action: ttp.action,
							cmd_args: params,
							params: params
						});
					}
				}
			};

			modalStore.trigger(modal);
		} else if (ttp.params) {
			const modalComponent: ModalComponent = { ref: ExploitAppModal };
			const modal: ModalSettings = {
				type: 'component',
				component: modalComponent,
				// Data
				title: 'Exploit Application',
				valueAttr: ttp.params,
				response: (params: boolean | ExploitParams) => {
					if (params) {
						store.sendMessage('execute_ttp', {
							target: selected_node_id,
							ttp_id: ttp.id,
							technique: ttp.technique,
							action: ttp.action,
							cmd_args: params,
							params: params
						});
					}
				}
			};

			modalStore.trigger(modal);
		} else {
			store.sendMessage('execute_ttp', {
				target: selected_node_id,
				ttp_id: ttp.id,
				technique: ttp.technique,
				action: ttp.action,
				cmd_args: ttp.cmd_args
			});
		}
	}
	function deleteSelectedNode() {
		store.sendMessage('delete_entity', {
			target: selected_node_id
		});
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Delete') {
			if (selectedNode) {
				deleteSelectedNode();
			}
		} else if (event.key === '`' || (event.ctrlkey && (event.key === '.' || event.key === '/'))) {
			console.log(` Graph KeyDown: '${event.key}'`);
			event.preventDefault();
			const drawerSettings: DrawerSettings = {
				id: 'console-drawer',
				// Provide your property overrides:
				bgDrawer: 'bg-surface-900 text-white',
				bgBackdrop: 'variant-glass-primary',
				// width: 'w-[280px] md:w-[480px]',
				padding: 'p-4',
				position: 'top',
				rounded: 'rounded-xl',
			};
			drawerStore.open(drawerSettings);
		}
	}
</script>

<svelte:window on:keydown={onKeydown} />
<div class="h-full mx-auto flex justify-center items-top">
	<!-- <div class="space-y-10 text-center flex flex-col items-center"> -->
	{#await store.isReady}
		<div class="justify-center">
			<figure>
				<section class="img-bg" />
				<IconRanLogo class="fill-token -scale-x-[100%] w-64 h-64" />
			</figure>
			<h2 class="h2 text-center">Ran</h2>
		</div>
	{:then sessions}
		<Graph bind:selected_node_id bind:selectedNode />
		<Armory class="basis-1/4" on:action={sendAction} globalConditions={activeGlobalConditions} {selectedNode} />
	{:catch someError}
		<div class="justify-center">
			<figure>
				<section class="img-bg" />
				<IconRanLogo class="fill-token -scale-x-[100%] w-64 h-64" />
			</figure>
			<h2 class="h2 text-center">Ran</h2>
			{someError}
		</div>
	{/await}
</div>

<style lang="postcss">
	figure {
		@apply flex relative flex-col;
	}
	.img-bg {
		@apply w-64 h-64 md:w-80 md:h-80;
	}
	.img-bg {
		@apply absolute z-[-1] rounded-full blur-[50px] transition-all;
		animation: pulse 5s cubic-bezier(0, 0, 0, 0.5) infinite, glow 5s linear infinite;
	}
	@keyframes glow {
		0% {
			@apply bg-primary-400/50;
		}
		33% {
			@apply bg-secondary-400/50;
		}
		66% {
			@apply bg-tertiary-400/50;
		}
		100% {
			@apply bg-primary-400/50;
		}
	}
	@keyframes pulse {
		50% {
			transform: scale(1.5);
		}
	}
</style>
