# Operation Timeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Action Log Drawer with an Operation Timeline that shows TTP executions, entity discoveries, and credential finds in a single chronological bottom strip.

**Architecture:** The existing `ActionLogStore` + `ActionLogDrawer` are deleted and replaced by a new `TimelineStore` (Svelte 5 `$state` class) and `OperationTimeline` component. The Rust backend emits a new `entity-discovered` SSE event for each new entity in `FactsChanged`, with a `category` field derived from entity kind. `sendAction`/`onExecuteTTP` become async so they can use the `cmd_id` from the `ExecuteAction` POST response as the correlation key for the `ttp-executed` SSE event.

**Tech Stack:** Svelte 5 (`$state`, `$derived`), TypeScript, Vitest, Iconify, Skeleton UI, Tailwind, Rust/Axum SSE

**V1 note:** `"access-gained"` category is deferred — exec access changes on existing entities don't appear in `FactsChanged.new_entities`. The `EntityEntry` union keeps the variant for future use; the backend only emits `"discovery"` and `"credential"` initially.

---

## File Map

| File | Action |
|------|--------|
| `crates/app/src/lib.rs` | Modify — emit `entity-discovered` SSE per new entity in `FactsChanged` handler |
| `frontend/src/lib/stores/timelineStore.svelte.ts` | Create — replaces `actionLogStore.svelte.ts` |
| `frontend/src/lib/stores/timelineStore.svelte.test.ts` | Create — replaces `actionLogStore.svelte.test.ts` |
| `frontend/src/lib/stores/actionLogStore.svelte.ts` | Delete |
| `frontend/src/lib/stores/actionLogStore.svelte.test.ts` | Delete |
| `frontend/src/lib/components/OperationTimeline.svelte` | Create — replaces `ActionLogDrawer.svelte` |
| `frontend/src/lib/components/ActionLogDrawer.svelte` | Delete |
| `frontend/src/routes/+page.svelte` | Modify — update imports, make sendAction/onExecuteTTP async, add entity-discovered handler |
| `frontend/src/routes/+layout.svelte` | Modify — update toggle button store reference |
| `frontend/src/lib/components/app_menu.svelte` | Modify — replace `actionLog.clear()` with `timeline.clear()` on reset |

---

### Task 1: Backend — emit `entity-discovered` SSE events

**Files:**
- Modify: `crates/app/src/lib.rs` (around line 1177)

- [ ] **Step 1: Add `entity-discovered` emission inside the `FactsChanged` match arm**

Open `crates/app/src/lib.rs`. Find the `FactsChanged` match arm (currently around line 1177). After the existing `publish_sse_event("facts-changed", ...)` call, add a loop that emits one `entity-discovered` event per new entity:

```rust
Ok(CampaignEvent::FactsChanged {
    new_entities,
    new_relations,
    ..
}) => {
    api::publish_sse_event(
        "facts-changed",
        serde_json::json!({
            "type": "facts-changed",
            "data": {
                "newEntities": new_entities,
                "newRelations": new_relations,
            },
        })
        .to_string(),
    );

    for entity in &new_entities {
        let category = match entity.kind.as_str() {
            "Secret" | "K8sCredential" => "credential",
            _ => "discovery",
        };
        api::publish_sse_event(
            "entity-discovered",
            serde_json::json!({
                "type": "entity-discovered",
                "data": {
                    "entityId": entity.id.0,
                    "entityName": entity.name,
                    "entityKind": entity.kind,
                    "category": category,
                },
            })
            .to_string(),
        );
    }
}
```

- [ ] **Step 2: Build the backend to verify it compiles**

```bash
cargo build -p app 2>&1 | tail -20
```

Expected: no errors. If you see `entity.id.0` errors, `EntityId` may have changed — use `entity.id.to_string()` instead (it implements `Display`).

- [ ] **Step 3: Commit**

```bash
git add crates/app/src/lib.rs
git commit -m "feat(sse): emit entity-discovered event for each new entity in FactsChanged"
```

---

### Task 2: Timeline store — write tests first, then implement

**Files:**
- Create: `frontend/src/lib/stores/timelineStore.svelte.ts`
- Create: `frontend/src/lib/stores/timelineStore.svelte.test.ts`
- Delete: `frontend/src/lib/stores/actionLogStore.svelte.ts`
- Delete: `frontend/src/lib/stores/actionLogStore.svelte.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `frontend/src/lib/stores/timelineStore.svelte.test.ts`:

```ts
import { describe, it, expect, beforeEach } from 'vitest';
import { TimelineStore, type TtpActionEntry, type EntityEntry } from '$lib/stores/timelineStore.svelte';

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
        expect(store.entries).toHaveLength(0);
        expect(store.open).toBe(false);
        expect(store.pendingCount).toBe(0);
    });

    it('addTtpAction prepends entry with kind ttp-action (newest first)', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2' }));
        expect(store.entries).toHaveLength(2);
        expect(store.entries[0].id).toBe('cmd-2');
        expect(store.entries[0].kind).toBe('ttp-action');
        expect(store.entries[1].id).toBe('cmd-1');
    });

    it('addEntityEvent prepends discovery entries', () => {
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/web-app' }));
        expect(store.entries).toHaveLength(1);
        expect(store.entries[0].kind).toBe('discovery');
    });

    it('addEntityEvent prepends credential entries', () => {
        store.addEntityEvent(makeEntityEntry({ kind: 'credential', entityId: 'ns/default/secret/db-pass', id: 'ns/default/secret/db-pass', entityKind: 'Secret', entityName: 'db-pass' }));
        expect(store.entries[0].kind).toBe('credential');
    });

    it('addEntityEvent deduplicates by entityId', () => {
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/web-app' }));
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/web-app' }));
        expect(store.entries).toHaveLength(1);
    });

    it('addEntityEvent does not deduplicate different entityIds', () => {
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/web-app', id: 'ns/default/pod/web-app' }));
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/api', id: 'ns/default/pod/api' }));
        expect(store.entries).toHaveLength(2);
    });

    it('pendingCount counts only pending ttp-action entries', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1', status: 'pending' }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2', status: 'pending' }));
        store.addEntityEvent(makeEntityEntry());
        expect(store.pendingCount).toBe(2);
        store.resolveTtpAction('cmd-1', true);
        expect(store.pendingCount).toBe(1);
    });

    it('resolveTtpAction marks matching entry as success', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-abc', true);
        const entry = store.entries[0];
        expect(entry.kind).toBe('ttp-action');
        if (entry.kind === 'ttp-action') {
            expect(entry.status).toBe('success');
            expect(entry.failReason).toBeUndefined();
        }
    });

    it('resolveTtpAction marks entry as failed with reason', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-abc', false, 'permission denied');
        const entry = store.entries[0];
        if (entry.kind === 'ttp-action') {
            expect(entry.status).toBe('failed');
            expect(entry.failReason).toBe('permission denied');
        }
    });

    it('resolveTtpAction with unknown id is a no-op', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-unknown', true);
        const entry = store.entries[0];
        if (entry.kind === 'ttp-action') {
            expect(entry.status).toBe('pending');
        }
    });

    it('resolveTtpAction on already-resolved entry is a no-op', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'success' }));
        store.resolveTtpAction('cmd-abc', false, 'should not change');
        const entry = store.entries[0];
        if (entry.kind === 'ttp-action') {
            expect(entry.status).toBe('success');
        }
    });

    it('clear removes all entries', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addEntityEvent(makeEntityEntry());
        store.clear();
        expect(store.entries).toHaveLength(0);
    });

    it('mixed entries interleave by insertion order, newest first', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addEntityEvent(makeEntityEntry({ entityId: 'pod-a', id: 'pod-a' }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2' }));
        expect(store.entries[0].id).toBe('cmd-2');
        expect(store.entries[1].id).toBe('pod-a');
        expect(store.entries[2].id).toBe('cmd-1');
    });
});
```

- [ ] **Step 2: Run the tests to confirm they fail (module not found)**

```bash
cd frontend && npm run test -- --run timelineStore 2>&1 | tail -20
```

Expected: FAIL — `Cannot find module '$lib/stores/timelineStore.svelte'`

- [ ] **Step 3: Create the timeline store**

Create `frontend/src/lib/stores/timelineStore.svelte.ts`:

```ts
export type TtpActionEntry = {
    kind: 'ttp-action';
    id: string;
    ttpId: string;
    ttpName: string;
    targetId: string;
    targetName: string;
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
    timestamp: Date;
};

export type TimelineEntry = TtpActionEntry | EntityEntry;

export class TimelineStore {
    entries = $state<TimelineEntry[]>([]);
    open = $state(false);

    get pendingCount(): number {
        return this.entries.filter(
            (e): e is TtpActionEntry => e.kind === 'ttp-action' && e.status === 'pending'
        ).length;
    }

    addTtpAction(entry: Omit<TtpActionEntry, 'kind'>): void {
        this.entries = [{ kind: 'ttp-action', ...entry }, ...this.entries];
    }

    addEntityEvent(entry: EntityEntry): void {
        if (this.entries.some((e) => e.id === entry.entityId)) return;
        this.entries = [entry, ...this.entries];
    }

    resolveTtpAction(id: string, success: boolean, failReason?: string): void {
        const entry = this.entries.find(
            (e): e is TtpActionEntry => e.kind === 'ttp-action' && e.id === id && e.status === 'pending'
        );
        if (!entry) return;
        entry.status = success ? 'success' : 'failed';
        if (!success && failReason !== undefined) entry.failReason = failReason;
    }

    clear(): void {
        this.entries = [];
    }
}

export const timeline = new TimelineStore();
```

- [ ] **Step 4: Run the tests — they should pass**

```bash
cd frontend && npm run test -- --run timelineStore 2>&1 | tail -20
```

Expected: all tests PASS.

- [ ] **Step 5: Delete the old action log store files**

```bash
rm frontend/src/lib/stores/actionLogStore.svelte.ts
rm frontend/src/lib/stores/actionLogStore.svelte.test.ts
```

- [ ] **Step 6: Run the full test suite to confirm nothing else broke**

```bash
cd frontend && npm run test -- --run 2>&1 | tail -20
```

Expected: all remaining tests pass. If `+page.svelte` or layout tests fail due to missing `actionLogStore` import, that's expected — those are fixed in Task 4.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/lib/stores/timelineStore.svelte.ts \
        frontend/src/lib/stores/timelineStore.svelte.test.ts
git rm frontend/src/lib/stores/actionLogStore.svelte.ts \
       frontend/src/lib/stores/actionLogStore.svelte.test.ts
git commit -m "feat(store): add TimelineStore, replace ActionLogStore"
```

---

### Task 3: OperationTimeline component

**Files:**
- Create: `frontend/src/lib/components/OperationTimeline.svelte`
- Delete: `frontend/src/lib/components/ActionLogDrawer.svelte`

- [ ] **Step 1: Create the OperationTimeline component**

Create `frontend/src/lib/components/OperationTimeline.svelte`:

```svelte
<script lang="ts">
    import Icon from '@iconify/svelte';
    import type { TimelineEntry, TtpActionEntry, EntityEntry } from '$lib/stores/timelineStore.svelte';

    interface Props {
        entries: TimelineEntry[];
        onfocusentity: (targetId: string) => void;
    }

    let { entries, onfocusentity }: Props = $props();

    function formatTime(d: Date): string {
        return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
    }

    function entryLabel(entry: TimelineEntry): string {
        if (entry.kind === 'ttp-action') {
            return `${entry.ttpName} on ${entry.targetName}`;
        }
        if (entry.kind === 'credential') {
            if (entry.entityKind === 'Secret') return `Found secret ${entry.entityName}`;
            if (entry.entityKind === 'K8sCredential') return `Found credential ${entry.entityName}`;
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

    function entryTimestamp(entry: TimelineEntry): Date {
        return entry.timestamp;
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
                    <span class="text-surface-500 text-xs shrink-0 mt-0.5">{formatTime(entryTimestamp(entry))}</span>
                </div>
            {/each}
        {/if}
    </div>
</div>
```

- [ ] **Step 2: Delete the old ActionLogDrawer component**

```bash
rm frontend/src/lib/components/ActionLogDrawer.svelte
```

- [ ] **Step 3: Run the TypeScript checker to verify the component types are sound**

```bash
cd frontend && npm run check 2>&1 | tail -30
```

Expected: errors only about `actionLogStore` imports in `+page.svelte` and `+layout.svelte` (fixed in Task 4). No errors inside `OperationTimeline.svelte` itself.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/components/OperationTimeline.svelte
git rm frontend/src/lib/components/ActionLogDrawer.svelte
git commit -m "feat(ui): add OperationTimeline component, remove ActionLogDrawer"
```

---

### Task 4: Wire +page.svelte

**Files:**
- Modify: `frontend/src/routes/+page.svelte`

The key changes are:
1. Replace `actionLog` import with `timeline`
2. Replace `ActionLogDrawer` import with `OperationTimeline`
3. Make `sendAction` and `onExecuteTTP` async — add the TTP entry *after* `ExecuteAction` resolves using `cmd_id` from the response
4. Update the `ttp-executed` handler to use `data.CmdId`
5. Add an `entity-discovered` SSE handler
6. Replace `<ActionLogDrawer>` with `<OperationTimeline>` in the template

- [ ] **Step 1: Update imports at the top of +page.svelte**

Replace:
```ts
import { actionLog } from '$lib/stores/actionLogStore.svelte';
import ActionLogDrawer from '$lib/components/ActionLogDrawer.svelte';
```
With:
```ts
import { timeline } from '$lib/stores/timelineStore.svelte';
import OperationTimeline from '$lib/components/OperationTimeline.svelte';
```

- [ ] **Step 2: Replace sendAction with an async version**

Replace the existing `sendAction` function:

```ts
async function sendAction(ttp: TTP, args = {}) {
    selectedTTP = ttp;
    ttpArgContext = { ...args, ...activeGlobalConditions };
    if (ttp.params) {
        showParamModal = true;
    } else if ((ttp.procedures?.length ?? 0) > 1) {
        showParamModal = true;
    } else {
        const targetName = campaignState.getEntityById(selectedObjectId)?.name ?? selectedObjectId;
        try {
            const result = await ExecuteAction({
                actionId: ttp.id,
                targetId: selectedObjectId,
                procedureId: '',
                args: {}
            });
            const cmdId = (result as any)?.cmd_id ?? crypto.randomUUID();
            timeline.addTtpAction({
                id: cmdId,
                ttpId: ttp.id,
                ttpName: ttp.name,
                targetId: selectedObjectId,
                targetName,
                status: 'pending',
                timestamp: new Date()
            });
        } catch (err) {
            handleError(err);
        }
    }
}
```

- [ ] **Step 3: Replace onExecuteTTP with an async version**

Replace the existing `onExecuteTTP` function:

```ts
async function onExecuteTTP(ttpId: string, execSystemId: string, procedureId: string, args: Record<string, string>) {
    const ttp = campaignState.getTtpById(ttpId);
    const targetName = campaignState.getEntityById(selectedObjectId)?.name ?? selectedObjectId;

    closeModal();

    try {
        const result = await ExecuteAction({ actionId: ttpId, execSystemId, targetId: selectedObjectId, procedureId, args });
        const cmdId = (result as any)?.cmd_id ?? crypto.randomUUID();
        timeline.addTtpAction({
            id: cmdId,
            ttpId,
            ttpName: ttp?.name ?? ttpId,
            targetId: selectedObjectId,
            targetName,
            status: 'pending',
            timestamp: new Date()
        });
    } catch (err) {
        handleError(err);
    }
}
```

- [ ] **Step 4: Update the ttp-executed handler inside onMount**

Find the existing handler:
```ts
ranAPI.on('ttp-executed', (data) => {
    actionLog.resolveEntry(data.TTP?.id ?? '', data.Success, data.FailReason);
    ...
```

Replace the `actionLog.resolveEntry` line with:
```ts
ranAPI.on('ttp-executed', (data) => {
    timeline.resolveTtpAction(data.CmdId ?? data.ID ?? '', data.Success, data.FailReason);
    ...
```

Keep the `read-file` file viewer logic below it unchanged.

- [ ] **Step 5: Add entity-discovered handler inside onMount**

After the `ttp-executed` handler registration, add:

```ts
ranAPI.on('entity-discovered', (data) => {
    timeline.addEntityEvent({
        kind: data.category ?? 'discovery',
        id: data.entityId,
        entityId: data.entityId,
        entityName: data.entityName,
        entityKind: data.entityKind,
        timestamp: new Date()
    });
});
```

- [ ] **Step 6: Update campaign reset to clear timeline**

Find the line:
```ts
actionLog.clear();
```
Replace with:
```ts
timeline.clear();
```

If `actionLog.clear()` doesn't exist directly (it may be inside an effect watching `campaignState.campaignId`), search for `actionLog` in the file and replace every occurrence.

- [ ] **Step 7: Replace ActionLogDrawer in the template**

Find in the template:
```svelte
{#if actionLog.drawerOpen}
    <ActionLogDrawer
        entries={actionLog.entries}
        onfocusentity={(id) => { selectedObjectId = id; }}
    />
{/if}
```

Replace with:
```svelte
{#if timeline.open}
    <OperationTimeline
        entries={timeline.entries}
        onfocusentity={(id) => { selectedObjectId = id; }}
    />
{/if}
```

- [ ] **Step 8: Update the armory collapse button position reference**

Find in the template:
```svelte
style="left: {armoryCollapsed ? '0' : `${armoryWidth}px`}; bottom: {actionLog.drawerOpen ? 'calc(15rem + 0.5rem)' : '0.5rem'};"
```

Replace with:
```svelte
style="left: {armoryCollapsed ? '0' : `${armoryWidth}px`}; bottom: {timeline.open ? 'calc(15rem + 0.5rem)' : '0.5rem'};"
```

- [ ] **Step 9: Run the TypeScript checker**

```bash
cd frontend && npm run check 2>&1 | tail -30
```

Expected: errors only in `+layout.svelte` (fixed in Task 5). No errors in `+page.svelte`.

- [ ] **Step 10: Commit**

```bash
git add frontend/src/routes/+page.svelte
git commit -m "feat(page): wire OperationTimeline, async action dispatch with cmd_id correlation"
```

---

### Task 5: Update +layout.svelte toggle button

**Files:**
- Modify: `frontend/src/routes/+layout.svelte`

- [ ] **Step 1: Update the import**

Find:
```ts
import { actionLog } from '$lib/stores/actionLogStore.svelte';
```
Replace with:
```ts
import { timeline } from '$lib/stores/timelineStore.svelte';
```

- [ ] **Step 2: Replace all actionLog references with timeline**

The toggle button currently reads/writes `actionLog.drawerOpen` and shows `actionLog.pendingCount`. Replace every occurrence:

- `actionLog.drawerOpen` → `timeline.open`
- `actionLog.pendingCount` → `timeline.pendingCount`

Also update any `aria-label`, `title`, or visible text that says "action log" to "Operation Timeline":
- `aria-label="Toggle action log"` → `aria-label="Toggle operation timeline"`
- `title="Action log"` → `title="Operation timeline"`

- [ ] **Step 3: Update app_menu.svelte**

`app_menu.svelte` also calls `actionLog.clear()` on campaign reset. Open `frontend/src/lib/components/app_menu.svelte` and replace:

```ts
import { actionLog } from '$lib/stores/actionLogStore.svelte';
```
with:
```ts
import { timeline } from '$lib/stores/timelineStore.svelte';
```

And replace:
```ts
actionLog.clear();
```
with:
```ts
timeline.clear();
```

- [ ] **Step 4: Run the full TypeScript checker — should be clean**

```bash
cd frontend && npm run check 2>&1 | tail -30
```

Expected: no errors.

- [ ] **Step 5: Run the full test suite**

```bash
cd frontend && npm run test -- --run 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/routes/+layout.svelte frontend/src/lib/components/app_menu.svelte
git commit -m "feat(layout): update timeline toggle button and app menu to use TimelineStore"
```

---

### Task 6: Manual smoke test

**No code changes — verify the feature end-to-end.**

- [ ] **Step 1: Start the app**

```bash
# In one terminal: start backend
cargo run -p app

# In another terminal: start frontend dev server
cd frontend && npm run dev
```

Open `http://localhost:5173` in a browser.

- [ ] **Step 2: Verify timeline toggle**

Click the timeline toggle button in the app bar. The Operation Timeline strip should appear at the bottom of the graph column. Click again — it closes. Confirm "No events yet" placeholder is shown when empty.

- [ ] **Step 3: Verify TTP action entries**

Select a node in the graph. Execute a TTP from the armory. Confirm:
- A pending entry appears in the timeline immediately (spinner icon, "TTP Name on target-name")
- When execution completes, the spinner changes to a checkmark (success) or X (failed)
- If failed, the failure reason appears as a second line in error color

- [ ] **Step 4: Verify entity discovery entries**

Execute a TTP that discovers new entities (e.g., list namespaces, list pods). Confirm:
- New entries appear in the timeline with the magnify icon in primary-400 color
- Label reads "Discovered pod web-app" or "Discovered namespace default" etc.
- Duplicate entities are not added twice even if facts-changed fires multiple times

- [ ] **Step 5: Verify credential entries**

Execute a TTP that discovers secrets. Confirm:
- Secret entries appear with key icon in warning-500 color
- Label reads "Found secret <name>"

- [ ] **Step 6: Verify entity focus click**

Click the target entity name in a TTP action entry. Confirm the graph focuses on that node and the entity info panel opens.

- [ ] **Step 7: Verify campaign reset clears timeline**

Reset the campaign. Confirm the timeline clears.

- [ ] **Step 8: Final commit if any fixes were needed**

If you found and fixed any issues during smoke test, commit them now:
```bash
git add -p
git commit -m "fix(timeline): <describe what was fixed>"
```
