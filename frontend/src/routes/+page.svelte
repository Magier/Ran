<script lang="ts">
	import Armory from './components/armory.svelte';
	import { domain, api} from '$lib/domain/models';
	import Icon from '@iconify/svelte';
	import Graph from './components/graph.svelte';
	import { Dialog, Popover, Portal } from '@skeletonlabs/skeleton-svelte';
	import ActionParamsModal from '$lib/modals/ActionParamsModal.svelte';
	import { onMount } from 'svelte';
	import { showToast, toaster } from '$lib/components/toaster';
	import EntityInfo from './components/entityInfo.svelte';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
	import { getRanAPI } from '$lib/ran_api';

	const campaignState = getCampaignState();

	const ranAPI = getRanAPI();

	let selectedObjectId: string = $state('');
	let selectedObject: api.Node | undefined = $state();
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
			campaignState.ExecuteAction(ttp.id, selectedObjectId, '', {}).then(() => {
				showToast(`Executed TTP ${ttp.name}`, '', 'success');
			}).catch((err) => {
				showToast(`Error executing TTP ${ttp.name}`, err, 'error');
			});
			// campaignState.ExecuteAction(ttp.id, selectedObjectId, '', {});
		}
	}

	function closeModal() {
		showParamModal = false;
	}
	function deleteSelectedNode() {
		campaignState.sendMessage('delete_entity', {
			target: selectedObjectId
		});
	}

	onMount(() => {
		// TODO: check if this alert handle is still useful
		campaignState.api.on('alert', (alert) => {
			console.log('Store Alert ', alert);
		});
	});

	const ToastMapping: Record<string, string> = {};
	function onExecuteTTP(ttpId: string, procedureId: string, args: Record<string, string>) {
		// TODO: cleanup, register handler only once
		ranAPI.on('ttp-executed', (data) => {
			console.log('TTP Executed Event', data);
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
		campaignState.ExecuteAction(ttpId, selectedObjectId, procedureId, args)
			.then(() => {
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
	{#await campaignState.init()}
		<Icon icon="game-icons:fishing-net" rotate={90} class="fill-token h-64 w-64 -scale-x-100" />
		<div>loading...</div>
	{:then sessions}
		<Armory class="h-full min-h-0" action={sendAction} targetId={selectedObjectId} />
		<Graph bind:selectedObjectId bind:selectedObject class="flex-1 h-full min-h-0" />

<Popover
	open={showDetails}
	onOpenChange={(e) => e.open}
	positioning={{ placement: 'top-end', fitViewport: true }}
	portalled={false}
>
	   <Popover.Anchor>
      <div id="info-anchor" class="absolute top-10 right-50"></div>
     </Popover.Anchor>
	<Portal>
		<Popover.Positioner>
			<Popover.Content class="border border-surface-600 w-110 rounded-lg bg-surface-100-900 p-4 shadow-xl ">
				<svelte:boundary onerror={handleError}>
					<EntityInfo selectedObject={selectedObject} {sendAction}/>
				</svelte:boundary>
			</Popover.Content>
		</Popover.Positioner>
	</Portal>
</Popover>

<Dialog
	open={showParamModal}
	onOpenChange={(e) => (showParamModal = e.open)}
>
	<Portal>
		<Dialog.Backdrop class="fixed inset-0 z-50 bg-surface-50-950/50"/>
		<Dialog.Positioner class="fixed inset-0 z-50 flex justify-center items-center">
			<Dialog.Content class="card min-w-modal bg-surface-100-900 p-8 space-y-4 shadow-xl">
				{#if selectedTTP}
					<ActionParamsModal
						targetId={selectedObjectId}
						argContext={ttpArgContext}
						ttp={selectedTTP!}
						onCancel={closeModal}
						onExecute={onExecuteTTP}
					/>
				{/if}
			</Dialog.Content>
		</Dialog.Positioner>
	</Portal>
</Dialog>
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
