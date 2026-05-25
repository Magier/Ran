# Operation Timeline

**Date:** 2026-05-25
**Status:** Approved

## Problem

The Action Log Drawer tracks only TTP execution events (pending/success/failed). It gives no visibility into discoveries, credentials obtained, or access gained — the events that explain *why* the graph changed. Users are left asking "where did this node come from?" after every `facts-changed` refresh.

## Goal

Replace the Action Log Drawer with an Operation Timeline: a bottom vertical strip that tells the full campaign story — actions, discoveries, credentials, access gains, and failures — in chronological order with timestamped, human-readable entries.

---

## Data Model

**File:** `frontend/src/lib/stores/timelineStore.svelte.ts`

```ts
export type TtpActionEntry = {
  kind: 'ttp-action';
  id: string;           // cmd_id — correlation key with ttp-executed SSE event
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
  id: string;           // entityId — dedupe key
  entityId: string;
  entityName: string;
  entityKind: string;   // Pod, ServiceAccountToken, Secret, etc.
  timestamp: Date;
};

export type TimelineEntry = TtpActionEntry | EntityEntry;

export class TimelineStore {
  entries = $state<TimelineEntry[]>([]);   // newest first
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
    // Deduplicate: skip if this entityId was already recorded
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

---

## Backend: `entity-discovered` SSE Event

**File:** `crates/app/src/lib.rs`

In the SSE bridge loop, when processing `CampaignEvent::FactsChanged`, iterate `new_entities` and emit one `entity-discovered` event per entity alongside the existing `facts-changed` event.

Category assignment (Rust-side):
- `kind` in `["ServiceAccountToken", "Secret"]` → `"credential"`
- entity `access_level` ≥ exec → `"access-gained"`
- everything else → `"discovery"`

Payload shape:
```json
{
  "type": "entity-discovered",
  "data": {
    "entityId": "ns/default/pod/web-app",
    "entityName": "web-app",
    "entityKind": "Pod",
    "category": "discovery"
  }
}
```

The `entity-discovered` event fires for every entity in `new_entities`, in addition to the existing `facts-changed` event (which continues to drive graph refresh).

---

## Component: OperationTimeline

**File:** `frontend/src/lib/components/OperationTimeline.svelte`

Replaces `ActionLogDrawer.svelte`. Same position: bottom of graph column, `h-60`, `border-t border-surface-200-800`.

### Entry row layout

```
[icon]  Label text                                          HH:MM:SS
        failure reason (second line, error color — failed ttp-action only)
```

### Icons and colors

| kind | status | icon | color |
|------|--------|------|-------|
| `ttp-action` | pending | `svg-spinners:90-ring-with-bg` | surface |
| `ttp-action` | success | `mdi:check-circle` | success-500 |
| `ttp-action` | failed | `mdi:close-circle` | error-500 |
| `discovery` | — | `mdi:magnify` | primary-400 |
| `credential` | — | `mdi:key` | warning-500 |
| `access-gained` | — | `mdi:shield-check` | success-400 |

### Label generation (frontend, pure function)

```
ttp-action (any status)          → "{ttpName} on {targetName}"
discovery + Pod                  → "Discovered pod {entityName}"
discovery + Namespace            → "Discovered namespace {entityName}"
discovery + ServiceAccount       → "Discovered service account {entityName}"
discovery + (other)              → "Discovered {entityKind} {entityName}"
credential + ServiceAccountToken → "Obtained token for {entityName}"
credential + Secret              → "Found secret {entityName}"
credential + (other)             → "Found credential {entityName}"
access-gained + (any)            → "Gained exec access to {entityName}"
```

### Clickability

Only `ttp-action` entries have a clickable target (calls `onfocusentity(targetId)`). Entity entries have no click handler in V1.

### Header

"Operation Timeline" (was "Action Log"). Entry count remains. App bar toggle button `aria-label` and `title` updated to match.

---

## Event Wiring

### `+page.svelte`

- Replace all `actionLog.*` calls with `timeline.*`
- `sendAction()` and `onExecuteTTP()` become async: call `ExecuteAction(cmd)`, await the response, then call `timeline.addTtpAction({ id: response.cmd_id, ... })`. If the POST throws, show a toast — no timeline entry is added. This fixes a pre-existing bug where pending entries were correlated by TTP definition id (breaking with concurrent same-TTP executions). The `execute_action_handler` already returns `{ cmd_id }` in `ExecuteActionAck`; the generated types lag behind — cast the response as `any` to read `cmd_id`, or update the OpenAPI spec.
- `ttp-executed` handler: `timeline.resolveTtpAction(data.CmdId, data.Success, data.FailReason)`
- New `entity-discovered` SSE handler in `onMount`:

```ts
ranAPI.on('entity-discovered', (data) => {
  timeline.addEntityEvent({
    kind: data.category,
    id: data.entityId,
    entityId: data.entityId,
    entityName: data.entityName,
    entityKind: data.entityKind,
    timestamp: new Date()
  });
});
```

- `<ActionLogDrawer>` → `<OperationTimeline>` with same `entries` and `onfocusentity` props

### `+layout.svelte`

- Toggle button imports `timeline` from `timelineStore.svelte.ts`
- `aria-label` and `title`: "Operation Timeline"
- Badge still shows `timeline.pendingCount`

---

## Files Changed

| File | Change |
|------|--------|
| `src/lib/stores/timelineStore.svelte.ts` | New |
| `src/lib/components/OperationTimeline.svelte` | New |
| `src/routes/+page.svelte` | Update imports + wiring |
| `src/routes/+layout.svelte` | Update toggle button |
| `crates/app/src/lib.rs` | Emit `entity-discovered` per new entity in `FactsChanged` handler |
| `src/lib/stores/actionLogStore.svelte.ts` | Delete |
| `src/lib/stores/actionLogStore.svelte.test.ts` | Delete |
| `src/lib/components/ActionLogDrawer.svelte` | Delete |

---

## Out of Scope

- Filtering timeline by category
- Persisting timeline across page reloads
- Resizable strip height
- Clicking entity events to focus graph node
- Showing the timeline on the Flow tab
