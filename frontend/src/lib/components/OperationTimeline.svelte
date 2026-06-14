<script lang="ts">
    import Icon from '@iconify/svelte';
    import type { TopEntry, EntityEntry, ActionGroup } from '$lib/stores/timelineStore.svelte';

    interface Props {
        entries: TopEntry[];
        onfocusentity: (targetId: string) => void;
        ontogglegroup: (cmdId: string) => void;
    }

    let { entries, onfocusentity, ontogglegroup }: Props = $props();

    function formatTime(d: Date): string {
        return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
    }

    function entityPrefix(entry: EntityEntry): string {
        if (entry.kind === 'credential') {
            if (entry.entityKind === 'Secret') return 'Found secret';
            return 'Found credential';
        }
        if (entry.kind === 'access-gained') return 'Gained exec access to';
        if (entry.entityKind === 'Pod') return 'Discovered pod';
        if (entry.entityKind === 'Namespace') return 'Discovered namespace';
        if (entry.entityKind === 'ServiceAccount') return 'Discovered service account';
        return `Discovered ${entry.entityKind}`;
    }

    function effectCounts(group: ActionGroup) {
        const counts = { discovery: 0, credential: 0, access: 0 };
        for (const e of group.effects) {
            if (e.kind === 'discovery') counts.discovery++;
            else if (e.kind === 'credential') counts.credential++;
            else if (e.kind === 'access-gained') counts.access++;
        }
        return counts;
    }

    function entityIcon(kind: EntityEntry['kind']): string {
        if (kind === 'credential') return 'mdi:key';
        if (kind === 'access-gained') return 'mdi:shield-check';
        return 'mdi:magnify';
    }

    function entityIconClass(kind: EntityEntry['kind']): string {
        if (kind === 'credential') return 'size-4 text-warning-500';
        if (kind === 'access-gained') return 'size-4 text-success-400';
        return 'size-4 text-primary-400';
    }

    let totalEvents = $derived(
        entries.reduce((n, e) => {
            if (e.kind === 'action-group') return n + 1 + e.effects.length;
            return n + 1;
        }, 0)
    );
</script>

<div
    class="h-60 shrink-0 bg-surface-100-900 border-t border-surface-200-800 flex flex-col"
    role="region"
    aria-label="Operation timeline"
>
    <!-- Header -->
    <div class="flex items-center px-3 py-1.5 border-b border-surface-200-800 shrink-0">
        <span class="text-sm font-semibold">Operation Timeline</span>
        <span class="ml-2 text-xs text-surface-500">{totalEvents} event{totalEvents === 1 ? '' : 's'}</span>
    </div>

    <!-- Entry list -->
    <div class="overflow-y-auto flex-1 flex flex-col">
        {#if entries.length === 0}
            <div class="flex items-center justify-center h-full text-surface-500 text-sm">
                No events yet
            </div>
        {:else}
            {#each entries as entry (entry.kind === 'action-group' ? entry.action.id : entry.id)}
                {#if entry.kind === 'action-group'}
                    {@const counts = effectCounts(entry)}
                    <!-- Action group header row -->
                    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
                    <div
                        class="flex items-start gap-2 px-3 py-2 border-b border-surface-200-800 text-sm hover:bg-surface-200-800 cursor-pointer select-none"
                        onclick={() => ontogglegroup(entry.action.id)}
                        aria-expanded={!entry.collapsed}
                    >
                        <!-- Chevron (far left) -->
                        <div class="mt-0.5 shrink-0 text-surface-500">
                            {#if entry.effects.length > 0}
                                <Icon
                                    icon={entry.collapsed ? 'mdi:chevron-right' : 'mdi:chevron-down'}
                                    class="size-4"
                                    aria-hidden="true"
                                />
                            {:else}
                                <div class="size-4"></div>
                            {/if}
                        </div>

                        <!-- Status icon -->
                        <div class="mt-0.5 shrink-0">
                            {#if entry.action.status === 'pending'}
                                <Icon icon="svg-spinners:90-ring-with-bg" class="size-4" aria-hidden="true" />
                            {:else if entry.action.status === 'success'}
                                <Icon icon="mdi:check-circle" class="size-4 text-success-500" aria-hidden="true" />
                            {:else}
                                <Icon icon="mdi:close-circle" class="size-4 text-error-500" aria-hidden="true" />
                            {/if}
                        </div>

                        <!-- Label: ttpName on target [via execSystem] -->
                        <div class="flex-1 min-w-0">
                            <div class="flex items-center gap-1 flex-wrap leading-tight">
                                <span class="font-medium">{entry.action.ttpName}</span>
                                <span class="text-surface-500">on</span>
                                <button
                                    type="button"
                                    class="text-primary-500 hover:underline truncate"
                                    title={entry.action.targetName}
                                    onclick={(e) => { e.stopPropagation(); onfocusentity(entry.action.targetId); }}
                                >
                                    {entry.action.targetName}
                                </button>
                                {#if entry.action.execSystemName}
                                    <span class="text-surface-500 text-xs">via</span>
                                    <span class="text-surface-500 text-xs truncate" title={entry.action.execSystemName}>
                                        {entry.action.execSystemName}
                                    </span>
                                {/if}
                            </div>
                            {#if entry.action.status === 'failed' && entry.action.failReason}
                                <div class="text-error-500 text-xs mt-0.5 truncate" title={entry.action.failReason}>
                                    {entry.action.failReason}
                                </div>
                            {/if}
                        </div>

                        <!-- Effect chips -->
                        <div class="flex items-center gap-1 shrink-0 mt-0.5">
                            {#if counts.discovery > 0}
                                <Icon icon="mdi:magnify" class="size-3.5 text-primary-400" aria-hidden="true" />
                                <span class="text-xs text-surface-400">{counts.discovery}</span>
                            {/if}
                            {#if counts.credential > 0}
                                <Icon icon="mdi:key" class="size-3.5 text-warning-500" aria-hidden="true" />
                                <span class="text-xs text-surface-400">{counts.credential}</span>
                            {/if}
                            {#if counts.access > 0}
                                <Icon icon="mdi:shield-check" class="size-3.5 text-success-400" aria-hidden="true" />
                                <span class="text-xs text-surface-400">{counts.access}</span>
                            {/if}
                            {#if entry.score != null}
                                <span class="text-xs text-surface-400 ml-1">★ {entry.score.toFixed(1)}</span>
                            {/if}
                        </div>

                        <!-- Timestamp -->
                        <span class="text-surface-500 text-xs shrink-0 mt-0.5">{formatTime(entry.action.timestamp)}</span>
                    </div>

                    <!-- Expanded child effect rows -->
                    {#if !entry.collapsed}
                        {#each entry.effects as effect (effect.id)}
                            <div class="flex items-start gap-2 pl-8 pr-3 py-1.5 border-b border-surface-200-800 text-sm hover:bg-surface-200-800 border-l-2 border-l-surface-300-700 ml-3">
                                <div class="mt-0.5 shrink-0">
                                    <Icon icon={entityIcon(effect.kind)} class={entityIconClass(effect.kind)} aria-hidden="true" />
                                </div>
                                <div class="flex-1 min-w-0">
                                    <span class="font-medium">{entityPrefix(effect)}</span> <button
                                        type="button"
                                        class="font-medium text-left hover:underline text-primary-500"
                                        onclick={() => onfocusentity(effect.entityId)}
                                    >{effect.entityName}</button>
                                </div>
                                <span class="text-surface-500 text-xs shrink-0 mt-0.5">{formatTime(effect.timestamp)}</span>
                            </div>
                        {/each}
                    {/if}

                {:else}
                    <!-- Standalone entity row (no parent action) -->
                    <div class="flex items-start gap-2 px-3 py-2 border-b border-surface-200-800 text-sm hover:bg-surface-200-800">
                        <div class="mt-0.5 shrink-0">
                            <Icon icon={entityIcon(entry.kind)} class={entityIconClass(entry.kind)} aria-hidden="true" />
                        </div>
                        <div class="flex-1 min-w-0">
                            <span class="font-medium">{entityPrefix(entry)}</span> <button
                                type="button"
                                class="font-medium text-left hover:underline text-primary-500"
                                onclick={() => onfocusentity(entry.entityId)}
                            >{entry.entityName}</button>
                        </div>
                        <span class="text-surface-500 text-xs shrink-0 mt-0.5">{formatTime(entry.timestamp)}</span>
                    </div>
                {/if}
            {/each}
        {/if}
    </div>
</div>
