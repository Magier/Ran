# Operation Timeline — Action Grouping Design

## Overview

Consolidate the flat Operation Timeline into a hierarchical view where entity effect events (discovery, credential, access-gained) are grouped as children of the TTP action that produced them. Action rows are collapsible and show a typed effect count summary at a glance. Standalone entity events (no parent action) continue to render as flat rows.

---

## Data Model

### `EntityEntry` — new field

```ts
type EntityEntry = {
    kind: 'discovery' | 'credential' | 'access-gained';
    id: string;
    entityId: string;
    entityName: string;
    entityKind: string;
    cmdId?: string;   // correlates with parent TtpActionEntry.id; absent → standalone
    timestamp: Date;
};
```

### `TtpActionEntry` — new fields

```ts
type TtpActionEntry = {
    kind: 'ttp-action';
    id: string;
    ttpId: string;
    ttpName: string;
    targetId: string;
    targetName: string;
    execSystemId?: string;    // only when exec system differs from target
    execSystemName?: string;  // only when exec system differs from target
    status: 'pending' | 'success' | 'failed';
    failReason?: string;
    timestamp: Date;
};
```

### New top-level types

```ts
type ActionGroup = {
    kind: 'action-group';
    action: TtpActionEntry;
    effects: EntityEntry[];   // ordered by arrival time
    collapsed: boolean;       // starts true
    score?: number;           // future utility score; absent → renders nothing
};

type TopEntry = ActionGroup | EntityEntry;  // EntityEntry here = standalone (no cmdId or unmatched cmdId)
```

`TimelineStore.entries` is replaced by `topEntries: TopEntry[]`.

---

## Store Behaviour

### Internal index

The store maintains a private `Map<string, ActionGroup>` keyed by `action.id` for O(1) group lookup when appending effects.

### Method changes

| Method | Behaviour |
|---|---|
| `addTtpAction(entry)` | Creates `ActionGroup` with empty `effects`, `collapsed: true`; prepends to `topEntries`; registers in index |
| `addEntityEvent(entry)` | If `entry.cmdId` matches a group in the index, appends to `group.effects` in-place (Svelte fine-grained reactivity). Otherwise prepends as a standalone `EntityEntry` to `topEntries`. |
| `resolveTtpAction(id, success, failReason?)` | Finds group via index; mutates `group.action.status` and `group.action.failReason` |
| `toggleGroup(cmdId)` | Flips `group.collapsed` |
| `pendingCount` | Counts `ActionGroup` entries where `action.status === 'pending'` |
| `clear()` | Clears `topEntries` and the index |

### Deduplication

Entity event deduplication (by `id`) continues to apply before the group-or-standalone decision.

---

## Backend Protocol Change

`entity-discovered` WebSocket events gain an optional field:

```json
{ "cmdId": "abc-123", ... }
```

When absent the frontend treats the event as standalone — no behaviour change for older backend versions.

### Frontend handler update (`+page.svelte`)

```ts
ranAPI.on('entity-discovered', (data) => {
    timeline.addEntityEvent({
        kind: data.category ?? 'discovery',
        id: data.entityId,
        entityId: data.entityId,
        entityName: data.entityName,
        entityKind: data.entityKind,
        cmdId: data.cmdId,          // may be undefined
        timestamp: new Date()
    });
});
```

### `onExecuteTTP` update

`execSystemId` and `execSystemName` are captured when available:

```ts
timeline.addTtpAction({
    id: cmdId,
    ttpId,
    ttpName: ttp?.name ?? ttpId,
    targetId: selectedObjectId,
    targetName,
    execSystemId: execSystemId !== selectedObjectId ? execSystemId : undefined,
    execSystemName: execSystemId !== selectedObjectId
        ? (campaignState.getEntityById(execSystemId)?.name ?? execSystemId)
        : undefined,
    status: 'pending',
    timestamp: new Date()
});
```

---

## Component — `OperationTimeline.svelte`

### Props

```ts
interface Props {
    entries: TopEntry[];          // renamed from TimelineEntry[]
    onfocusentity: (targetId: string) => void;
}
```

### Rendering

```
for each topEntry in entries:
  if topEntry.kind === 'action-group':
    render ActionGroupRow
  else:
    render EntityRow  (unchanged from today)
```

### ActionGroupRow — collapsed header layout

```
[status icon]  [ttpName]  on [targetName]  via [execSystemName?]  ·  [🔍 N  🔑 N  🛡 N]  [score?]  [▶/▼]  [time]
```

- **`on [targetName]`** — `text-primary-500 hover:underline`, clicking fires `onfocusentity(action.targetId)`
- **`via [execSystemName]`** — only shown when `execSystemId` is set and differs from `targetId`; rendered as `text-surface-500 text-xs` (lower salience)
- **Effect chips** — only counts > 0 are rendered; a pending action with no effects yet shows nothing here
  - Discovery: `mdi:magnify` `text-primary-400`
  - Credential: `mdi:key` `text-warning-500`
  - Access: `mdi:shield-check` `text-success-400`
- **Utility score** — `{#if group.score != null}<span class="text-xs text-surface-400">★ {group.score.toFixed(1)}</span>{/if}` — compiles to nothing until populated
- **Chevron toggle** — `mdi:chevron-right` / `mdi:chevron-down`; only the chevron is the click target (preserves entity-focus click on target name)
- **Timestamp** — far right, same as today

### ActionGroupRow — expanded children

Child `EntityEntry` rows render below the header, indented with a left border accent, using the same icon + label + timestamp layout as standalone entries.

### Effect count derivation

Counts are derived inline inside the `{#each}` block from `group.effects` — no store getter needed:

```ts
const discoveryCount = group.effects.filter(e => e.kind === 'discovery').length;
const credentialCount = group.effects.filter(e => e.kind === 'credential').length;
const accessCount = group.effects.filter(e => e.kind === 'access-gained').length;
```

---

## Test Coverage

- `addTtpAction` creates a group with empty effects and `collapsed: true`
- `addEntityEvent` with matching `cmdId` appends to group effects (not `topEntries`)
- `addEntityEvent` with absent/unmatched `cmdId` prepends as standalone to `topEntries`
- `addEntityEvent` deduplication still applies before group-or-standalone decision
- `toggleGroup` flips `collapsed`
- `resolveTtpAction` updates status on the action inside the group
- `pendingCount` counts only pending groups
- `clear` empties both `topEntries` and the index

---

## Out of Scope

- Utility score calculation logic (future)
- Persisting collapse state across page reloads
- Concurrent action attribution edge cases beyond the `cmdId` correlation
