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
    open = $state(false);

    private index = new Map<string, ActionGroup>();
    private seenEntityIds = new Set<string>();

    pendingCount = $derived(
        this.topEntries.filter(
            (e): e is ActionGroup => e.kind === 'action-group' && e.action.status === 'pending'
        ).length
    );

    addTtpAction(entry: Omit<TtpActionEntry, 'kind'>): void {
        const group: ActionGroup = {
            kind: 'action-group',
            action: { kind: 'ttp-action', ...entry },
            effects: [],
            collapsed: true
        };
        this.topEntries = [group, ...this.topEntries];
        // Store the reactive proxy from topEntries (index 0) so mutations propagate
        this.index.set(entry.id, this.topEntries[0] as ActionGroup);
    }

    addEntityEvent(entry: EntityEntry): void {
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

    resolveTtpAction(id: string, success: boolean, failReason?: string): void {
        const group = this.index.get(id);
        if (!group || group.action.status !== 'pending') return;
        group.action.status = success ? 'success' : 'failed';
        if (!success && failReason !== undefined) group.action.failReason = failReason;
    }

    toggleGroup(cmdId: string): void {
        const group = this.index.get(cmdId);
        if (!group) return;
        group.collapsed = !group.collapsed;
    }

    clear(): void {
        this.topEntries = [];
        this.index.clear();
        this.seenEntityIds.clear();
    }
}

export const timeline = new TimelineStore();
