<script lang="ts">
	import Armory from './components/armory.svelte';
	import store from '$lib/stores/store';
	import * as runtime from '$lib/wailsjs/runtime';
	import { ExecuteAction, StartEmulation } from '$lib/wailsjs/go/main/App.js';
	// import { onMount } from 'svelte';
	import Icon from '@iconify/svelte';
	import Graph from './components/graph.svelte';
	// import ExploitAppModal from '$lib/modals/ExploitAppModal';
	import { GetRunningPods } from '$lib/wailsjs/go/main/App';
	import { domain, main } from '$lib/wailsjs/go/models';
	import { Modal } from '@skeletonlabs/skeleton-svelte';
	import ActionParamsModal from '$lib/modals/ActionParamsModal.svelte';
	import { onMount } from 'svelte';
	import { showToast, toaster } from '$lib/components/toaster';
	import { Combobox } from '@skeletonlabs/skeleton-svelte';
	import EntityInfo from './components/entityInfo.svelte';

	$effect(() => {
		runtime.EventsOn('error', (dataStr) => {
			// let data = JSON.parse(dataStr);
			showToast('Error in backend', dataStr, 'error');
		});
	});

	function start(): void {
		if (selectedTarget === '') {
			showToast('No target selected', 'Please select a target to start the emulation', 'error');
			return;
		}

		StartEmulation(selectedTarget)
			.then((e) => {
				targetIsSet = true;
			})
			.catch((err) => {
				// TODO: show prompt asking to create the pod
				console.error(err);
			});
	}

	interface ComboboxData {
		label: string;
		value: string;
	}
	let selectedTarget = $state('');
	let availablePods: ComboboxData[] = $state([{ label: 'Loading...', value: 'testing' }]);

	$effect(() => {
		GetRunningPods().then((pods) => {
			availablePods = pods.map((pod: string) => ({ label: pod, value: pod }));
			console.log('new available pods');
			console.log(availablePods);
		});
	});

	let targetIsSet: boolean = $state(false);

	let selectedNodeId: string = $state('');
	let selectedNode: main.Node | undefined = $state();
	let showDetails = $derived(selectedNodeId !== '');
	let showParamModal: boolean = $state(false);
	let activeGlobalConditions: Object = {};
	let selectedTTP: domain.TTP | undefined = $state();

	function sendAction(ttp: domain.TTP) {
		selectedTTP = ttp;
		if (ttp.params) {
			showParamModal = true;
		} else if ((ttp.procedures?.length ?? 0) > 1) {
			showParamModal = true;
		} else {
			ExecuteAction(ttp.id, selectedNodeId, '', {});
		}
	}

	function closeModal() {
		showParamModal = false;
	}
	function deleteSelectedNode() {
		store.sendMessage('delete_entity', {
			target: selectedNodeId
		});
	}

	// function onKeydown(event: KeyboardEvent) {
	// 	if (event.key === 'Delete') {
	// 		if (selectedNode) {
	// 			deleteSelectedNode();
	// 		}
	// 	} else if (event.key === '`' || (event.ctrlkey && (event.key === '.' || event.key === '/'))) {
	// 		console.log(` Graph KeyDown: '${event.key}'`);
	// 		event.preventDefault();
	// 		const drawerSettings: DrawerSettings = {
	// 			id: 'console-drawer',
	// 			// Provide your property overrides:
	// 			bgDrawer: 'bg-surface-900 text-white',
	// 			bgBackdrop: 'variant-glass-primary',
	// 			// width: 'w-[280px] md:w-[480px]',
	// 			padding: 'p-4',
	// 			position: 'top',
	// 			rounded: 'rounded-xl'
	// 		};
	// 		drawerStore.open(drawerSettings);
	// 	}
	// }
	onMount(() => {
		store.onAlert((alert) => {
			console.log('Store Alert ', alert);
		});
	});

	const ToastMapping: Record<string, string> = {};
	function onExecuteTTP(ttpId: string, procedureId: string, args: Record<string, string>) {
		runtime.EventsOnce('ttp-executed', (dataStr) => {
			let data = JSON.parse(dataStr);
			const toastType = data.Success ? 'success' : 'error';
			const title = data.Success
				? `TTP ${data.TTP.name} executed successfully`
				: `TTP ${data.TTP.name} failed`;

			const toastConfig = {
				title: title,
				description: data.TTP.name,
				type: toastType,
				duration: 5000
			};

			if (!(ttpId in ToastMapping)) {
				toaster.create(toastConfig);
			} else {
				const toastId = ToastMapping[ttpId];
				delete ToastMapping[ttpId];
				toaster.update(toastId, toastConfig);
			}
		});

		console.log('Executing TTP', ttpId, selectedNodeId, procedureId, args);
		ExecuteAction(ttpId, selectedNodeId, procedureId, args)
			.then((e) => {
				let toastId = showToast('Executing TTP', ttpId, 'info');
				ToastMapping[ttpId] = toastId;
			})
			.catch((err) => {
				showToast('Error executing TTP', err, 'error');
			});
		closeModal();
	}
</script>

<div class="grid h-[calc(100vh-70px)] grid-cols-[300px_minmax(0,1fr)_auto] gap-x-1">
	{#await store.connect(false)}
		<Icon icon="game-icons:fishing-net" rotate={90} class="fill-token h-64 w-64 -scale-x-[100%]" />
		<div>loading...</div>
	{:then sessions}
		<Armory class="" action={sendAction} targetId={selectedNodeId} />
		<div class="flex flex-col">
			{#if !targetIsSet}
				<div class="flex items-center">
					<select class="select" bind:value={selectedTarget}>
						{#each availablePods as pod}
							<option value={pod.value}>{pod.label}</option>
						{/each}
					</select>
					<button onclick={start} class="btn preset-filled-primary-500">Start</button>
					<!-- <input
						autocomplete="off"
						bind:value={selectedTarget}
						id="target"
						type="text"
						class="mr-2 rounded-l p-2"
					/> -->
				</div>
			{/if}
			<Graph bind:selectedNodeId bind:selectedNode class="flex-1 " />
		</div>
		<aside class={['', showDetails ? 'w-96' : 'w-0']}>
			<svelte:boundary onerror={(e) => console.error(e)}>
				<EntityInfo {selectedNode} />
			</svelte:boundary>
		</aside>
		<!-- globalConditions={activeGlobalConditions}
			{selectedNode}
		/> -->
		<Modal
			open={showParamModal}
			onOpenChange={(e) => (showParamModal = e.open)}
			contentBase="card min-w-modal bg-surface-100-900 p-8 space-y-4 shadow-xl"
			backdropClasses="backdrop-blur-sm"
		>
			{#snippet content()}
				<ActionParamsModal
					targetId={selectedNodeId}
					ttp={selectedTTP!}
					onCancel={closeModal}
					onExecute={onExecuteTTP}
				/>
			{/snippet}
		</Modal>
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
</div>

<style>
	* {
		color: white;
	}
</style>
