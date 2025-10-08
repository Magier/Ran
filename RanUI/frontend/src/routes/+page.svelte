<script lang="ts">
	import Armory from './components/armory.svelte';
	import store from '$lib/stores/store';
	import * as runtime from '$lib/wailsjs/runtime';
	import { ExecuteAction } from '$lib/wailsjs/go/main/App.js';
	import { domain, main } from '$lib/wailsjs/go/models';
	import Icon from '@iconify/svelte';
	import Graph from './components/graph.svelte';
	import { Modal, Popover } from '@skeletonlabs/skeleton-svelte';
	import ActionParamsModal from '$lib/modals/ActionParamsModal.svelte';
	import { onMount } from 'svelte';
	import { showToast, toaster } from '$lib/components/toaster';
	import EntityInfo from './components/entityInfo.svelte';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';

	const campaignState = getCampaignState();


	let selectedObjectId: string = $state('');
	let selectedObject: main.Node | undefined = $state();
	let showDetails = $derived(selectedObjectId !== '');
	let ttpArgContext: Record<string, any> = $state({});
	let showParamModal: boolean = $state(false);
	let activeGlobalConditions: Object = {};
	let selectedTTP: domain.TTP | undefined = $state();


	$effect(() => {
		let _ = campaignState.campaignId;
		// reset the page/graph state when the campaign resets
		selectedObjectId = '';
		selectedObject = undefined;
	})
	

	function sendAction(ttp: domain.TTP, args = {}) {
		selectedTTP = ttp;
		ttpArgContext = { ...args, ...activeGlobalConditions };
		if (ttp.params) {
			showParamModal = true;
		} else if ((ttp.procedures?.length ?? 0) > 1) {
			showParamModal = true;
		} else {
			ExecuteAction(ttp.id, selectedObjectId, '', {});
		}
	}

	function closeModal() {
		showParamModal = false;
	}
	function deleteSelectedNode() {
		store.sendMessage('delete_entity', {
			target: selectedObjectId
		});
	}

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
				if (!data.Success) {
					toastConfig['description'] = data.FailReason ? data.FailReason : `Failed for unknown reason`;
				} else {
					toastConfig['description'] = `Executed successfully`;
				}
				toaster.update(toastId, toastConfig);
			}
		});

		console.log('Executing TTP', ttpId, selectedObjectId, procedureId, args);
		ExecuteAction(ttpId, selectedObjectId, procedureId, args)
			.then((e) => {
				let toastId = showToast('Executing TTP', ttpId, 'info');
				ToastMapping[ttpId] = toastId;
			})
			.catch((err) => {
				showToast('Error executing TTP', err, 'error');
			});
		closeModal();
	}

	function handleError(e: unknown) {
		let description = 'Unknown error';
		if (e instanceof Error) {
			description = e.message;
		} else if (typeof e === 'string') {
			description = e;
		}

		toaster.create({
			title: 'Error',
			description,
			type: 'error'
		});
	}
</script>

<div class="relative grid h-[calc(100vh-60px)] grid-cols-[300px_minmax(0,1fr)_auto] gap-x-1">
	{#await campaignState.connect(false)}
		<Icon icon="game-icons:fishing-net" rotate={90} class="fill-token h-64 w-64 -scale-x-[100%]" />
		<div>loading...</div>
	{:then sessions}
		<Armory class="h-full min-h-0" action={sendAction} targetId={selectedObjectId} />
		<Graph bind:selectedObjectId bind:selectedObject class="flex-1 h-full min-h-0" />

		<Popover
			open={showDetails}
			onOpenChange={(e) => e.open}
			positioning={{ placement: 'right', fitViewport: true }}
			triggerBase=""
			arrow={false}
			portalled={false} 
			contentBase="border border-surface-600 absolute w-110 -left-110 top-0 top-0 z-10  rounded-lg bg-surface-100-900 p-4 shadow-xl "
		>
			{#snippet trigger()}{/snippet}
			{#snippet content()}
				<svelte:boundary onerror={handleError}>
					<EntityInfo selectedObject={selectedObject} {sendAction}/>
				</svelte:boundary>
			{/snippet}
		</Popover>

		<Modal
			open={showParamModal}
			onOpenChange={(e) => (showParamModal = e.open)}
			contentBase="card min-w-modal bg-surface-100-900 p-8 space-y-4 shadow-xl"
			backdropClasses="backdrop-blur-sm"
		>
			{#snippet content()}
				<ActionParamsModal
					targetId={selectedObjectId}
					argContext={ttpArgContext}
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
