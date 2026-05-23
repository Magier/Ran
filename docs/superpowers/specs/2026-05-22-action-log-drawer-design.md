# Action Log Drawer

**Date:** 2026-05-22
**Status:** Approved

## Problem

The current UX shows a spinner toast when an action starts and replaces it with a success/error toast when it finishes. Toasts are ephemeral — once dismissed they're gone. There's no way to review what ran, against what target, or why something failed without re-running it.

## Goal

A persistent, openable/closeable drawer that lists all executed actions in chronological order with their status (pending, success, failed), the target entity (clickable to focus in graph), and failure details when relevant.

---

## Data Model

**File:** `frontend/src/lib/stores/actionLogStore.svelte.ts`

```ts
type ActionLogEntry = {
  id: string;        // cmd_id from ExecuteActionCmd — correlation key with ttp-executed SSE event
  ttpId: string;
  ttpName: string;
  targetId: string;
  targetName: string;  // resolved display name at dispatch time
  status: 'pending' | 'success' | 'failed';
  failReason?: string;
  startedAt: Date;
}
```

The store is a Svelte 5 `$state`-based singleton (same pattern as `CampaignState`):

- `entries: ActionLogEntry[]` — reactive array, newest first
- `addEntry(entry: ActionLogEntry)` — prepends a new pending entry
- `resolveEntry(id: string, success: boolean, failReason?: string)` — finds by `id`, updates `status` and optionally `failReason`
- `clear()` — empties the list (called on campaign reset)
- `drawerOpen: boolean` — reactive toggle, shared between layout button and page drawer

The `id` field maps to `ExecuteActionCmd.cmd_id` (already in the generated API types at `gen_types.ts:507`). `cmd_id` is generated client-side with `crypto.randomUUID()` before each `ExecuteAction` call. The `ttp-executed` SSE event carries this same id as `data.id` — the handler matches on `data.id`, not `data.TTP?.id` (which is the TTP definition id and can't distinguish concurrent executions of the same TTP). This replaces the `ToastMapping: Record<string, string>` in `+page.svelte`.

---

## Event Wiring

### Dispatching an action

Both `sendAction()` and `onExecuteTTP()` in `+page.svelte`:

1. Call `actionLog.addEntry({ id: cmd.cmd_id, ..., status: 'pending', startedAt: new Date() })`
2. Call `ExecuteAction(cmd)` — no toast created
3. On `.catch(err)`: call `actionLog.resolveEntry(cmd.cmd_id, false, err.message)`

### Receiving the result

The existing `ttp-executed` handler in `+page.svelte` (currently dismisses the spinner toast and creates a success/error toast):

- Replace toast calls with `actionLog.resolveEntry(data.id, data.Success, data.FailReason)`
- Keep the `read-file` file viewer logic unchanged

### Campaign reset

`campaignState.reset()` call site also calls `actionLog.clear()`.

### What stays as toasts

- `error-msg` SSE events → error toast (unchanged)
- `parse-audited` gap events → error toast (unchanged)
- File save errors in `app_menu.svelte` → error toast (unchanged)

---

## Bottom Drawer Component

**File:** `frontend/src/lib/components/ActionLogDrawer.svelte`

### Layout

- Fixed position, bottom of viewport, full width
- Height: 240px when open (shows ~4–5 entries)
- Z-index above graph/panels, below modals
- Lives in `+page.svelte` (not layout, since the entity-focus callback is graph-specific)
- Controlled by `open: boolean` prop in `+page.svelte`

### Entry row

```
[status icon]  TTP Name  →  [target entity button]    HH:MM:SS
               Failure reason (second line, error color, only when failed)
```

- **Status icon**: `svg-spinners:90-ring-with-bg` (pending), `mdi:check-circle` green (success), `mdi:close-circle` red (failed)
- **Target entity** is a `<button>` that calls `onfocusentity(targetId)` — the parent sets `selectedObjectId = targetId` which focuses the graph node and opens entity info
- Entries are ordered newest-first

### App bar button

Added to `+layout.svelte` AppBar trail (or passed down via context):

- Icon: `mdi:history` or `mdi:format-list-bulleted`
- Badge: count of `pending` entries; disappears when drawer is open or no pending entries
- Toggles `drawerOpen` boolean in `+page.svelte`

Since the drawer lives in `+page.svelte` but the button needs to be in the layout's app bar, `drawerOpen: boolean` is included in `actionLogStore.svelte.ts` as a store-level reactive property. Both `+layout.svelte` (button) and `+page.svelte` (drawer) import the store and read/write `drawerOpen` directly.

---

## Files Changed

| File | Change |
|------|--------|
| `src/lib/stores/actionLogStore.svelte.ts` | New — store with entries, addEntry, resolveEntry, clear, drawerOpen |
| `src/lib/components/ActionLogDrawer.svelte` | New — drawer UI component |
| `src/routes/+page.svelte` | Remove ToastMapping + action toasts; wire addEntry/resolveEntry; add drawer + onfocusentity handler |
| `src/routes/+layout.svelte` | Add app bar button that toggles drawerOpen from store |

---

## Out of Scope

- Persisting the log across page reloads
- Filtering or searching the log
- Resizable drawer height
- Showing the log on the Flow tab
