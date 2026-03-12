<script lang="ts">
	import Armory from './components/armory.svelte';
	import type { Node, TTP } from '$lib/api/index';
	import Icon from '@iconify/svelte';
	import Graph from './components/graph.svelte';
	import { Dialog, Popover, Portal } from '@skeletonlabs/skeleton-svelte';
	import ActionParamsModal from '$lib/modals/ActionParamsModal.svelte';
	import { onMount, onDestroy } from 'svelte';
	import { toaster } from '$lib/components/toaster';
	import EntityInfo from './components/entityInfo.svelte';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
	import { ExecuteAction, getRanAPI } from '$lib/ran_api';
	import { browser } from '$app/environment';

	const campaignState = getCampaignState();

	const ranAPI = getRanAPI();

	let selectedObjectId: string = $state('');
	let selectedObject: Node | undefined = $state();
	let ttpArgContext: Record<string, any> = $state({});
	let showParamModal: boolean = $state(false);
	let activeGlobalConditions: Object = {};
	let selectedTTP: TTP | undefined = $state();
	let focusArmorySearch: () => void = $state(() => {});

	$effect(() => {
		let _ = campaignState.campaignId;
		// reset the page/graph state when the campaign resets
		selectedObjectId = '';
		selectedObject = undefined;
	})

	function sendAction(ttp: TTP, args = {}) {
		selectedTTP = ttp;
		ttpArgContext = { ...args, ...activeGlobalConditions };
		if (ttp.params) {
			showParamModal = true;
		} else if ((ttp.procedures?.length ?? 0) > 1) {
			showParamModal = true;
		} else {
			const toastId = toaster.create({
				title: `Executing "${ttp.id}"`,
				type: 'info',
				duration: Infinity,
				meta: { spinner: true }
			});
			ToastMapping[ttp.id] = toastId;

			ExecuteAction({actionId: ttp.id, targetId: selectedObjectId, procedureId: '', args: {}})
				.catch((err) => {
					const id = ToastMapping[ttp.id];
					delete ToastMapping[ttp.id];
					toaster.dismiss(id);
					toaster.create({
						title: `Error executing "${ttp.id}"`,
						description: typeof err === 'string' ? err : (err?.message ?? 'Unknown error'),
						type: 'error',
						duration: 5000
					});
				});
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

	function handleKeyPress(event: KeyboardEvent) {
		// Only trigger if not typing in an input/textarea
		if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) {
			return;
		}

		if (event.key === 'a') {
			event.preventDefault();
			focusArmorySearch();
		}
	}

	onMount(() => {
		if (browser) {
			window.addEventListener('keydown', handleKeyPress);
		}

		// Initialize campaign if not already done
		if (campaignState.armory.size === 0) {
			campaignState.init();
		}

		// TODO: check if this alert handle is still useful
		campaignState.api.on('alert', (alert) => {
			console.log('Store Alert ', alert);
		});

		ranAPI.on('ttp-executed', (data) => {
			console.log('TTP Executed Event', data);
			const key = data.TTP?.id;
			const toastId = ToastMapping[key];
			if (!toastId) return;
			delete ToastMapping[key];

			toaster.dismiss(toastId);

			if (data.Success) {
				toaster.create({
					title: `"${data.TTP.name}" executed successfully`,
					description: 'Executed successfully',
					type: 'success',
					duration: 5000
				});
			} else {
				toaster.create({
					title: `"${data.TTP.name}" failed`,
					description: data.FailReason ?? 'Failed for unknown reason',
					type: 'error',
					duration: 5000
				});
			}
		});
	});

	onDestroy(() => {
		if (browser) {
			window.removeEventListener('keydown', handleKeyPress);
		}
	});

	const ToastMapping: Record<string, string> = {};
	function onExecuteTTP(ttpId: string, execSystemId: string, procedureId: string, args: Record<string, string>) {
		const toastId = toaster.create({
			title: `Executing "${ttpId}"`,
			type: 'info',
			duration: Infinity,
			meta: { spinner: true }
		});
		ToastMapping[ttpId] = toastId;

		closeModal();

		ExecuteAction({actionId: ttpId, execSystemId, targetId: selectedObjectId, procedureId, args})
			.catch((err) => {
				const id = ToastMapping[ttpId];
				delete ToastMapping[ttpId];
				toaster.dismiss(id);
				toaster.create({
					title: `Error executing "${ttpId}"`,
					description: typeof err === 'string' ? err : (err?.message ?? 'Unknown error'),
					type: 'error',
					duration: 5000
				});
			});
	}

	function handleError(e: unknown) {
		let description = 'Unknown error';
		if (e instanceof Error) {
			description = e.message;
		} else if (typeof e === 'string') {
			description = e;
		}

		console.error(e)

		toaster.create({
			title: 'Error',
			description,
			type: 'error'
		});
	}
</script>

<div class="relative grid h-[calc(100vh-60px)] grid-cols-[300px_minmax(0,1fr)] gap-x-1">
	{#if campaignState.armory.size > 0}
		<Armory
			class="h-full min-h-0"
			action={sendAction}
			targetId={selectedObjectId}
			bind:focusSearch={focusArmorySearch}
		/>
		<Graph bind:selectedObjectId={selectedObjectId} bind:selectedObject class="flex-1 h-full min-h-0" />

		{#if selectedObjectId !== ''}
			<svelte:boundary onerror={handleError}>
				<EntityInfo class="absolute top-2 right-2" objectId={selectedObjectId} {sendAction} />
			</svelte:boundary>
		{/if}

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
	{:else}
		<div class="flex items-center justify-center w-full h-full">
			<div class="text-center">
				<Icon icon="game-icons:fishing-net" rotate={90} class="fill-token h-64 w-64 -scale-x-100 mx-auto" />
				<div class="mt-4 text-surface-600-400">Loading campaign...</div>
			</div>
		</div>
	{/if}
</div>

