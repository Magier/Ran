# Design: Cleanup After Emulation

**Date:** 2026-04-30
**Status:** Approved

## Overview

When a campaign is reset, Ran should automatically dispatch cleanup procedures for every successfully executed attack step, wait for all cleanup results, record them in the audit trail, and only then wipe campaign state. This closes the gap with the legacy Go implementation's `cleanupSteps()` and leaves the cluster in a known state after an emulation.

---

## Goals

- Execute each TTP's `cleanup` procedure (if defined) when the campaign is reset.
- All cleanup steps run in parallel — no ordering dependencies for now.
- Each cleanup result (success / failure) is recorded before state is wiped.
- The full audit trail (attack steps + cleanup outcomes) is readable via the API before reset completes.
- Reset proceeds after all cleanup steps complete or the timeout expires — never hangs indefinitely.

## Non-Goals

- Cleanup dependency ordering / sequencing (future work).
- A dedicated `/api/campaign/cleanup` endpoint (reset is the only trigger for now).
- Cleanup on partial reset or session close.

---

## Data Model Changes

### `crates/armory` — `Ttp.cleanup`

Add an optional `cleanup` procedure to `Ttp`. It uses the existing `Procedure` type; no new struct needed.

```rust
pub struct Ttp {
    // ... existing fields unchanged ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<Procedure>,
}
```

`RawTtp` gets a matching `cleanup: Option<RawProcedure>`. The `into_ttp` conversion maps it to `Ttp.cleanup`.

YAML example (already used by some TTPs in the armory):

```yaml
cleanup:
  key: ubuntu
  command: apt remove -y ${PKG}
```

### `crates/c2` — `ExecTtp.is_cleanup`

Add `is_cleanup: bool` (default `false`) to `ExecTtp`. Set to `true` by `build_cleanup_actions` so the flag flows through to the execution record.

### `crates/campaign` — `ExecutionRecord.is_cleanup`

Add `is_cleanup: bool` (default `false`) to `ExecutionRecord`. Copied from `ExecTtp` in `ExecutionRecord::from_execution`. Cleanup records appear in `execution_records` alongside attack steps — no separate collection.

---

## Campaign Changes (`crates/campaign`)

### Refactor `prepare_action`

Extract a private `prepare_action_with_ttp` that contains the full grounding/routing pipeline:

```
prepare_action(request, armory)
  → look up Ttp from armory
  → prepare_action_with_ttp(request, ttp)   ← new internal entry point

build_cleanup_actions(armory)
  → per record: synthesize cleanup Ttp
  → prepare_action_with_ttp(request, cleanup_ttp)
```

This avoids duplicating the six-stage pipeline and keeps `build_cleanup_actions` as a thin wrapper.

### `Campaign::build_cleanup_actions(armory: &Armory) -> Vec<ExecTtp>`

```
for each execution_record in execution_records (reversed):
    look up ttp = armory.get_ttp(record.ttp_id)
    if ttp.cleanup is None → skip
    synthesize cleanup_ttp:
        id   = "{ttp_id}_cleanup"
        name = "{ttp_name} Cleanup"
        tactic / techniques / params = same as original
        procedures = [ttp.cleanup]   ← single procedure
    build request:
        action_id   = cleanup_ttp.id   (unused — we pass ttp directly)
        target_id   = record.target_id
        args        = record.args
    call prepare_action_with_ttp(request, cleanup_ttp) → ExecTtp
    set exec_ttp.is_cleanup = true   ← stamped after grounding, not on request
    on error: log and continue (don't abort remaining cleanup steps)

return Vec<ExecTtp>
```

---

## App Layer Changes (`crates/app/src/lib.rs`)

### `reset_campaign()` — two-phase reset

**Phase 1 — Cleanup dispatch**

1. Acquire read lock on campaign.
2. Call `campaign.build_cleanup_actions(&self.armory)`.
3. Release lock.
4. If the returned list is empty, skip to Phase 2.
5. Dispatch all `ExecTtp`s to C2 concurrently (`FuturesUnordered` or `tokio::join_all`).
6. Each response arrives via the existing `TtpExecuted` event loop, which calls `on_ttp_executed` → appends an `ExecutionRecord { is_cleanup: true, ... }` to the campaign.
7. Await all cleanup command IDs to appear in `execution_records` (poll the campaign), with a **30-second timeout**. On timeout: log a warning listing unresolved cmd IDs, then proceed.

**Phase 2 — State reset**

8. Acquire write lock on campaign.
9. Call `campaign.reset(ran_name, target_cluster)` — wipes all state.
10. Publish `CampaignEvent::Reset`.

---

## Timeout Behavior

- Default timeout: **30 seconds** (configurable via `AppConfig` later if needed).
- On timeout: reset proceeds anyway, incomplete cleanup steps are silently abandoned (their `ExecutionRecord`s were not yet written, which is acceptable — the cluster may need manual cleanup).
- Timeout applies to the full cleanup batch, not per-step.

---

## Files Touched

| File | Change |
|------|--------|
| `crates/armory/src/model.rs` | Add `cleanup: Option<Procedure>` to `Ttp` |
| `crates/armory/src/raw.rs` | Add `cleanup: Option<RawProcedure>` to `RawTtp`; map in `into_ttp` |
| `crates/c2/src/types.rs` | Add `is_cleanup: bool` to `ExecTtp` |
| `crates/campaign/src/execution_record.rs` | Add `is_cleanup: bool`; copy from `ExecTtp` in `from_execution` |
| `crates/campaign/src/campaign/execution.rs` | Extract `prepare_action_with_ttp`; add `build_cleanup_actions` |
| `crates/app/src/lib.rs` | Rewrite `reset_campaign` with two-phase cleanup+reset |

---

## Open Questions

None — all design decisions confirmed.
