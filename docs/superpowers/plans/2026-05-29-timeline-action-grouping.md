# Operation Timeline Action Grouping — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure the Operation Timeline so TTP action rows are collapsible and group their effect events (discovery, credential, access-gained) as children, with typed effect-count chips shown at a glance on the collapsed header row.

**Architecture:** `TimelineStore` owns all grouping logic via a private `Map<string, ActionGroup>` index; the component stays dumb and renders `TopEntry[]`. Entity events arriving with a `cmdId` field are appended to their parent group's `effects` array in-place; events without `cmdId` render as standalone rows unchanged.

**Tech Stack:** Svelte 5 (runes, `$state`, `$derived`), TypeScript, Skeleton UI / Tailwind, Vitest + @testing-library/svelte, Iconify (`mdi:*` icons)

**Spec:** `docs/superpowers/specs/2026-05-29-timeline-action-grouping.md`

---

## File Map

| File | Change |
|---|---|
| `frontend/src/lib/stores/timelineStore.svelte.ts` | Rewrite types + store class |
| `frontend/src/lib/stores/timelineStore.svelte.test.ts` | Rewrite tests against new API |
| `frontend/src/lib/components/OperationTimeline.svelte` | Update props + rendering |
| `frontend/src/routes/+page.svelte` | Update two event handlers, prop |

---

## Task 1: Rewrite store types and class

**Files:**
- Modify: `frontend/src/lib/stores/timelineStore.svelte.ts` (full rewrite)

- [ ] **Step 1: Write the new failing tests**

Replace the entire contents of `frontend/src/lib/stores/timelineStore.svelte.test.ts` with:

```ts
import { describe, it, expect, beforeEach } from 'vitest';
import {
    TimelineStore,
    type TtpActionEntry,
    type EntityEntry,
    type ActionGroup,
    type TopEntry
} from '$lib/stores/timelineStore.svelte';

function makeTtpEntry(overrides: Partial<Omit<TtpActionEntry, 'kind'>> = {}): Omit<TtpActionEntry, 'kind'> {
    return {
        id: 'cmd-abc',
        ttpId: 'list-env',
        ttpName: 'List Environment Variables',
        targetId: 'pod-1',
        targetName: 'my-pod',
        status: 'pending',
        timestamp: new Date('2026-05-25T10:00:00Z'),
        ...overrides
    };
}

function makeEntityEntry(overrides: Partial<EntityEntry> = {}): EntityEntry {
    return {
        kind: 'discovery',
        id: 'ns/default/pod/web-app',
        entityId: 'ns/default/pod/web-app',
        entityName: 'web-app',
        entityKind: 'Pod',
        timestamp: new Date('2026-05-25T10:01:00Z'),
        ...overrides
    };
}

describe('TimelineStore', () => {
    let store: TimelineStore;

    beforeEach(() => {
        store = new TimelineStore();
    });

    it('starts empty with timeline closed', () => {
        expect(store.topEntries).toHaveLength(0);
        expect(store.open).toBe(false);
        expect(store.pendingCount).toBe(0);
    });

    // addTtpAction
    it('addTtpAction creates an ActionGroup prepended to topEntries', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        expect(store.topEntries).toHaveLength(1);
        const entry = store.topEntries[0];
        expect(entry.kind).toBe('action-group');
        if (entry.kind === 'action-group') {
            expect(entry.action.id).toBe('cmd-1');
            expect(entry.effects).toHaveLength(0);
            expect(entry.collapsed).toBe(true);
        }
    });

    it('addTtpAction prepends newest first', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2' }));
        expect(store.topEntries[0].kind).toBe('action-group');
        if (store.topEntries[0].kind === 'action-group') {
            expect(store.topEntries[0].action.id).toBe('cmd-2');
        }
    });

    // addEntityEvent — grouping
    it('addEntityEvent with matching cmdId appends to group effects', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: 'cmd-abc' }));
        expect(store.topEntries).toHaveLength(1); // still one top-level entry
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.effects).toHaveLength(1);
            expect(entry.effects[0].entityName).toBe('web-app');
        }
    });

    it('addEntityEvent without cmdId prepends as standalone', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: undefined }));
        expect(store.topEntries).toHaveLength(2);
        expect(store.topEntries[0].kind).toBe('discovery');
    });

    it('addEntityEvent with unmatched cmdId prepends as standalone', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.addEntityEvent(makeEntityEntry({ id: 'x', entityId: 'x', cmdId: 'cmd-unknown' }));
        expect(store.topEntries).toHaveLength(2);
        expect(store.topEntries[0].kind).toBe('discovery');
    });

    // deduplication
    it('addEntityEvent deduplicates standalone entries by id', () => {
        store.addEntityEvent(makeEntityEntry({ id: 'pod-a', entityId: 'pod-a', cmdId: undefined }));
        store.addEntityEvent(makeEntityEntry({ id: 'pod-a', entityId: 'pod-a', cmdId: undefined }));
        expect(store.topEntries).toHaveLength(1);
    });

    it('addEntityEvent deduplicates group effects by id', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: 'cmd-abc' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: 'cmd-abc' })); // same id
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.effects).toHaveLength(1);
        }
    });

    it('addEntityEvent does not suppress entity when a group has the same id as the entity', () => {
        store.addTtpAction(makeTtpEntry({ id: 'ns/default/pod/web-app' }));
        store.addEntityEvent(makeEntityEntry({ id: 'ns/default/pod/web-app', entityId: 'ns/default/pod/web-app', cmdId: undefined }));
        expect(store.topEntries).toHaveLength(2);
        expect(store.topEntries[0].kind).toBe('discovery');
    });

    // pendingCount
    it('pendingCount counts only pending action groups', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1', status: 'pending' }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2', status: 'pending' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: undefined }));
        expect(store.pendingCount).toBe(2);
        store.resolveTtpAction('cmd-1', true);
        expect(store.pendingCount).toBe(1);
    });

    // resolveTtpAction
    it('resolveTtpAction marks matching group action as success', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-abc', true);
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.action.status).toBe('success');
            expect(entry.action.failReason).toBeUndefined();
        }
    });

    it('resolveTtpAction marks entry as failed with reason', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-abc', false, 'permission denied');
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.action.status).toBe('failed');
            expect(entry.action.failReason).toBe('permission denied');
        }
    });

    it('resolveTtpAction with unknown id is a no-op', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-unknown', true);
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.action.status).toBe('pending');
        }
    });

    it('resolveTtpAction on already-resolved entry is a no-op', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'success' }));
        store.resolveTtpAction('cmd-abc', false, 'should not change');
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.action.status).toBe('success');
        }
    });

    // toggleGroup
    it('toggleGroup flips collapsed from true to false', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.toggleGroup('cmd-abc');
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.collapsed).toBe(false);
        }
    });

    it('toggleGroup flips collapsed from false to true', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.toggleGroup('cmd-abc'); // false
        store.toggleGroup('cmd-abc'); // true
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.collapsed).toBe(true);
        }
    });

    it('toggleGroup with unknown id is a no-op', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        expect(() => store.toggleGroup('cmd-unknown')).not.toThrow();
    });

    // clear
    it('clear removes all entries', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: undefined }));
        store.clear();
        expect(store.topEntries).toHaveLength(0);
        // index is also cleared: new entity with old cmdId goes to standalone
        store.addEntityEvent(makeEntityEntry({ cmdId: 'cmd-1' }));
        expect(store.topEntries).toHaveLength(1);
        expect(store.topEntries[0].kind).toBe('discovery');
    });

    // mixed ordering
    it('mixed entries interleave by insertion order, newest first', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addEntityEvent(makeEntityEntry({ id: 'pod-a', entityId: 'pod-a', cmdId: undefined }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2' }));
        const ids = store.topEntries.map((e) =>
            e.kind === 'action-group' ? e.action.id : e.id
        );
        expect(ids).toEqual(['cmd-2', 'pod-a', 'cmd-1']);
    });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /Users/me/Dev/Ran/frontend && pnpm test 2>&1 | tail -30
```

Expected: multiple failures — `topEntries`, `ActionGroup`, `toggleGroup` not defined yet.

- [ ] **Step 3: Rewrite the store**

Replace the entire contents of `frontend/src/lib/stores/timelineStore.svelte.ts` with:

```ts
export type TtpActionEntry = {
    kind: 'ttp-action';
    id: string;
    ttpId: string;
    ttpName: string;
    targetId: string;
    targetName: string;
    execSystemId?: string;
    execSystemName?: string;
    status: 'pending' | 'success' | 'failed';
    failReason?: string;
    timestamp: Date;
};

export type EntityEntry = {
    kind: 'discovery' | 'credential' | 'access-gained';
    id: string;
    entityId: string;
    entityName: string;
    entityKind: string;
    cmdId?: string;
    timestamp: Date;
};

export type ActionGroup = {
    kind: 'action-group';
    action: TtpActionEntry;
    effects: EntityEntry[];
    collapsed: boolean;
    score?: number;
};

export type TopEntry = ActionGroup | EntityEntry;

export class TimelineStore {
    topEntries = $state<TopEntry[]>([]);
    open = $state(false);

    private index = new Map<string, ActionGroup>();
    private seenEntityIds = new Set<string>();

    get pendingCount(): number {
        return this.topEntries.filter(
            (e): e is ActionGroup => e.kind === 'action-group' && e.action.status === 'pending'
        ).length;
    }

    addTtpAction(entry: Omit<TtpActionEntry, 'kind'>): void {
        const group: ActionGroup = {
            kind: 'action-group',
            action: { kind: 'ttp-action', ...entry },
            effects: [],
            collapsed: true
        };
        this.index.set(entry.id, group);
        this.topEntries = [group, ...this.topEntries];
    }

    addEntityEvent(entry: EntityEntry): void {
        if (this.seenEntityIds.has(entry.id)) return;
        this.seenEntityIds.add(entry.id);

        if (entry.cmdId) {
            const group = this.index.get(entry.cmdId);
            if (group) {
                group.effects.push(entry);
                return;
            }
        }

        this.topEntries = [entry, ...this.topEntries];
    }

    resolveTtpAction(id: string, success: boolean, failReason?: string): void {
        const group = this.index.get(id);
        if (!group || group.action.status !== 'pending') return;
        group.action.status = success ? 'success' : 'failed';
        if (!success && failReason !== undefined) group.action.failReason = failReason;
    }

    toggleGroup(cmdId: string): void {
        const group = this.index.get(cmdId);
        if (!group) return;
        group.collapsed = !group.collapsed;
    }

    clear(): void {
        this.topEntries = [];
        this.index.clear();
        this.seenEntityIds.clear();
    }
}

export const timeline = new TimelineStore();
```

- [ ] **Step 4: Run tests — expect pass**

```bash
cd /Users/me/Dev/Ran/frontend && pnpm test 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
cd /Users/me/Dev/Ran/frontend && git add src/lib/stores/timelineStore.svelte.ts src/lib/stores/timelineStore.svelte.test.ts && git commit -m "feat(timeline): restructure store with ActionGroup, effect grouping, toggleGroup"
```

---

## Task 2: Update OperationTimeline component

**Files:**
- Modify: `frontend/src/lib/components/OperationTimeline.svelte` (full rewrite)

- [ ] **Step 1: Rewrite the component**

Replace the entire contents of `frontend/src/lib/components/OperationTimeline.svelte` with:

```svelte
<script lang="ts">
    import Icon from '@iconify/svelte';
    import type { TopEntry, EntityEntry, ActionGroup } from '$lib/stores/timelineStore.svelte';
    import { timeline } from '$lib/stores/timelineStore.svelte';

    interface Props {
        entries: TopEntry[];
        onfocusentity: (targetId: string) => void;
    }

    let { entries, onfocusentity }: Props = $props();

    function formatTime(d: Date): string {
        return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
    }

    function entityLabel(entry: EntityEntry): string {
        if (entry.kind === 'credential') {
            if (entry.entityKind === 'Secret') return `Found secret ${entry.entityName}`;
            return `Found credential ${entry.entityName}`;
        }
        if (entry.kind === 'access-gained') {
            return `Gained exec access to ${entry.entityName}`;
        }
        if (entry.entityKind === 'Pod') return `Discovered pod ${entry.entityName}`;
        if (entry.entityKind === 'Namespace') return `Discovered namespace ${entry.entityName}`;
        if (entry.entityKind === 'ServiceAccount') return `Discovered service account ${entry.entityName}`;
        return `Discovered ${entry.entityKind} ${entry.entityName}`;
    }

    function effectCounts(group: ActionGroup) {
        return {
            discovery: group.effects.filter(e => e.kind === 'discovery').length,
            credential: group.effects.filter(e => e.kind === 'credential').length,
            access: group.effects.filter(e => e.kind === 'access-gained').length,
        };
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
                    <div class="flex items-start gap-2 px-3 py-2 border-b border-surface-200-800 text-sm hover:bg-surface-200-800">
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
                                    onclick={() => onfocusentity(entry.action.targetId)}
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

                        <!-- Chevron toggle -->
                        {#if entry.effects.length > 0}
                            <button
                                type="button"
                                class="mt-0.5 shrink-0 text-surface-500 hover:text-surface-300"
                                onclick={() => timeline.toggleGroup(entry.action.id)}
                                aria-label={entry.collapsed ? 'Expand effects' : 'Collapse effects'}
                            >
                                <Icon
                                    icon={entry.collapsed ? 'mdi:chevron-right' : 'mdi:chevron-down'}
                                    class="size-4"
                                    aria-hidden="true"
                                />
                            </button>
                        {:else}
                            <div class="size-4 shrink-0 mt-0.5"></div>
                        {/if}

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
                                    <span class="font-medium">{entityLabel(effect)}</span>
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
                            <span class="font-medium">{entityLabel(entry)}</span>
                        </div>
                        <span class="text-surface-500 text-xs shrink-0 mt-0.5">{formatTime(entry.timestamp)}</span>
                    </div>
                {/if}
            {/each}
        {/if}
    </div>
</div>
```

- [ ] **Step 2: Type-check the component**

```bash
cd /Users/me/Dev/Ran/frontend && pnpm check 2>&1 | tail -30
```

Expected: no errors. Fix any type errors before continuing.

- [ ] **Step 3: Commit**

```bash
cd /Users/me/Dev/Ran/frontend && git add src/lib/components/OperationTimeline.svelte && git commit -m "feat(timeline): update OperationTimeline for grouped ActionGroup rendering"
```

---

## Task 3: Update page event handlers and component prop

**Files:**
- Modify: `frontend/src/routes/+page.svelte`

The two call sites that feed the timeline need updating. No new imports are needed — `timeline` is already imported.

- [ ] **Step 1: Update `entity-discovered` handler to forward `cmdId`**

In `+page.svelte`, locate the `ranAPI.on('entity-discovered', ...)` block (around line 343) and update it:

```ts
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
```

- [ ] **Step 2: Update `onExecuteTTP` to capture exec system**

Locate the `onExecuteTTP` function (around line 368) and update the `timeline.addTtpAction` call inside it:

```ts
async function onExecuteTTP(ttpId: string, execSystemId: string, procedureId: string, args: Record<string, string>) {
    const ttp = campaignState.getTtpById(ttpId);
    const targetName = campaignState.getEntityById(selectedObjectId)?.name ?? selectedObjectId;

    closeModal();

    try {
        const result = await ExecuteAction({ actionId: ttpId, execSystemId, targetId: selectedObjectId, procedureId, args });
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
```

- [ ] **Step 3: Update the OperationTimeline prop**

Locate the `<OperationTimeline>` usage in the template (around line 477) and update the `entries` prop:

```svelte
{#if timeline.open}
    <OperationTimeline
        entries={timeline.topEntries}
        onfocusentity={(id) => { selectedObjectId = id; }}
    />
{/if}
```

- [ ] **Step 4: Type-check the full app**

```bash
cd /Users/me/Dev/Ran/frontend && pnpm check 2>&1 | tail -30
```

Expected: no errors.

- [ ] **Step 5: Run tests one final time**

```bash
cd /Users/me/Dev/Ran/frontend && pnpm test 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
cd /Users/me/Dev/Ran/frontend && git add src/routes/+page.svelte && git commit -m "feat(timeline): wire cmdId and execSystem into timeline from page handlers"
```
