<script lang="ts">
    import { buildCampaignFlowDownload } from '$lib/campaignFlow';
    import { getCampaignState } from '$lib/components/CampaignState.svelte';
    import { showToast, toaster } from '$lib/components/toaster';
    import { saveFile } from '$lib/io';
    import PlanPickerModal from '$lib/modals/PlanPickerModal.svelte';
	import {
		buildPlanDownload,
		defaultPlanDescription,
		defaultPlanName,
		planFilename
	} from '$lib/planDownload';
    import { getRanAPI } from '$lib/ran_api';
    import { timeline } from '$lib/stores/timelineStore.svelte';
    import type { PlanSummary } from '$lib/api';
	import Icon from '@iconify/svelte';
	import { Dialog, Menu, Portal } from '@skeletonlabs/skeleton-svelte';
	import { tick } from 'svelte';

    const campaignState = getCampaignState();
    const ranAPI = getRanAPI();

    let showPlanPicker = $state(false);
	let showSavePlan = $state(false);
	let planName = $state('');
	let planFileName = $state('');
	let planFileNameOverridden = $state(false);
	let planDescription = $state('');
	let planIncludeFailed = $state(false);
	let planSaving = $state(false);
	let planNameInput: HTMLInputElement;
    let plansLoading = $state(false);
    let plans = $state<PlanSummary[]>([]);

    async function openPlanPicker() {
        plans = [];
        plansLoading = true;
        showPlanPicker = true;
        try {
            plans = await ranAPI.ListPlans();
        } catch (error) {
            showToast('Failed to list plans', (error as Error).message, 'error');
            showPlanPicker = false;
        } finally {
            plansLoading = false;
        }
    }

    async function loadPlan(plan: PlanSummary) {
        try {
            await ranAPI.LoadPlan(plan.filename);
            showToast('Plan started', plan.name, 'success');
        } catch (error) {
            showToast('Failed to load plan', (error as Error).message, 'error');
        } finally {
            showPlanPicker = false;
        }
    }

	async function savePlan() {
		const name = planName.trim();
		if (!name) return;
		planSaving = true;
		try {
			const download = buildPlanDownload(
				await ranAPI.ExportPlan(planIncludeFailed, name, planDescription),
				name,
				new Date(),
				planFileName
			);
			saveFile(download.data, download.filename, download.mimeType);
			showToast(
				'Plan saved',
				planIncludeFailed
					? 'Successful and failed actions were exported in execution order.'
					: 'Successful actions were exported in execution order.',
				'success'
			);
			showSavePlan = false;
		} catch (error) {
			showToast('Failed to save plan', (error as Error).message, 'error');
		} finally {
			planSaving = false;
		}
	}

	async function openSavePlan() {
		const now = new Date();
		planName = defaultPlanName(now);
		planFileName = planFilename(planName);
		planFileNameOverridden = false;
		planDescription = defaultPlanDescription(now);
		planIncludeFailed = false;
		showSavePlan = true;
		await tick();
		planNameInput.focus();
		planNameInput.select();
	}

    function onMenuClick(event: { value: string }) {
        let { value } = event;

        switch (value) {
            case 'reset':
                campaignState.reset();
                timeline.clear();
                break;
            case 'load_plan':
                openPlanPicker();
                break;
			case 'save_plan':
				openSavePlan();
				break;
            case 'save_flow':
                campaignState
                    .GetFlow()
                    .then((flow) => {
                        const download = buildCampaignFlowDownload(flow);
                        saveFile(download.data, download.filename, download.mimeType);
                    })
                    .catch((error) => {
                        console.error('Error getting flow:', error);
                        showToast(
                            'Failed to save flow',
                            `Could not get flow: ${error.message}`,
                            'error'
                        );
                    });
                break;
            default:
                console.log('Unknown menu item:', value);
                break;
        }
    }
</script>

<Menu onSelect={onMenuClick}>
    <Menu.Trigger class="btn hover:preset-tonal text-xl">
		<Icon icon="game-icons:fishing-net" rotate={90} class="fill-token h-6 w-6 -scale-x-100" />
        Ran
    </Menu.Trigger>
    <Portal>
        <Menu.Positioner class="z-[110]">
            <Menu.Content class="z-[110]">
                <Menu.ItemGroup>
                    <Menu.ItemGroupLabel>Campaign</Menu.ItemGroupLabel>
                    <Menu.Item value="reset">
                        <Menu.ItemText>Reset</Menu.ItemText>
                    </Menu.Item>
                    <Menu.Item value="load_plan">
                        <Menu.ItemText>Load Plan</Menu.ItemText>
                    </Menu.Item>
					<Menu.Item value="save_plan">
						<Menu.ItemText>Save Plan</Menu.ItemText>
					</Menu.Item>
                </Menu.ItemGroup>
                <Menu.Separator />
                <Menu.ItemGroup>
                    <Menu.ItemGroupLabel>Flow</Menu.ItemGroupLabel>
                    <Menu.Item value="save_flow">
                        <!-- <Menu.ItemIndicator>💾</Menu.ItemIndicator> -->
                        <Menu.ItemText>Save Ran JSON</Menu.ItemText>
                    </Menu.Item>
                </Menu.ItemGroup>
            </Menu.Content>
        </Menu.Positioner>
    </Portal>
</Menu>

<Dialog open={showPlanPicker} onOpenChange={(e) => (showPlanPicker = e.open)}>
    <Portal>
        <Dialog.Backdrop class="fixed inset-0 z-50 bg-surface-50-950/50" />
        <Dialog.Positioner class="fixed inset-0 z-50 flex justify-center items-center">
            <Dialog.Content class="card bg-surface-100-900 p-4 space-y-2 shadow-xl max-w-2xl w-full">
                <PlanPickerModal
                    {plans}
                    loading={plansLoading}
                    onLoad={loadPlan}
                    onClose={() => (showPlanPicker = false)}
                />
            </Dialog.Content>
        </Dialog.Positioner>
    </Portal>
</Dialog>

<Dialog open={showSavePlan} onOpenChange={(e) => (showSavePlan = e.open)}>
	<Portal>
		<Dialog.Backdrop class="fixed inset-0 z-50 bg-surface-50-950/50" />
		<Dialog.Positioner class="fixed inset-0 z-50 flex items-center justify-center">
			<Dialog.Content class="card bg-surface-100-900 w-full max-w-sm space-y-4 p-5 shadow-xl">
				<form
					class="space-y-4"
					onsubmit={(event) => {
						event.preventDefault();
						savePlan();
					}}
				>
					<div>
						<h2 class="text-lg font-semibold">Save Plan</h2>
						<p class="text-surface-500 text-sm">Actions will be saved in execution order.</p>
					</div>
					<label class="block space-y-1">
						<span class="text-sm font-medium">Plan name</span>
						<input
							class="input w-full"
							type="text"
							bind:this={planNameInput}
							value={planName}
							oninput={(event) => {
								planName = event.currentTarget.value;
								if (!planFileNameOverridden) planFileName = planFilename(planName);
							}}
							placeholder="My assessment plan"
							required
						/>
					</label>
					<label class="block space-y-1">
						<span class="text-sm font-medium">Filename</span>
						<input
							class="input w-full"
							type="text"
							value={planFileName}
							oninput={(event) => {
								planFileName = event.currentTarget.value;
								planFileNameOverridden = true;
							}}
							required
						/>
					</label>
					<label class="block space-y-1">
						<span class="text-sm font-medium">Description</span>
						<textarea class="textarea w-full" rows="3" bind:value={planDescription}></textarea>
					</label>
					<label class="flex items-center gap-2 text-sm">
						<input class="checkbox" type="checkbox" bind:checked={planIncludeFailed} />
						<span>Include failed actions</span>
					</label>
					<div class="flex justify-end gap-2">
						<button class="btn preset-tonal" type="button" onclick={() => (showSavePlan = false)}>Cancel</button>
						<button
							class="btn preset-filled-primary-500"
							type="submit"
							disabled={!planName.trim() || !planFileName.trim() || planSaving}
						>
							{planSaving ? 'Saving…' : 'Save'}
						</button>
					</div>
				</form>
			</Dialog.Content>
		</Dialog.Positioner>
	</Portal>
</Dialog>
