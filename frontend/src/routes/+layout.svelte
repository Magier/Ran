<script lang="ts">
    import { onMount } from 'svelte';
    import { AppBar, Toast, Switch } from '@skeletonlabs/skeleton-svelte';
    import {setContext} from 'svelte';
    import { page } from '$app/state';
    import IconMap from '~icons/game-icons/treasure-map';
    import IconSteps from '~icons/game-icons/footsteps';
    import IconSun from '~icons/material-symbols/light-mode';
    import IconMoon from '~icons/material-symbols/dark-mode';
    import { browser } from '$app/environment';
    import { toaster } from '$lib/components/toaster';
    import Icon from '@iconify/svelte';
    import '../app.css';
    import { setCampaignState } from '$lib/components/CampaignState.svelte';
	import AppMenu from '$lib/components/app_menu.svelte';
    let { children } = $props();

    setCampaignState();

    let isDark: boolean = $state(false)

    function toggle(details: { checked: boolean }) {
        isDark = details.checked;
        if (browser) {
            localStorage.setItem('theme', isDark ? 'dark' : 'light');
            updateBodyTheme();
        }
    }

    function updateBodyTheme() {
        if (browser) {
            document.documentElement.style.colorScheme = isDark ? 'dark' : 'light';
            document.documentElement.classList.toggle('dark', isDark);
        }
    }

    setContext('theme', { get isDark() { return isDark }, toggle });

    let mediaQuery: MediaQueryList | null = $state(null);
    onMount(() => {
        if (browser) {
            // Priority: localStorage > system preference
            const stored = localStorage.getItem('theme');

            if (stored) {
                isDark = stored === 'dark';
            } else {
                mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
                isDark = mediaQuery.matches;

                const handler = (event: MediaQueryListEvent) => {
                    // Only update from system preference if user hasn't set explicit preference
                    if (!localStorage.getItem('theme')) {
                        isDark = event.matches;
                        updateBodyTheme();
                    }
                };
                mediaQuery.addEventListener("change", handler);
            }

            updateBodyTheme();

            return () => {
                mediaQuery?.removeEventListener("change", handler);
            };
        }
    });

</script>

<AppBar class="top-0 p-0 border-b border-surface-200-800 h-[calc(var(--header-height))] flex ">
    <AppBar.Toolbar class="grid-cols-[auto_auto]">
        <AppBar.Lead>
            <!-- <ArrowLeft size={24} /> -->
             <AppMenu></AppMenu>
        </AppBar.Lead>
        <AppBar.Trail>
            <nav class="btn-group preset-outlined-surface-200-800 flex-col md:flex-row p-0">
                <a class="btn preset-filled-primary  hover:preset-tonal" class:selected={page.url.pathname === '/' || page.url.pathname === ''} href="/">
                    <IconMap class="inline-block text-xl" />
                    Graph
                </a>
                <a class="btn hover:preset-tonal" class:selected={page.url.pathname === '/flow'} href="/flow">
                    <IconSteps class="inline-block text-xl" />
                    Flow
                </a>
            </nav>
        <Switch checked={isDark} onCheckedChange={toggle} class="mx-2">
            <Switch.Control>
                <Switch.Thumb>
                    <Switch.Context>
                        {#snippet children(switch_)}
                            {#if switch_().checked}
                                <IconMoon class="inline-block size-3" />
                            {:else}
                                <IconSun class="inline-block size-3" />
                            {/if}
                        {/snippet}
                    </Switch.Context>
                </Switch.Thumb>
            </Switch.Control>
            <!-- <Switch.Label>
            </Switch.Label> -->
            <Switch.HiddenInput />
        </Switch>
        </AppBar.Trail>
    </AppBar.Toolbar>
</AppBar>

<Toast.Group {toaster}>
    {#snippet children(toast)}
        <Toast {toast}>
            <Toast.Message>
                <Toast.Title class="flex items-center gap-2">
                    {#if toast.meta?.spinner}
                        <Icon icon="svg-spinners:90-ring-with-bg" class="inline-block size-4" />
                    {/if}
                    {toast.title}
                </Toast.Title>
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
        background-color: var(--color-surface-200-800);
        color: var(--color-surface-contrast-200-800);
	}
</style>
