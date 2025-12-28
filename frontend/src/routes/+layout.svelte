<script lang="ts">
	import { onMount } from 'svelte';
	import { AppBar, Navigation, Toast, Menu, Portal } from '@skeletonlabs/skeleton-svelte';
	import {setContext} from 'svelte';
	import { page } from '$app/state';
	import IconMap from '~icons/game-icons/treasure-map';
	import IconSteps from '~icons/game-icons/footsteps';
	import { browser } from '$app/environment';
	import { showToast, toaster } from '$lib/components/toaster';
	import '../app.css';
	import { getCampaignState, setCampaignState } from '$lib/components/CampaignState.svelte';
	import { saveFile } from '$lib/io';
	let { children } = $props();

	setCampaignState();
	const campaignState = getCampaignState();

	let isDark: boolean = $state(false)
	function toggle() { isDark = !isDark; }
	setContext('theme', { get isDark() { return isDark }, toggle });

	let mediaQuery: MediaQueryList | null = $state(null);
	onMount(() => {
		if (browser) {
			mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
			isDark = mediaQuery.matches;
			const handler = (event: MediaQueryListEvent) => {
				isDark = event.matches;
			};
			mediaQuery.addEventListener("change", handler);
			return () => {
				mediaQuery?.removeEventListener("change", handler);
			};
		}
	});

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

<AppBar>
	<AppBar.Toolbar class="grid-cols-[auto_auto]">
		<AppBar.Lead>
			<!-- <ArrowLeft size={24} /> -->
			<Menu onSelect={onMenuClick}>
				<Menu.Trigger class="btn preset-filled">Ran</Menu.Trigger>
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
		</AppBar.Lead>
		<AppBar.Trail>
			<!-- <nav> -->
				<a class="{page.url.pathname === '/' ? 'selected' : ''} pr-3" href="/"
					><IconMap class="inline-block text-xl" />Graph</a
				>
				<a class={page.url.pathname === '/flow' ? 'selected' : ''} href="/flow"
					><IconSteps class="inline-block text-xl" />Flow</a
				>
			<!-- </nav> -->
		</AppBar.Trail>
	</AppBar.Toolbar>
</AppBar>

<Toast.Group {toaster}>
	{#snippet children(toast)}
		<Toast {toast}>
			<Toast.Message>
				<Toast.Title>{toast.title}</Toast.Title>
				<Toast.Description>{toast.description}</Toast.Description>
			</Toast.Message>
			<Toast.CloseTrigger />
		</Toast>
	{/snippet}
</Toast.Group>
<main class="">
	{@render children()}
</main>

<style>
	* {
		color: #b6b6b6;
	}

	.selected {
		color: white;
	}
</style>
