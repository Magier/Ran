import type { BootstrapOperation } from '$lib/api';

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
    timestamp?: Date;
    startup?: boolean;
    detail?: string;
};

export type EntityEntry = {
    kind: 'discovery' | 'credential' | 'access-gained';
    id: string;
    entityId: string;
    entityName: string;
    entityKind: string;
    cmdId?: string;
    timestamp?: Date;
};

export type ActionGroup = {
    kind: 'action-group';
    action: TtpActionEntry;
    effects: EntityEntry[];
    collapsed: boolean;
    score?: number;
};

export type TopEntry = ActionGroup | EntityEntry;

/** A historical execution distilled to what the timeline needs to replay it. */
export type BackfillRecord = {
    id: string;
    ttpId: string;
    ttpName: string;
    targetId: string;
    targetName: string;
    execSystemId?: string;
    execSystemName?: string;
    success: boolean;
    failReason?: string;
    timestampMs: number;
};

/** An in-flight (dispatched, not yet completed) action to seed as pending. */
export type PendingRecord = Omit<BackfillRecord, 'success' | 'failReason'>;

export class TimelineStore {
    topEntries = $state<TopEntry[]>([]);
    open = $state(true);

    private index = new Map<string, ActionGroup>();
    private seenEntityIds = new Set<string>();

    pendingCount = $derived(
        this.topEntries.filter(
            (e): e is ActionGroup => e.kind === 'action-group' && e.action.status === 'pending'
        ).length
    );

    /** Prepend a fresh action group and return the reactive proxy stored in the index. */
    private createGroup(entry: Omit<TtpActionEntry, 'kind'>): ActionGroup {
        const action: TtpActionEntry = { kind: 'ttp-action', ...entry };
        const group: ActionGroup = { kind: 'action-group', action, effects: [], collapsed: true };
        this.topEntries = [group, ...this.topEntries];
        const proxy = this.topEntries[0] as ActionGroup;
        this.index.set(entry.id, proxy);
        return proxy;
    }

    addTtpAction(entry: Omit<TtpActionEntry, 'kind'>): void {
        // The ttp-executed SSE may have already created (and resolved) this entry
        // if it beat the HTTP response. Enrich it with the names the UI knows
        // rather than creating a duplicate, and leave the resolved status intact.
        const existing = this.index.get(entry.id);
        if (existing) {
            existing.action.ttpId = entry.ttpId;
            existing.action.ttpName = entry.ttpName;
            existing.action.targetId = entry.targetId;
            existing.action.targetName = entry.targetName;
            if (entry.execSystemId) {
                existing.action.execSystemId = entry.execSystemId;
                existing.action.execSystemName = entry.execSystemName;
            }
            return;
        }
        this.createGroup(entry);
    }

    addEntityEvent(entry: EntityEntry): void {
        // Global dedup: each entity id appears at most once across all groups and standalone rows.
        // This matches the previous flat-list dedup behaviour. Reset on clear().
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

    /**
     * Record a completed TTP execution (driven by the `ttp-executed` SSE).
     *
     * Resolves the matching pending entry when the action was initiated from
     * this UI. When no entry exists — e.g. the action was fired via the MCP
     * server or an autonomous plan — it creates a new, already-resolved entry
     * so MCP-driven actions show up in the timeline, not just the flow page.
     */
    recordExecutedTtp(entry: Omit<TtpActionEntry, 'kind'>): void {
        const existing = this.index.get(entry.id);
        if (existing) {
            if (existing.action.status === 'pending') {
                existing.action.status = entry.status;
                if (entry.status === 'failed') existing.action.failReason = entry.failReason;
            }
            return;
        }
        this.createGroup(entry);
    }

    /**
     * Seed the timeline with the campaign's existing execution history.
     *
     * The store is otherwise live-only: it just listens to SSE events that
     * arrive after the UI connects, so a session attached to an already-running
     * campaign would start blank. This replays the persisted records — newest
     * last so the prepend in `recordExecutedTtp` leaves them newest-first — and
     * is idempotent via the per-id index, so it's safe to call alongside any
     * live `ttp-executed` events that race in.
     */
    backfill(records: BackfillRecord[]): void {
        const ordered = [...records].sort((a, b) => a.timestampMs - b.timestampMs);
        for (const r of ordered) {
            this.recordExecutedTtp({
                id: r.id,
                ttpId: r.ttpId,
                ttpName: r.ttpName,
                targetId: r.targetId,
                targetName: r.targetName,
                execSystemId: r.execSystemId,
                execSystemName: r.execSystemName,
                status: r.success ? 'success' : 'failed',
                failReason: r.success ? undefined : r.failReason,
                timestamp: new Date(r.timestampMs)
            });
        }
    }

    /**
     * Seed in-flight actions as pending entries (driven by `/api/flow`'s
     * `Ongoing` steps on load). Routed through `addTtpAction` so it's idempotent
     * against a live `ttp-dispatched` for the same id, and so the eventual
     * `ttp-executed` resolves the entry in place. Oldest-first so the prepend
     * leaves the most recently dispatched on top.
     */
    backfillPending(records: PendingRecord[]): void {
        const ordered = [...records].sort((a, b) => a.timestampMs - b.timestampMs);
        for (const r of ordered) {
            this.addTtpAction({
                id: r.id,
                ttpId: r.ttpId,
                ttpName: r.ttpName,
                targetId: r.targetId,
                targetName: r.targetName,
                execSystemId: r.execSystemId,
                execSystemName: r.execSystemName,
                status: 'pending',
                timestamp: new Date(r.timestampMs)
            });
        }
    }

    /**
     * Add durable kubeconfig loads that happened before the frontend connected.
     * Startup groups live at the oldest end of the newest-first store and use
     * stable backend IDs, so repeated campaign-state refreshes are idempotent.
     */
    backfillBootstrap(operations: BootstrapOperation[]): void {
        for (const operation of [...operations].sort((a, b) => b.id.localeCompare(a.id))) {
            if (this.index.has(operation.id)) continue;

            const group: ActionGroup = {
                kind: 'action-group',
                action: {
                    kind: 'ttp-action',
                    id: operation.id,
                    ttpId: '',
                    ttpName: operation.name,
                    targetId: '',
                    targetName: '',
                    status: 'success',
                    startup: true,
                    detail: operation.detail
                },
                effects: [],
                collapsed: true
            };
            this.topEntries = [...this.topEntries, group];
            const proxy = this.topEntries[this.topEntries.length - 1] as ActionGroup;
            this.index.set(operation.id, proxy);

            for (const effect of operation.effects) {
                if (this.seenEntityIds.has(effect.entityId)) continue;
                this.seenEntityIds.add(effect.entityId);
                proxy.effects.push({
                    kind: effect.category === 'credential' ? 'credential' : 'discovery',
                    id: effect.entityId,
                    entityId: effect.entityId,
                    entityName: effect.entityName,
                    entityKind: effect.entityKind
                });
            }
        }
    }

    toggleGroup(cmdId: string): void {
        for (const entry of this.topEntries) {
            if (entry.kind === 'action-group' && entry.action.id === cmdId) {
                entry.collapsed = !entry.collapsed;
                return;
            }
        }
    }

    clear(): void {
        this.topEntries = [];
        this.index.clear();
        this.seenEntityIds.clear();
    }
}

export const timeline = new TimelineStore();
