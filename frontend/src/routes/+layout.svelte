<script lang="ts">
    import { onMount } from 'svelte';
    import { AppBar, Toast } from '@skeletonlabs/skeleton-svelte';
    import {setContext} from 'svelte';
    import { page } from '$app/state';
    import IconMap from '~icons/game-icons/treasure-map';
    import IconSteps from '~icons/game-icons/footsteps';
    import { browser } from '$app/environment';
    import { toaster } from '$lib/components/toaster';
    import '../app.css';
    import { setCampaignState } from '$lib/components/CampaignState.svelte';
	import AppMenu from '$lib/components/app_menu.svelte';
    let { children } = $props();

    setCampaignState();

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

</script>

<AppBar class="top-0 p-0 border-b border-surface-200-800 bg-surface-50-950/75 h-[calc(var(--header-height))] flex ">
    <AppBar.Toolbar class="grid-cols-[auto_auto]">
        <AppBar.Lead>
            <!-- <ArrowLeft size={24} /> -->
             <AppMenu></AppMenu>
        </AppBar.Lead>
        <AppBar.Trail>
            <nav class="btn-group preset-outlined-surface-200-800 flex-col md:flex-row p-0"> 
                <a class="btn hover:preset-tonal" class:selected={page.url.pathname === '/' || page.url.pathname === ''} href="/">
                    <IconMap class="inline-block text-xl" />
                    Graph
                </a>
                <a class="btn hover:preset-tonal" class:selected={page.url.pathname === '/flow'} href="/flow">
                    <IconSteps class="inline-block text-xl" />
                    Flow
                </a>
            </nav>
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
    
	.btn.selected {
        background-color: var(--color-primary-50-950);
        color: var(--color-primary-contrast-50-950);
	}
</style>
