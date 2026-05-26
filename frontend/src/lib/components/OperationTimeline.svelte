<script lang="ts">
    import Icon from '@iconify/svelte';
    import type { TimelineEntry, EntityEntry } from '$lib/stores/timelineStore.svelte';

    interface Props {
        entries: TimelineEntry[];
        onfocusentity: (targetId: string) => void;
    }

    let { entries, onfocusentity }: Props = $props();

    function formatTime(d: Date): string {
        return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
    }

    function entryLabel(entry: EntityEntry): string {
        if (entry.kind === 'credential') {
            if (entry.entityKind === 'Secret') return `Found secret ${entry.entityName}`;
            return `Found credential ${entry.entityName}`;
        }
        if (entry.kind === 'access-gained') {
            return `Gained exec access to ${entry.entityName}`;
        }
        // discovery
        if (entry.entityKind === 'Pod') return `Discovered pod ${entry.entityName}`;
        if (entry.entityKind === 'Namespace') return `Discovered namespace ${entry.entityName}`;
        if (entry.entityKind === 'ServiceAccount') return `Discovered service account ${entry.entityName}`;
        return `Discovered ${entry.entityKind} ${entry.entityName}`;
    }


</script>

<div
    class="h-60 shrink-0 bg-surface-100-900 border-t border-surface-200-800 flex flex-col"
    role="region"
    aria-label="Operation timeline"
>
    <!-- Header -->
    <div class="flex items-center px-3 py-1.5 border-b border-surface-200-800 shrink-0">
        <span class="text-sm font-semibold">Operation Timeline</span>
        <span class="ml-2 text-xs text-surface-500">{entries.length} event{entries.length === 1 ? '' : 's'}</span>
    </div>

    <!-- Entry list -->
    <div class="overflow-y-auto flex-1 flex flex-col">
        {#if entries.length === 0}
            <div class="flex items-center justify-center h-full text-surface-500 text-sm">
                No events yet
            </div>
        {:else}
            {#each entries as entry (entry.id)}
                <div class="flex items-start gap-2 px-3 py-2 border-b border-surface-200-800 text-sm hover:bg-surface-200-800">
                    <!-- Status/category icon -->
                    <div class="mt-0.5 shrink-0">
                        {#if entry.kind === 'ttp-action'}
                            {#if entry.status === 'pending'}
                                <Icon icon="svg-spinners:90-ring-with-bg" class="size-4" aria-hidden="true" />
                            {:else if entry.status === 'success'}
                                <Icon icon="mdi:check-circle" class="size-4 text-success-500" aria-hidden="true" />
                            {:else}
                                <Icon icon="mdi:close-circle" class="size-4 text-error-500" aria-hidden="true" />
                            {/if}
                        {:else if entry.kind === 'credential'}
                            <Icon icon="mdi:key" class="size-4 text-warning-500" aria-hidden="true" />
                        {:else if entry.kind === 'access-gained'}
                            <Icon icon="mdi:shield-check" class="size-4 text-success-400" aria-hidden="true" />
                        {:else}
                            <Icon icon="mdi:magnify" class="size-4 text-primary-400" aria-hidden="true" />
                        {/if}
                    </div>

                    <!-- Content -->
                    <div class="flex-1 min-w-0">
                        <div class="flex items-center gap-1 flex-wrap leading-tight">
                            {#if entry.kind === 'ttp-action'}
                                <span class="font-medium">{entry.ttpName}</span>
                                <span class="text-surface-500">on</span>
                                <button
                                    type="button"
                                    class="text-primary-500 hover:underline truncate"
                                    title={entry.targetName}
                                    onclick={() => onfocusentity(entry.targetId)}
                                >
                                    {entry.targetName}
                                </button>
                            {:else}
                                <span class="font-medium">{entryLabel(entry)}</span>
                            {/if}
                        </div>
                        {#if entry.kind === 'ttp-action' && entry.status === 'failed' && entry.failReason}
                            <div class="text-error-500 text-xs mt-0.5 truncate" title={entry.failReason}>
                                {entry.failReason}
                            </div>
                        {/if}
                    </div>

                    <!-- Timestamp -->
                    <span class="text-surface-500 text-xs shrink-0 mt-0.5">{formatTime(entry.timestamp)}</span>
                </div>
            {/each}
        {/if}
    </div>
</div>
