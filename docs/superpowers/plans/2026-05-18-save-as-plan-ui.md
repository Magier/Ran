# Save as Plan — UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Save" submenu to the attack flow menu with three options — Save as JSON (existing), Save as Plan, and Save as Plan (with failed steps).

**Architecture:** A nested Skeleton UI `Menu` replaces the flat "Save" item; a new `ExportAsPlan(includeFailed)` method is added to `RanAPI` (raw fetch, returns YAML string) and forwarded through `CampaignState`. The menu handler downloads the result via the existing `saveFile` utility.

**Tech Stack:** Svelte 5, SvelteKit, Skeleton UI v3 (`@skeletonlabs/skeleton-svelte`), TypeScript. Backend endpoint `GET /api/plans/export?include_failed=<bool>` already exists and returns raw YAML text.

---

### Task 1: Add `ExportAsPlan` to `RanAPI` and `CampaignState`

**Files:**
- Modify: `frontend/src/lib/ran_api.ts` (lines 313–325 area and 401–407 exports)
- Modify: `frontend/src/lib/components/CampaignState.svelte.ts` (line 508 area)

The backend endpoint `GET /api/plans/export?include_failed=true|false` returns a plain-text YAML string. Use `fetch` directly rather than the typed REST client (the endpoint is not in the OpenAPI spec).

- [ ] **Step 1: Add `ExportAsPlan` to the `RanAPI` class**

  Open `frontend/src/lib/ran_api.ts`. After the `ExportAttackFlow` method (currently at line ~313), add:

  ```typescript
  async ExportAsPlan(includeFailed: boolean): Promise<string> {
      const response = await fetch(`/api/plans/export?include_failed=${includeFailed}`);
      if (!response.ok) {
          throw new Error('Failed to export plan');
      }
      return response.text();
  }
  ```

- [ ] **Step 2: Export `ExportAsPlan` as a bound function**

  At the bottom of `frontend/src/lib/ran_api.ts`, after the existing `export const SaveFlow = ...` line, add:

  ```typescript
  export const ExportAsPlan = ranAPI.ExportAsPlan.bind(ranAPI);
  ```

- [ ] **Step 3: Add `ExportAsPlan` to `CampaignState`**

  Open `frontend/src/lib/components/CampaignState.svelte.ts`. After the `ExportAttackFlow` method (currently at line ~508), add:

  ```typescript
  ExportAsPlan(includeFailed: boolean): Promise<string> {
      return this.api.ExportAsPlan(includeFailed);
  }
  ```

- [ ] **Step 4: Run type check**

  ```bash
  cd /Users/me/Dev/Ran/frontend && pnpm check
  ```

  Expected: no TypeScript errors.

- [ ] **Step 5: Commit**

  ```bash
  git add frontend/src/lib/ran_api.ts frontend/src/lib/components/CampaignState.svelte.ts
  git commit -m "feat(frontend): add ExportAsPlan to RanAPI and CampaignState"
  ```

---

### Task 2: Replace flat "Save" menu item with submenu in `app_menu.svelte`

**Files:**
- Modify: `frontend/src/lib/components/app_menu.svelte`

Skeleton UI's `Menu` supports nested menus: when a child `<Menu>` is rendered inside a parent `<Menu.Content>`, the `RootContext` automatically wires `setParent`/`setChild`. `Menu.TriggerItem` acts as the visible entry point that both renders as a menu item and opens the child menu on hover/click.

- [ ] **Step 1: Replace the `<script>` block with the updated switch**

  Replace the entire `<script>` block in `frontend/src/lib/components/app_menu.svelte` with:

  ```svelte
  <script lang="ts">
      import { showToast, toaster } from '$lib/components/toaster';
      import Icon from '@iconify/svelte';
      import { saveFile } from '$lib/io';
      import { Menu, Portal } from '@skeletonlabs/skeleton-svelte';
      import { getCampaignState } from '$lib/components/CampaignState.svelte';

      const campaignState = getCampaignState();

      function onMenuClick(event) {
          let {value} = event;

          switch (value) {
              case 'reset':
                   campaignState.reset()
                  break;
              case 'save_flow':
                  campaignState.ExportAttackFlow().then((flow) => {
                      const fileName = `campaign_${new Date().toISOString()}.json`;
                      const data = JSON.stringify(flow, null, 2);
                      saveFile(data, fileName, 'application/json');
                  }).catch((error) => {
                      console.error('Error getting flow:', error);
                      showToast('Failed to save flow', `Could not get flow: ${error.message}`, 'error');
                  });
                  break;
              case 'save_plan':
                  campaignState.ExportAsPlan(false).then((yaml) => {
                      const fileName = `plan_${new Date().toISOString().replace(/[:.]/g, '-')}.yaml`;
                      saveFile(yaml, fileName, 'application/yaml');
                  }).catch((error) => {
                      console.error('Error exporting plan:', error);
                      showToast('Failed to save plan', `Could not export plan: ${error.message}`, 'error');
                  });
                  break;
              case 'save_plan_failed':
                  campaignState.ExportAsPlan(true).then((yaml) => {
                      const fileName = `plan_${new Date().toISOString().replace(/[:.]/g, '-')}.yaml`;
                      saveFile(yaml, fileName, 'application/yaml');
                  }).catch((error) => {
                      console.error('Error exporting plan:', error);
                      showToast('Failed to save plan', `Could not export plan: ${error.message}`, 'error');
                  });
                  break;
              default:
                  console.log('Unknown menu item:', value);
                  break;
          }
      }
  </script>
  ```

- [ ] **Step 2: Replace the template with the submenu structure**

  Replace the entire template section (everything after `</script>`) with:

  ```svelte
  <Menu onSelect={onMenuClick}>
      <Menu.Trigger class="btn hover:preset-tonal text-xl">
          <Icon icon="game-icons:fishing-net" rotate={90} class="fill-token h-6 w-6 -scale-x-100" />
          Ran
      </Menu.Trigger>
      <Portal>
          <Menu.Positioner>
              <Menu.Content>
                  <Menu.ItemGroup>
                      <Menu.ItemGroupLabel>Campaign</Menu.ItemGroupLabel>
                      <Menu.Item value="reset">
                          <Menu.ItemText>Reset</Menu.ItemText>
                      </Menu.Item>
                  </Menu.ItemGroup>
                  <Menu.Separator />
                  <Menu.ItemGroup>
                      <Menu.ItemGroupLabel>Flow</Menu.ItemGroupLabel>
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
                  </Menu.ItemGroup>
              </Menu.Content>
          </Menu.Positioner>
      </Portal>
  </Menu>
  ```

- [ ] **Step 3: Run type check**

  ```bash
  cd /Users/me/Dev/Ran/frontend && pnpm check
  ```

  Expected: no TypeScript errors.

- [ ] **Step 4: Commit**

  ```bash
  git add frontend/src/lib/components/app_menu.svelte
  git commit -m "feat(frontend): replace flat Save item with submenu (JSON, Plan, Plan+failed)"
  ```
