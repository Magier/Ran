<script lang="ts">
    import Icon from '@iconify/svelte';
    import type { ActionLogEntry } from '$lib/stores/actionLogStore.svelte';

    interface Props {
        entries: ActionLogEntry[];
        onfocusentity: (targetId: string) => void;
    }

    let { entries, onfocusentity }: Props = $props();

    function formatTime(d: Date): string {
        return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    }
</script>

<div
    class="fixed bottom-0 left-0 right-0 z-40 bg-surface-100-900 border-t border-surface-200-800 flex flex-col"
    style="height: 240px;"
>
    <!-- Header -->
    <div class="flex items-center px-3 py-1.5 border-b border-surface-200-800 shrink-0">
        <span class="text-sm font-semibold">Action Log</span>
        <span class="ml-2 text-xs text-surface-500">{entries.length} action{entries.length === 1 ? '' : 's'}</span>
    </div>

    <!-- Entry list -->
    <div class="overflow-y-auto flex-1">
        {#if entries.length === 0}
            <div class="flex items-center justify-center h-full text-surface-500 text-sm">
                No actions yet
            </div>
        {:else}
            {#each entries as entry (entry.id)}
                <div class="flex items-start gap-2 px-3 py-2 border-b border-surface-200-800 text-sm hover:bg-surface-200-800">
                    <!-- Status icon -->
                    <div class="mt-0.5 shrink-0">
                        {#if entry.status === 'pending'}
                            <Icon icon="svg-spinners:90-ring-with-bg" class="size-4" />
                        {:else if entry.status === 'success'}
                            <Icon icon="mdi:check-circle" class="size-4 text-success-500" />
                        {:else}
                            <Icon icon="mdi:close-circle" class="size-4 text-error-500" />
                        {/if}
                    </div>

                    <!-- Content -->
                    <div class="flex-1 min-w-0">
                        <div class="flex items-center gap-1 flex-wrap leading-tight">
                            <span class="font-medium">{entry.ttpName}</span>
                            <span class="text-surface-500">on</span>
                            <button
                                class="text-primary-500 hover:underline truncate"
                                onclick={() => onfocusentity(entry.targetId)}
                            >
                                {entry.targetName}
                            </button>
                        </div>
                        {#if entry.status === 'failed' && entry.failReason}
                            <div class="text-error-500 text-xs mt-0.5 truncate" title={entry.failReason}>
                                {entry.failReason}
                            </div>
                        {/if}
                    </div>

                    <!-- Timestamp -->
                    <span class="text-surface-500 text-xs shrink-0 mt-0.5">{formatTime(entry.startedAt)}</span>
                </div>
            {/each}
        {/if}
    </div>
</div>
