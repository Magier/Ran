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
    timestamp: Date;
};

export type EntityEntry = {
    kind: 'discovery' | 'credential' | 'access-gained';
    id: string;
    entityId: string;
    entityName: string;
    entityKind: string;
    cmdId?: string;
    timestamp: Date;
};

export type ActionGroup = {
    kind: 'action-group';
    action: TtpActionEntry;
    effects: EntityEntry[];
    collapsed: boolean;
    score?: number;
};

export type TopEntry = ActionGroup | EntityEntry;

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
