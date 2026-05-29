# Action Log Drawer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace ephemeral action spinner/success/error toasts with a persistent bottom drawer that lists all executed actions in chronological order with status, target entity (clickable to focus graph), and failure details.

**Architecture:** A new singleton Svelte 5 `$state` store (`actionLogStore.svelte.ts`) holds entries and `drawerOpen`. `+page.svelte` calls `addEntry` at dispatch and `resolveEntry` on `ttp-executed` instead of creating toasts. A fixed-position `ActionLogDrawer.svelte` component renders the list. The app bar button in `+layout.svelte` reads and toggles `drawerOpen` from the store.

**Tech Stack:** SvelteKit 5, Svelte 5 runes (`$state`), Tailwind CSS, `@iconify/svelte`, Skeleton UI v3. Tests via Vitest + `@testing-library/svelte` (jsdom environment).

---

### Key facts before you start

- **Correlation key:** `ExecuteActionCmd` has no `cmd_id` field. The `ttp-executed` SSE event carries `data.TTP?.id` (the TTP definition id, e.g. `"list-env-vars"`). Use `ttpId` as the match key in `resolveEntry`. This matches the existing `ToastMapping` pattern.
- **SSE event fields (PascalCase):** `data.TTP.id`, `data.TTP.name`, `data.Success` (boolean), `data.FailReason` (string), `data.Args` (object). These are the actual runtime field names, different from the snake_case `ExecutionRecordEntry` REST type.
- **Test file naming:** Must use `.svelte.test.ts` extension to run in the jsdom + svelteTesting environment.
- **Svelte 5 runes in classes:** `$state` and `$derived` work as class field initialisers in `.svelte.ts` files. Mutating a nested object property of a `$state` array (e.g. `entry.status = 'failed'`) triggers reactivity.
- **Run tests from frontend dir:** `cd frontend && pnpm test`

---

### File map

| Path | Action |
|------|--------|
| `frontend/src/lib/stores/actionLogStore.svelte.ts` | Create — store (entries, drawerOpen, addEntry, resolveEntry, clear) |
| `frontend/src/lib/stores/actionLogStore.svelte.test.ts` | Create — unit tests for the store |
| `frontend/src/lib/components/ActionLogDrawer.svelte` | Create — bottom drawer UI |
| `frontend/src/routes/+page.svelte` | Modify — remove ToastMapping + action toasts; wire store; mount drawer |
| `frontend/src/routes/+layout.svelte` | Modify — add app bar toggle button |

---

### Task 1: Create actionLogStore.svelte.ts with tests

**Files:**
- Create: `frontend/src/lib/stores/actionLogStore.svelte.ts`
- Create: `frontend/src/lib/stores/actionLogStore.svelte.test.ts`

- [ ] **Step 1.1: Write the failing tests**

Create `frontend/src/lib/stores/actionLogStore.svelte.test.ts`:

```ts
import { describe, it, expect, beforeEach } from 'vitest';
import { ActionLogStore, type ActionLogEntry } from '$lib/stores/actionLogStore.svelte';

function makeEntry(overrides: Partial<ActionLogEntry> = {}): ActionLogEntry {
    return {
        id: 'test-id',
        ttpId: 'list-env',
        ttpName: 'List Environment Variables',
        targetId: 'pod-1',
        targetName: 'my-pod',
        status: 'pending',
        startedAt: new Date('2026-05-22T10:00:00Z'),
        ...overrides
    };
}

describe('ActionLogStore', () => {
    let store: ActionLogStore;

    beforeEach(() => {
        store = new ActionLogStore();
    });

    it('starts empty with drawer closed', () => {
        expect(store.entries).toHaveLength(0);
        expect(store.drawerOpen).toBe(false);
        expect(store.pendingCount).toBe(0);
    });

    it('addEntry prepends entries (newest first)', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'action-a' }));
        store.addEntry(makeEntry({ id: 'b', ttpId: 'action-b' }));
        expect(store.entries).toHaveLength(2);
        expect(store.entries[0].id).toBe('b');
        expect(store.entries[1].id).toBe('a');
    });

    it('pendingCount counts only pending entries', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'action-a', status: 'pending' }));
        store.addEntry(makeEntry({ id: 'b', ttpId: 'action-b', status: 'pending' }));
        expect(store.pendingCount).toBe(2);
        store.resolveEntry('action-a', true);
        expect(store.pendingCount).toBe(1);
    });

    it('resolveEntry marks the most recent pending entry for the ttpId as success', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'list-env', status: 'pending' }));
        store.resolveEntry('list-env', true);
        expect(store.entries[0].status).toBe('success');
        expect(store.entries[0].failReason).toBeUndefined();
    });

    it('resolveEntry marks entry as failed and stores failReason', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'list-env', status: 'pending' }));
        store.resolveEntry('list-env', false, 'permission denied');
        expect(store.entries[0].status).toBe('failed');
        expect(store.entries[0].failReason).toBe('permission denied');
    });

    it('resolveEntry with unknown ttpId is a no-op', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'list-env', status: 'pending' }));
        store.resolveEntry('unknown-ttp', true);
        expect(store.entries[0].status).toBe('pending');
    });

    it('resolveEntry on already-resolved entry is a no-op', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'list-env', status: 'success' }));
        store.resolveEntry('list-env', false, 'should not change');
        expect(store.entries[0].status).toBe('success');
    });

    it('clear removes all entries', () => {
        store.addEntry(makeEntry({ id: 'a' }));
        store.addEntry(makeEntry({ id: 'b' }));
        store.clear();
        expect(store.entries).toHaveLength(0);
    });
});
```

- [ ] **Step 1.2: Run tests to confirm they fail**

```bash
cd frontend && pnpm test
```

Expected: FAIL — `Cannot find module '$lib/stores/actionLogStore.svelte'`

- [ ] **Step 1.3: Implement the store**

Create `frontend/src/lib/stores/actionLogStore.svelte.ts`:

```ts
export type ActionLogEntry = {
    id: string;
    ttpId: string;
    ttpName: string;
    targetId: string;
    targetName: string;
    status: 'pending' | 'success' | 'failed';
    failReason?: string;
    startedAt: Date;
};

export class ActionLogStore {
    entries = $state<ActionLogEntry[]>([]);
    drawerOpen = $state(false);

    get pendingCount(): number {
        return this.entries.filter((e) => e.status === 'pending').length;
    }

    addEntry(entry: ActionLogEntry): void {
        this.entries = [entry, ...this.entries];
    }

    resolveEntry(ttpId: string, success: boolean, failReason?: string): void {
        const entry = this.entries.find((e) => e.ttpId === ttpId && e.status === 'pending');
        if (!entry) return;
        entry.status = success ? 'success' : 'failed';
        if (!success && failReason) entry.failReason = failReason;
    }

    clear(): void {
        this.entries = [];
    }
}

export const actionLog = new ActionLogStore();
```

- [ ] **Step 1.4: Run tests to confirm they pass**

```bash
cd frontend && pnpm test
```

Expected: all tests PASS

- [ ] **Step 1.5: Commit**

```bash
cd frontend && git add src/lib/stores/actionLogStore.svelte.ts src/lib/stores/actionLogStore.svelte.test.ts
git commit -m "feat(store): add ActionLogStore for action log drawer"
```

---

### Task 2: Create ActionLogDrawer.svelte

**Files:**
- Create: `frontend/src/lib/components/ActionLogDrawer.svelte`

- [ ] **Step 2.1: Create the component**

Create `frontend/src/lib/components/ActionLogDrawer.svelte`:

```svelte
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
```

- [ ] **Step 2.2: Verify TypeScript compiles**

```bash
cd frontend && pnpm check
```

Expected: no errors related to `ActionLogDrawer.svelte`

- [ ] **Step 2.3: Commit**

```bash
cd frontend && git add src/lib/components/ActionLogDrawer.svelte
git commit -m "feat(ui): add ActionLogDrawer component"
```

---

### Task 3: Wire store into +page.svelte

**Files:**
- Modify: `frontend/src/routes/+page.svelte`

This task removes `ToastMapping` and all action-execution toasts, and replaces them with store calls.

- [ ] **Step 3.1: Add imports at the top of the `<script>` block**

In `frontend/src/routes/+page.svelte`, add two imports to the existing import block (after line 14):

```ts
import { actionLog } from '$lib/stores/actionLogStore.svelte';
import ActionLogDrawer from '$lib/components/ActionLogDrawer.svelte';
```

**Keep** the existing `import { toaster } from '$lib/components/toaster'` — `handleError` still uses `toaster.create` for the `<svelte:boundary>` error boundary.

- [ ] **Step 3.2: Replace `sendAction` — remove spinner toast, add store entry**

Replace the existing `sendAction` function (around lines 254–283) with:

```ts
function sendAction(ttp: TTP, args = {}) {
    selectedTTP = ttp;
    ttpArgContext = { ...args, ...activeGlobalConditions };
    if (ttp.params) {
        showParamModal = true;
    } else if ((ttp.procedures?.length ?? 0) > 1) {
        showParamModal = true;
    } else {
        const targetName = campaignState.getEntityById(selectedObjectId)?.name ?? selectedObjectId;
        actionLog.addEntry({
            id: crypto.randomUUID(),
            ttpId: ttp.id,
            ttpName: ttp.name,
            targetId: selectedObjectId,
            targetName,
            status: 'pending',
            startedAt: new Date()
        });

        ExecuteAction({ actionId: ttp.id, targetId: selectedObjectId, procedureId: '', args: {} })
            .catch((err) => {
                actionLog.resolveEntry(
                    ttp.id,
                    false,
                    typeof err === 'string' ? err : (err?.message ?? 'Unknown error')
                );
            });
    }
}
```

- [ ] **Step 3.3: Replace `onExecuteTTP` — remove spinner toast, add store entry**

Replace the existing `onExecuteTTP` function (around lines 377–400) with:

```ts
const ToastMapping: Record<string, string> = {};
function onExecuteTTP(ttpId: string, execSystemId: string, procedureId: string, args: Record<string, string>) {
    const ttp = campaignState.getTtpById(ttpId);
    const targetName = campaignState.getEntityById(selectedObjectId)?.name ?? selectedObjectId;
    actionLog.addEntry({
        id: crypto.randomUUID(),
        ttpId,
        ttpName: ttp?.name ?? ttpId,
        targetId: selectedObjectId,
        targetName,
        status: 'pending',
        startedAt: new Date()
    });

    closeModal();

    ExecuteAction({ actionId: ttpId, execSystemId, targetId: selectedObjectId, procedureId, args })
        .catch((err) => {
            actionLog.resolveEntry(
                ttpId,
                false,
                typeof err === 'string' ? err : (err?.message ?? 'Unknown error')
            );
        });
}
```

(The `ToastMapping` declaration on the line before `onExecuteTTP` is removed in this replacement — `ToastMapping` is no longer needed.)

- [ ] **Step 3.4: Replace `ttp-executed` handler in `onMount` — remove toasts, call resolveEntry**

Replace the existing `ranAPI.on('ttp-executed', ...)` block (around lines 328–360) with:

```ts
ranAPI.on('ttp-executed', (data) => {
    actionLog.resolveEntry(data.TTP?.id, data.Success, data.FailReason);

    if (data.Success && data.TTP?.id === 'read-file' && data.Args?.PATH) {
        ranAPI.GetFileContent(data.Args.PATH).then((file) => {
            fileViewerPath = file.path ?? data.Args.PATH;
            fileViewerContent = file.content ?? '';
            showFileViewer = true;
        }).catch(() => {});
    }
});
```

- [ ] **Step 3.5: Clear the log on campaign reset**

In `+page.svelte`, find the `campaignState.reset()` call site and add `actionLog.clear()` after it. Search for the reset button `onclick`. It should be in the template — add:

```ts
function handleReset() {
    campaignState.reset();
    actionLog.clear();
}
```

And update the reset button's `onclick` from `campaignState.reset` to `handleReset`. (If there is no explicit reset button wiring in this file, skip this step — `clear()` can be called from wherever reset is triggered.)

- [ ] **Step 3.6: Mount the drawer and wire onfocusentity**

At the end of the page template (after the closing `</div>` of the main layout div, before `</script>`), add:

```svelte
<ActionLogDrawer
    entries={actionLog.entries}
    onfocusentity={(id) => { selectedObjectId = id; }}
/>
```

Place it inside `{#if campaignState.isReady()}` if you want it hidden before the campaign loads, or outside if it should always be available. Outside is fine — the log will simply show "No actions yet" until actions run.

Put it just before the closing `</div>` of the outermost page div:

```svelte
    <!-- existing dialogs ... -->

    <ActionLogDrawer
        entries={actionLog.entries}
        onfocusentity={(id) => { selectedObjectId = id; }}
    />
</div>  <!-- end of h-[calc(100vh-35px)] div -->
```

- [ ] **Step 3.7: Verify TypeScript compiles**

```bash
cd frontend && pnpm check
```

Expected: no errors

- [ ] **Step 3.8: Run tests**

```bash
cd frontend && pnpm test
```

Expected: all tests pass

- [ ] **Step 3.9: Commit**

```bash
cd frontend && git add src/routes/+page.svelte
git commit -m "feat(page): replace action toasts with ActionLogStore and drawer"
```

---

### Task 4: Add app bar toggle button in +layout.svelte

**Files:**
- Modify: `frontend/src/routes/+layout.svelte`

- [ ] **Step 4.1: Add store import**

At the top of the `<script>` block in `frontend/src/routes/+layout.svelte`, add:

```ts
import { actionLog } from '$lib/stores/actionLogStore.svelte';
```

- [ ] **Step 4.2: Add toggle button to AppBar trail**

In the `<AppBar.Trail>` block, add the button between `</nav>` and `<Switch`:

```svelte
<!-- Action log toggle button -->
<button
    class="btn btn-sm relative p-1"
    onclick={() => (actionLog.drawerOpen = !actionLog.drawerOpen)}
    title="Toggle action log"
    aria-label="Toggle action log"
>
    <Icon icon="mdi:history" class="size-5" />
    {#if actionLog.pendingCount > 0 && !actionLog.drawerOpen}
        <span
            class="absolute -top-1 -right-1 bg-warning-500 text-warning-contrast-500 text-[10px] rounded-full w-4 h-4 flex items-center justify-center font-bold"
        >
            {actionLog.pendingCount}
        </span>
    {/if}
</button>
```

- [ ] **Step 4.3: Wire drawer visibility to the store**

The `ActionLogDrawer` in `+page.svelte` currently always renders. Make it conditional on `actionLog.drawerOpen`.

In `frontend/src/routes/+page.svelte`, wrap the `<ActionLogDrawer ...>` mount with:

```svelte
{#if actionLog.drawerOpen}
    <ActionLogDrawer
        entries={actionLog.entries}
        onfocusentity={(id) => { selectedObjectId = id; }}
    />
{/if}
```

- [ ] **Step 4.4: Verify TypeScript compiles**

```bash
cd frontend && pnpm check
```

Expected: no errors

- [ ] **Step 4.5: Run tests**

```bash
cd frontend && pnpm test
```

Expected: all tests pass

- [ ] **Step 4.6: Commit**

```bash
cd frontend && git add src/routes/+layout.svelte src/routes/+page.svelte
git commit -m "feat(layout): add action log toggle button to app bar"
```

---

## Self-review

**Spec coverage check:**

| Spec requirement | Task |
|---|---|
| Bottom drawer, openable/closeable | Task 4 (conditional render + drawerOpen) |
| Button in app bar with pending count badge | Task 4.2 |
| Log entries: action name | Task 2 (ttpName in entry row) |
| Log entries: status icon (spinner/check/X) | Task 2 (status icon block) |
| Log entries: target entity (clickable) | Task 2 (button → onfocusentity) |
| Log entries: timestamp | Task 2 (formatTime) |
| Log entries: failure reason | Task 2 (failReason second line) |
| onfocusentity sets selectedObjectId | Task 3.6 |
| addEntry on dispatch | Task 3.2, 3.3 |
| resolveEntry on ttp-executed | Task 3.4 |
| clear on campaign reset | Task 3.5 |
| Remove action spinner/success/error toasts | Task 3.2, 3.3, 3.4 |
| Keep error toasts (error-msg, parse-audited) | Not touched — they remain in CampaignState |
| Dedicated store (option C) | Task 1 |

**Notes:**
- `ToastMapping` in Task 3.3 is removed. The declaration `const ToastMapping: Record<string, string> = {}` at line 376 of `+page.svelte` must also be deleted — it appears on its own line just before `onExecuteTTP`.
- `toaster` import in `+page.svelte` is kept — `handleError` (used by `<svelte:boundary>`) still calls `toaster.create`. Only the explicit action-dispatch toast calls are removed.
