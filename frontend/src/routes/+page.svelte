<script lang="ts">
	import Armory from './components/armory.svelte';
	import type { Node, TTP } from '$lib/api/index';
	import Icon from '@iconify/svelte';
	import Graph from './components/graph.svelte';
	import { Dialog, Popover, Portal } from '@skeletonlabs/skeleton-svelte';
	import ActionParamsModal from '$lib/modals/ActionParamsModal.svelte';
	import FileViewerModal from '$lib/modals/FileViewerModal.svelte';
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
	let showFileViewer: boolean = $state(false);
	let fileViewerPath: string = $state('');
	let fileViewerContent: string = $state('');

	// Armory resize and collapse state
	const ARMORY_WIDTH_KEY = '_armoryWidth';
	const ARMORY_COLLAPSED_KEY = '_armoryCollapsed';
	let armoryWidth = $state(300); // default width
	let armoryCollapsed = $state(false);
	let isResizing = $state(false);
	let rafId: number | null = null;

	// EntityInfo resize state
	const ENTITYINFO_WIDTH_KEY = '_entityInfoWidth';
	const ENTITYINFO_HEIGHT_KEY = '_entityInfoHeight';
	let entityInfoWidth = $state(300);
	let entityInfoHeight = $state(400);
	let isResizingEntityInfo = $state(false);
	let entityInfoRafId: number | null = null;

	// Get responsive default armory width based on screen size
	function getResponsiveDefaultWidth(): number {
		if (!browser) return 300;
		const width = window.innerWidth;
		if (width < 768) return 250; // mobile
		if (width < 1024) return 280; // tablet
		if (width < 1440) return 300; // small desktop
		return 350; // large desktop
	}

	// Get responsive min/max constraints based on screen size
	function getResponsiveConstraints(): { min: number; max: number } {
		if (!browser) return { min: 200, max: 600 };
		const width = window.innerWidth;
		if (width < 768) return { min: 200, max: 400 }; // mobile
		if (width < 1024) return { min: 250, max: 500 }; // tablet
		return { min: 250, max: 650 }; // desktop
	}

	function loadArmoryPreferences() {
		if (!browser) return;
		try {
			const savedWidth = sessionStorage.getItem(ARMORY_WIDTH_KEY);
			const savedCollapsed = sessionStorage.getItem(ARMORY_COLLAPSED_KEY);
			
			const { min, max } = getResponsiveConstraints();
			const defaultWidth = getResponsiveDefaultWidth();
			
			if (savedWidth) {
				armoryWidth = Math.max(min, Math.min(max, parseInt(savedWidth)));
			} else {
				armoryWidth = defaultWidth;
			}
			
			if (savedCollapsed) armoryCollapsed = savedCollapsed === 'true';
		} catch (e) {
			console.warn('Failed to load armory preferences:', e);
		}
	}

	function saveArmoryPreferences() {
		if (!browser) return;
		try {
			sessionStorage.setItem(ARMORY_WIDTH_KEY, armoryWidth.toString());
			sessionStorage.setItem(ARMORY_COLLAPSED_KEY, armoryCollapsed.toString());
		} catch (e) {
			console.warn('Failed to save armory preferences:', e);
		}
	}

	function loadEntityInfoPreferences() {
		if (!browser) return;
		try {
			const savedWidth = sessionStorage.getItem(ENTITYINFO_WIDTH_KEY);
			const savedHeight = sessionStorage.getItem(ENTITYINFO_HEIGHT_KEY);
			
			if (savedWidth) {
				entityInfoWidth = Math.max(200, Math.min(800, parseInt(savedWidth)));
			}
			if (savedHeight) {
				entityInfoHeight = Math.max(300, Math.min(1000, parseInt(savedHeight)));
			}
		} catch (e) {
			console.warn('Failed to load entityInfo preferences:', e);
		}
	}

	function saveEntityInfoPreferences() {
		if (!browser) return;
		try {
			sessionStorage.setItem(ENTITYINFO_WIDTH_KEY, entityInfoWidth.toString());
			sessionStorage.setItem(ENTITYINFO_HEIGHT_KEY, entityInfoHeight.toString());
		} catch (e) {
			console.warn('Failed to save entityInfo preferences:', e);
		}
	}

	function startResize(e: MouseEvent) {
		isResizing = true;
		e.preventDefault();
		// Prevent text selection during drag
		document.body.style.userSelect = 'none';
		document.body.style.cursor = 'col-resize';
	}

	function handleMouseMove(e: MouseEvent) {
		if (!isResizing) return;
		
		// Cancel any pending animation frame
		if (rafId !== null) {
			cancelAnimationFrame(rafId);
		}
		
		// Use requestAnimationFrame for smooth updates
		rafId = requestAnimationFrame(() => {
			const { min, max } = getResponsiveConstraints();
			const newWidth = Math.max(min, Math.min(max, e.clientX));
			armoryWidth = newWidth;
			rafId = null;
		});
	}

	function stopResize() {
		if (isResizing) {
			isResizing = false;
			// Restore default cursor and text selection
			document.body.style.userSelect = '';
			document.body.style.cursor = '';
			// Cancel any pending animation frame
			if (rafId !== null) {
				cancelAnimationFrame(rafId);
				rafId = null;
			}
			saveArmoryPreferences();
		}
	}

	function toggleArmoryCollapse() {
		armoryCollapsed = !armoryCollapsed;
		saveArmoryPreferences();
	}

	function startResizeEntityInfo(e: MouseEvent) {
		isResizingEntityInfo = true;
		e.preventDefault();
		e.stopPropagation();
		document.body.style.userSelect = 'none';
		document.body.style.cursor = 'nwse-resize';
	}

	function handleMouseMoveEntityInfo(e: MouseEvent) {
		if (!isResizingEntityInfo) return;
		
		if (entityInfoRafId !== null) {
			cancelAnimationFrame(entityInfoRafId);
		}
		
		entityInfoRafId = requestAnimationFrame(() => {
			// EntityInfo is anchored at top-right with: top-2 (8px) + navbar (60px) = 68px from top, right-2 (8px) from right
			// Calculate width from mouse X to right edge (minus the 8px offset)
			const rightEdge = window.innerWidth - 8;
			const newWidth = Math.max(200, Math.min(800, rightEdge - e.clientX));
			
			// Calculate height from top anchor (68px) to mouse Y
			const topOffset = 68; // navbar (60px) + top-2 (8px)
			const newHeight = Math.max(300, Math.min(1000, e.clientY - topOffset));
			
			entityInfoWidth = newWidth;
			entityInfoHeight = newHeight;
			entityInfoRafId = null;
		});
	}

	function stopResizeEntityInfo() {
		if (isResizingEntityInfo) {
			isResizingEntityInfo = false;
			document.body.style.userSelect = '';
			document.body.style.cursor = '';
			if (entityInfoRafId !== null) {
				cancelAnimationFrame(entityInfoRafId);
				entityInfoRafId = null;
			}
			saveEntityInfoPreferences();
		}
	}

	function handleWindowResize() {
		if (armoryCollapsed || isResizing) return;
		// Adjust armory width to stay within responsive constraints
		const { min, max } = getResponsiveConstraints();
		if (armoryWidth < min) armoryWidth = min;
		if (armoryWidth > max) armoryWidth = max;
	}

	$effect(() => {
		loadArmoryPreferences();
		loadEntityInfoPreferences();
	});

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
			window.addEventListener('mousemove', handleMouseMove);
			window.addEventListener('mouseup', stopResize);
			window.addEventListener('resize', handleWindowResize);
			
			// EntityInfo resize listeners
			window.addEventListener('mousemove', handleMouseMoveEntityInfo);
			window.addEventListener('mouseup', stopResizeEntityInfo);
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

				if (data.TTP?.id === 'read-file' && data.Args?.PATH) {
					ranAPI.GetFileContent(data.Args.PATH).then((file) => {
						fileViewerPath = file.path ?? data.Args.PATH;
						fileViewerContent = file.content ?? '';
						showFileViewer = true;
					}).catch(() => {});
				}
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
			window.removeEventListener('mousemove', handleMouseMove);
			window.removeEventListener('mouseup', stopResize);
			window.removeEventListener('resize', handleWindowResize);
			
			// EntityInfo resize listeners
			window.removeEventListener('mousemove', handleMouseMoveEntityInfo);
			window.removeEventListener('mouseup', stopResizeEntityInfo);
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

<div class="relative flex h-[calc(100vh-60px)] gap-x-0">
	{#if campaignState.armory.size > 0}
		<!-- Armory panel -->
		<div
			class="bg-surface-100-900 flex-shrink-0"
			class:armory-transition={!isResizing}
			style="width: {armoryCollapsed ? '0px' : `${armoryWidth}px`}; overflow: hidden;"
		>
			<Armory
				class="h-full min-h-0 w-full"
				action={sendAction}
				targetId={selectedObjectId}
				target={selectedObject}
				bind:focusSearch={focusArmorySearch}
			/>
		</div>

		<!-- Resize handle -->
		{#if !armoryCollapsed}
			<button
				class="resize-handle w-2 cursor-col-resize flex-shrink-0 border-0 p-0"
				onmousedown={startResize}
				aria-label="Resize armory panel"
			></button>
		{/if}

		<!-- Collapse/Expand button -->
		<button
			class="collapse-button absolute left-0 bottom-2 z-50 rounded-r-md px-0.5 py-2"
			class:armory-transition={!isResizing}
			style="left: {armoryCollapsed ? '0' : `${armoryWidth}px`};"
			onclick={toggleArmoryCollapse}
			title={armoryCollapsed ? 'Expand armory' : 'Collapse armory'}
		>
			<Icon
				icon={armoryCollapsed ? 'mdi:chevron-right' : 'mdi:chevron-left'}
				width="16"
				class="text-surface-500"
			/>
		</button>

		<!-- Graph area -->
		<div class="flex-1 min-w-0">
			<Graph bind:selectedObjectId={selectedObjectId} bind:selectedObject class="h-full" />
		</div>

		{#if selectedObjectId !== ''}
			<svelte:boundary onerror={handleError}>
				<div class="absolute top-2 right-2 flex flex-col" style="width: {entityInfoWidth}px; max-height: {entityInfoHeight}px;">
					<EntityInfo class="overflow-auto" objectId={selectedObjectId} {sendAction} />
					<!-- Resize handle at bottom-left corner -->
					<button
						class="absolute bottom-0 left-0 w-4 h-4 cursor-nwse-resize opacity-30 hover:opacity-100 transition-opacity"
						style="background: linear-gradient(135deg, transparent 50%, currentColor 50%);"
						onmousedown={startResizeEntityInfo}
						aria-label="Resize entity info panel"
					></button>
				</div>
			</svelte:boundary>
		{/if}

		<Dialog open={showParamModal} onOpenChange={(e) => (showParamModal = e.open)}>
			<Portal>
				<Dialog.Backdrop class="fixed inset-0 z-[100] bg-surface-50-950/50" />
				<Dialog.Positioner class="fixed inset-0 z-[100] flex justify-center items-center ">
					<Dialog.Content class="card min-w-modal bg-surface-100-900 p-4 space-y-2 shadow-xl max-h-[90vh] flex flex-col border border-surface-600 ">
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

		<Dialog
			open={showFileViewer}
			onOpenChange={(e) => (showFileViewer = e.open)}
		>
			<Portal>
				<Dialog.Backdrop class="fixed inset-0 z-50 bg-surface-50-950/50"/>
				<Dialog.Positioner class="fixed inset-0 z-50 flex justify-center items-center">
					<Dialog.Content class="card bg-surface-100-900 p-4 space-y-2 shadow-xl max-w-3xl w-full">
						<FileViewerModal
							path={fileViewerPath}
							content={fileViewerContent}
							onClose={() => (showFileViewer = false)}
						/>
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

<style>
	.armory-transition {
		transition: width 0.3s ease-in-out;
	}

	.resize-handle {
		background-color: transparent;
		border-left: 1px solid rgba(128, 128, 128, 0.2);
		transition: opacity 0.2s ease, border-color 0.2s ease;
		opacity: 0.6;
	}

	.resize-handle:hover {
		opacity: 1;
		border-color: rgba(128, 128, 128, 0.5);
	}

	.collapse-button {
		background-color: rgba(0, 0, 0, 0.1);
		border: 1px solid rgba(128, 128, 128, 0.1);
		transition: all 0.2s ease;
		opacity: 0.3;
	}

	.collapse-button:hover {
		background-color: rgba(0, 0, 0, 0.2);
		border-color: rgba(128, 128, 128, 0.3);
		opacity: 1;
	}
</style>

