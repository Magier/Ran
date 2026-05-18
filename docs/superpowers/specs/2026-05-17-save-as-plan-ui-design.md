# Save as Plan — UI Design

**Date:** 2026-05-17
**Status:** Approved for implementation

## Overview

Replace the flat "Save" menu item in the attack flow view with a submenu that offers three save formats: JSON (existing), Plan (success-only), and Plan (with failed steps).

---

## Changes

### `frontend/src/lib/ran_api.ts`

Add one new method: `ExportAsPlan(includeFailed: boolean): Promise<void>`

- Calls `GET /api/plans/export?include_failed={includeFailed}` on the current campaign
- On success: calls `saveFile(yaml, \`plan_${timestamp}.yaml\`, 'application/yaml')` where timestamp is `new Date().toISOString().replace(/[:.]/g, '-')`
- On error: calls `showToast('Failed to save plan', ..., 'error')` (same error pattern used elsewhere in the file)
- Export as a bound function alongside the existing `ExportAttackFlow` export at the bottom of the file

### `frontend/src/lib/components/app_menu.svelte`

Replace the flat `<Menu.Item value="save_flow">` with a nested `<Menu>` component using `Menu.TriggerItem`. Skeleton UI's `Menu.Root` automatically wires parent/child menus via Svelte context — no manual plumbing needed.

```svelte
<Menu onSelect={onMenuClick}>
    <Menu.TriggerItem>
        <Menu.ItemText>Save</Menu.ItemText>
    </Menu.TriggerItem>
    <Portal>
        <Menu.Positioner>
            <Menu.Content>
                <Menu.Item value="save_flow">
                    <Menu.ItemText>Save as JSON</Menu.ItemText>
                </Menu.Item>
                <Menu.Item value="save_plan">
                    <Menu.ItemText>Save as Plan</Menu.ItemText>
                </Menu.Item>
                <Menu.Item value="save_plan_failed">
                    <Menu.ItemText>Save as Plan (with failed steps)</Menu.ItemText>
                </Menu.Item>
            </Menu.Content>
        </Menu.Positioner>
    </Portal>
</Menu>
```

Add two cases to the existing `onMenuClick` switch (the nested Menu uses the same handler):

```
case 'save_plan':        campaignState.ExportAsPlan(false); break;
case 'save_plan_failed': campaignState.ExportAsPlan(true);  break;
```

---

## Backend

The endpoint `GET /api/plans/export?include_failed=false|true` already exists. No backend changes required.

---

## Scope

Two files changed, no new components, no new state.
