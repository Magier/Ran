export type TtpActionEntry = {
    kind: 'ttp-action';
    id: string;
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
    id: string;
    entityId: string;
    entityName: string;
    entityKind: string;
    timestamp: Date;
};

export type TimelineEntry = TtpActionEntry | EntityEntry;

export class TimelineStore {
    entries = $state<TimelineEntry[]>([]);
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
