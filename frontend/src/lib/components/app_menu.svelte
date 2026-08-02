<script lang="ts">
    import { showToast, toaster } from '$lib/components/toaster';
	import Icon from '@iconify/svelte';
    import { saveFile } from '$lib/io';
	import { Dialog, Menu, Portal } from '@skeletonlabs/skeleton-svelte';
    import { getCampaignState } from '$lib/components/CampaignState.svelte';
    import { timeline } from '$lib/stores/timelineStore.svelte';
    import { getRanAPI } from '$lib/ran_api';
    import type { PlanSummary } from '$lib/api';
    import PlanPickerModal from '$lib/modals/PlanPickerModal.svelte';

    const campaignState = getCampaignState();
    const ranAPI = getRanAPI();

    let showPlanPicker = $state(false);
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

    function onMenuClick(event: { value: string }) {
        let {value} = event;

        switch (value) {
            case 'reset':
                 campaignState.reset();
                 timeline.clear();
                break;
            case 'load_plan':
                openPlanPicker();
                break;
            case 'save_flow':
                campaignState.ExportAttackFlow().then((flow) => {
                    const fileName = `campaign_${new Date().toISOString()}.json`;
                    const data = JSON.stringify(flow, null, 2);
                    saveFile(data, fileName, 'application/json');
                }).catch((error) => {
                    console.error('Error getting flow:', error);
                    showToast('Failed to save flow', `Could not get flow: ${error.message}`, 'error'
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
        <Menu.Positioner>
            <Menu.Content>
                <Menu.ItemGroup>
                    <Menu.ItemGroupLabel>Campaign</Menu.ItemGroupLabel>
                    <Menu.Item value="reset">
                        <Menu.ItemText>Reset</Menu.ItemText>
                    </Menu.Item>
                    <Menu.Item value="load_plan">
                        <Menu.ItemText>Load Plan</Menu.ItemText>
                    </Menu.Item>
                </Menu.ItemGroup>
                <Menu.Separator />
                <Menu.ItemGroup>
                    <Menu.ItemGroupLabel>Flow</Menu.ItemGroupLabel>
                    <Menu.Item value="save_flow">
                        <!-- <Menu.ItemIndicator>💾</Menu.ItemIndicator> -->
                        <Menu.ItemText>Save</Menu.ItemText>
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