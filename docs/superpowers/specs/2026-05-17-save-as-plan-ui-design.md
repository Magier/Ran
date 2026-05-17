# Save as Plan — UI Design

**Date:** 2026-05-17
**Status:** Approved for implementation

## Overview

Add "Save as Plan" and "Save as Plan (with failed steps)" menu items to the attack flow view. These call the existing `/api/plans/export` endpoint and download the resulting YAML to the user's machine.

---

## Changes

### `frontend/src/lib/ran_api.ts`

Add one new method: `ExportAsPlan(includeFailed: boolean): Promise<void>`

- Calls `GET /api/plans/export?include_failed={includeFailed}` on the current campaign
- On success: calls `saveFile(yaml, \`plan_${timestamp}.yaml\`, 'application/yaml')` where timestamp is `new Date().toISOString().replace(/[:.]/g, '-')`
- On error: calls `showToast('Failed to save plan', ..., 'error')` (same error pattern used elsewhere in the file)
- Export as a bound function alongside the existing `ExportAttackFlow` export at the bottom of the file

### `frontend/src/lib/components/app_menu.svelte`

Add two menu items to the Flow section, after the existing "Save" item:

```
{ label: 'Save as Plan', value: 'save-plan' }
{ label: 'Save as Plan (with failed steps)', value: 'save-plan-failed' }
```

Add two cases to the `onMenuClick` switch:

```
case 'save-plan':         campaignState.ExportAsPlan(false); break;
case 'save-plan-failed':  campaignState.ExportAsPlan(true);  break;
```

---

## Backend

The endpoint `GET /api/plans/export?include_failed=false|true` already exists. No backend changes required.

---

## Scope

Two files changed, no new components, no new state. The download and error handling patterns are identical to the existing `ExportAttackFlow` flow.
