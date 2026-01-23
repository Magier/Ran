<script lang="ts">
    import { showToast, toaster } from '$lib/components/toaster';
	import Icon from '@iconify/svelte';
    import { saveFile } from '$lib/io';
	import { Menu, Portal } from '@skeletonlabs/skeleton-svelte';
    import { getCampaignState } from '$lib/components/CampaignState.svelte';

    const campaignState = getCampaignState();

    function onMenuClick(event) {
        let {value} = event;

        switch (value) {
            case 'reset':
                 campaignState.reset()
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