<script lang="ts">
	import Armory from './components/armory.svelte';
	import type { Node, TTP, ScoredCandidate } from '$lib/api/index';
	import Icon from '@iconify/svelte';
	import Graph from './components/graph.svelte';
	import { Dialog, Popover, Portal } from '@skeletonlabs/skeleton-svelte';
	import ActionParamsModal from '$lib/modals/ActionParamsModal.svelte';
	import FileViewerModal from '$lib/modals/FileViewerModal.svelte';
	import { onMount, onDestroy } from 'svelte';
	import { toaster } from '$lib/components/toaster';
	import { timeline } from '$lib/stores/timelineStore.svelte';
	import OperationTimeline from '$lib/components/OperationTimeline.svelte';
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
	const MIN_ENTITYINFO_WIDTH = 200;
	const MAX_ENTITYINFO_WIDTH = 800;
	const MIN_ENTITYINFO_HEIGHT = 100;
	const MAX_ENTITYINFO_HEIGHT = 1000;
	const DEFAULT_ENTITYINFO_WIDTH = 300;
	const DEFAULT_ENTITYINFO_HEIGHT = 400;
	let entityInfoWidth = $state(DEFAULT_ENTITYINFO_WIDTH);
	let entityInfoHeight = $state(DEFAULT_ENTITYINFO_HEIGHT);
	let hasManuallyResizedEntityInfo = $state(false);
	let isResizingEntityInfo = $state(false);
	let entityInfoRafId: number | null = null;
	let entityInfoContainer: HTMLDivElement | undefined = $state();

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
			
			if (savedWidth && savedHeight) {
				// Only load and apply saved dimensions if both exist (indicating user manually resized)
				entityInfoWidth = Math.max(MIN_ENTITYINFO_WIDTH, Math.min(MAX_ENTITYINFO_WIDTH, parseInt(savedWidth)));
				entityInfoHeight = Math.max(MIN_ENTITYINFO_HEIGHT, Math.min(MAX_ENTITYINFO_HEIGHT, parseInt(savedHeight)));
				hasManuallyResizedEntityInfo = true;
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
		// Measure current dimensions before switching to fixed sizing
		if (entityInfoContainer && !hasManuallyResizedEntityInfo) {
			entityInfoWidth = entityInfoContainer.offsetWidth;
			entityInfoHeight = entityInfoContainer.offsetHeight;
		}
		isResizingEntityInfo = true;
		hasManuallyResizedEntityInfo = true;
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
			const newWidth = Math.max(MIN_ENTITYINFO_WIDTH, Math.min(MAX_ENTITYINFO_WIDTH, rightEdge - e.clientX));
			
			// Calculate height from top anchor (68px) to mouse Y
			const topOffset = 68; // navbar (60px) + top-2 (8px)
			const newHeight = Math.max(MIN_ENTITYINFO_HEIGHT, Math.min(MAX_ENTITYINFO_HEIGHT, e.clientY - topOffset));
			
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

	// Reset entity info to content-fit when a new node is selected
	$effect(() => {
		if (selectedObjectId !== '') {
			hasManuallyResizedEntityInfo = false;
		}
	});

	async function sendAction(ttp: TTP, args = {}) {
		selectedTTP = ttp;
		ttpArgContext = { ...args, ...activeGlobalConditions };
		if ((ttp.params?.length ?? 0) > 0) {
			showParamModal = true;
		} else if ((ttp.procedures?.length ?? 0) > 1) {
			showParamModal = true;
		} else {
			const targetName = campaignState.getEntityById(selectedObjectId)?.name ?? selectedObjectId;
			try {
				const result = await ExecuteAction({
					actionId: ttp.id,
					targetId: selectedObjectId,
					procedureId: '',
					args: {}
				});
				const cmdId = (result as any)?.cmdId ?? crypto.randomUUID();
				timeline.addTtpAction({
					id: cmdId,
					ttpId: ttp.id,
					ttpName: ttp.name,
					targetId: selectedObjectId,
					targetName,
					status: 'pending',
					timestamp: new Date()
				});
			} catch (err) {
				handleError(err);
			}
		}
	}

	// Execute a recommendation against its own target. Selects that target, then
	// reuses the standard action flow (param modal for parameterized TTPs,
	// direct execution otherwise).
	function runRecommendation(rec: ScoredCandidate) {
		const ttp = campaignState.getTtpById(rec.ttp_id);
		if (!ttp) {
			handleError(new Error(`Unknown TTP: ${rec.ttp_id}`));
			return;
		}
		selectedObjectId = rec.target_id;
		sendAction(ttp);
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
			const cmdId = data.CmdId ?? data.ID ?? '';
			const targetId = data.TargetID ?? '';
			const execSystemId = data.ExecSystemID ?? '';
			const differsFromTarget = execSystemId && execSystemId !== targetId;
			// Resolves the pending entry for UI-initiated actions, or creates the
			// entry outright for actions driven via MCP / autonomous plans.
			timeline.recordExecutedTtp({
				id: cmdId,
				ttpId: data.TTP?.id ?? '',
				ttpName: data.TTP?.name ?? data.TTP?.id ?? cmdId,
				targetId,
				targetName: campaignState.getEntityById(targetId)?.name ?? targetId,
				execSystemId: differsFromTarget ? execSystemId : undefined,
				execSystemName: differsFromTarget
					? (campaignState.getEntityById(execSystemId)?.name ?? execSystemId)
					: undefined,
				status: data.Success ? 'success' : 'failed',
				failReason: data.Success ? undefined : data.FailReason,
				timestamp: new Date()
			});

			if (data.Success && data.TTP?.id === 'read-file' && data.Args?.PATH) {
				ranAPI.GetFileContent(data.Args.PATH).then((file) => {
					fileViewerPath = file.path ?? data.Args.PATH;
					fileViewerContent = file.content ?? '';
					showFileViewer = true;
				}).catch(() => {});
			}
		});

		ranAPI.on('entity-discovered', (data) => {
			timeline.addEntityEvent({
				kind: data.category ?? 'discovery',
				id: data.entityId,
				entityId: data.entityId,
				entityName: data.entityName,
				entityKind: data.entityKind,
				cmdId: data.cmdId,
				timestamp: new Date()
			});
		});

		// Show actions as in-progress the moment they're dispatched — by this UI, an
		// autonomous plan, MCP, or the CLI — rather than only once they complete.
		// addTtpAction is idempotent: a UI-initiated action already has a pending
		// entry, so this enriches it; an externally-driven one creates a fresh one.
		ranAPI.on('ttp-dispatched', (data) => {
			const cmdId = data.CmdId ?? data.ID ?? '';
			const targetId = data.TargetID ?? '';
			const execSystemId = data.ExecSystemID ?? '';
			const differsFromTarget = execSystemId && execSystemId !== targetId;
			timeline.addTtpAction({
				id: cmdId,
				ttpId: data.TTP?.id ?? '',
				ttpName: data.TTP?.name ?? data.TTP?.id ?? cmdId,
				targetId,
				targetName: campaignState.getEntityById(targetId)?.name ?? targetId,
				execSystemId: differsFromTarget ? execSystemId : undefined,
				execSystemName: differsFromTarget
					? (campaignState.getEntityById(execSystemId)?.name ?? execSystemId)
					: undefined,
				status: 'pending',
				timestamp: new Date()
			});
		});

		// Seed the timeline from the campaign's existing state so a session attached
		// to an already-running campaign isn't blank: completed actions from the
		// execution log, then any in-flight (Ongoing) steps as pending on top. The
		// id-index dedup makes this safe alongside the live handlers above.
		ranAPI.GetExecutionRecords()
			.then((records) => {
				timeline.backfill(
					records.map((r) => {
						const differsFromTarget = r.exec_system_id && r.exec_system_id !== r.target_id;
						return {
							id: r.id,
							ttpId: r.ttp_id,
							ttpName: r.ttp_name || r.ttp_id || r.id,
							targetId: r.target_id,
							targetName: campaignState.getEntityById(r.target_id)?.name ?? r.target_id,
							execSystemId: differsFromTarget ? r.exec_system_id : undefined,
							execSystemName: differsFromTarget
								? (campaignState.getEntityById(r.exec_system_id)?.name ?? r.exec_system_id)
								: undefined,
							success: r.success,
							failReason: r.fail_reason,
							timestampMs: r.completed_at_ms || r.started_at_ms
						};
					})
				);
			})
			.catch((err) => console.error('Timeline backfill failed', err))
			.finally(() => {
				ranAPI.GetFlow()
					.then((flow) => {
						timeline.backfillPending(
							flow.steps
								.filter((s) => s.status === 'Ongoing')
								.map((s) => {
									const differsFromTarget = s.executedOn && s.executedOn !== s.targetId;
									return {
										id: s.id,
										ttpId: s.TTP.id,
										ttpName: s.TTP.name || s.TTP.id || s.id,
										targetId: s.targetId,
										targetName: campaignState.getEntityById(s.targetId)?.name ?? s.targetId,
										execSystemId: differsFromTarget ? s.executedOn : undefined,
										execSystemName: differsFromTarget
											? (campaignState.getEntityById(s.executedOn)?.name ?? s.executedOn)
											: undefined,
										timestampMs: Date.parse(s.startedAt) || 0
									};
								})
						);
					})
					.catch((err) => console.error('Timeline pending backfill failed', err));
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

	async function onExecuteTTP(ttpId: string, execSystemId: string, authIdentityId: string, procedureId: string, args: Record<string, string>) {
		const ttp = campaignState.getTtpById(ttpId);
		const targetName = campaignState.getEntityById(selectedObjectId)?.name ?? selectedObjectId;

		closeModal();

		try {
			const result = await ExecuteAction({ actionId: ttpId, execSystemId, authIdentityId: authIdentityId || undefined, targetId: selectedObjectId, procedureId, args });
			const cmdId = (result as any)?.cmdId ?? crypto.randomUUID();
			const differsFromTarget = execSystemId && execSystemId !== selectedObjectId;
			timeline.addTtpAction({
				id: cmdId,
				ttpId,
				ttpName: ttp?.name ?? ttpId,
				targetId: selectedObjectId,
				targetName,
				execSystemId: differsFromTarget ? execSystemId : undefined,
				execSystemName: differsFromTarget
					? (campaignState.getEntityById(execSystemId)?.name ?? execSystemId)
					: undefined,
				status: 'pending',
				timestamp: new Date()
			});
		} catch (err) {
			handleError(err);
		}
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

<div class="relative flex h-[calc(100vh-35px)] gap-x-0">
	{#if campaignState.isReady()}
		<!-- Armory panel -->
		<div
			class="bg-surface-100-900 flex-shrink-0 {isResizing ? '' : 'transition-[width] duration-300 ease-in-out'}"
			style="width: {armoryCollapsed ? '0px' : `${armoryWidth}px`}; overflow: hidden;"
		>
			<Armory
				class="h-full min-h-0 w-full"
				action={sendAction}
				runRecommendation={runRecommendation}
				targetId={selectedObjectId}
				target={selectedObject}
				bind:focusSearch={focusArmorySearch}
			/>
		</div>

		<!-- Resize handle -->
		{#if !armoryCollapsed}
			<button
				class="w-px shrink-0 cursor-col-resize border-0 p-0 bg-surface-200-800 hover:bg-primary-500 transition-colors"
				onmousedown={startResize}
				aria-label="Resize armory panel"
			></button>
		{/if}

		<!-- Collapse/Expand button -->
		<button
			class="absolute left-0 z-50 bg-surface-200-800 hover:bg-surface-300-700 border border-surface-400-600 rounded-r-md px-0.5 py-2 opacity-30 hover:opacity-100 transition-all duration-200"
			style="left: {armoryCollapsed ? '0' : `${armoryWidth}px`}; top: 0.5rem;"
			onclick={toggleArmoryCollapse}
			title={armoryCollapsed ? 'Expand armory' : 'Collapse armory'}
		>
			<Icon
				icon={armoryCollapsed ? 'mdi:chevron-right' : 'mdi:chevron-left'}
				width="16"
				class="text-surface-contrast-200-800"
			/>
		</button>

<!-- Graph area with EntityInfo overlay and Action Log drawer -->
	<div class="flex-1 min-w-0 flex flex-col min-h-0">
		<div class="flex-1 min-h-0 relative">
			<Graph bind:selectedObjectId={selectedObjectId} bind:selectedObject class="h-full" />

			{#if selectedObjectId !== ''}
				<svelte:boundary onerror={handleError}>
					<div
						bind:this={entityInfoContainer}
						class="absolute top-2 right-2 flex flex-col z-50"
						class:max-w-[800px]={!hasManuallyResizedEntityInfo}
						class:max-h-[calc(100vh-80px)]={!hasManuallyResizedEntityInfo}
						style={hasManuallyResizedEntityInfo ? `width: ${entityInfoWidth}px; height: ${entityInfoHeight}px;` : 'width: fit-content; height: fit-content;'}
					>
						<EntityInfo class={hasManuallyResizedEntityInfo ? "overflow-auto flex-1" : "overflow-auto"} objectId={selectedObjectId} {sendAction} />
						<!-- Resize handle at bottom-left corner -->
						<button
						class="absolute bottom-0 left-0 w-4 h-4 cursor-nwse-resize opacity-30 hover:opacity-100 transition-opacity bg-gradient-to-bl from-transparent from-50% to-current to-50% rounded-bl-lg"
							onmousedown={startResizeEntityInfo}
							aria-label="Resize entity info panel"
						></button>
					</div>
				</svelte:boundary>
			{/if}
		</div>

		{#if timeline.open}
			<OperationTimeline
				entries={timeline.topEntries}
				onfocusentity={(id) => { selectedObjectId = id; }}
				ontogglegroup={(cmdId) => timeline.toggleGroup(cmdId)}
			/>
		{/if}
	</div>

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
