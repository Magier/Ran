export type ActionLogEntry = {
    id: string;
    ttpId: string;
    ttpName: string;
    targetId: string;
    targetName: string;
    status: 'pending' | 'success' | 'failed';
    failReason?: string;
    startedAt: Date;
};

export class ActionLogStore {
    entries = $state<ActionLogEntry[]>([]);
    drawerOpen = $state(false);

    get pendingCount(): number {
        return this.entries.filter((e) => e.status === 'pending').length;
    }

    addEntry(entry: ActionLogEntry): void {
        this.entries = [entry, ...this.entries];
    }

    resolveEntry(ttpId: string, success: boolean, failReason?: string): void {
        const entry = this.entries.find((e) => e.ttpId === ttpId && e.status === 'pending');
        if (!entry) return;
        entry.status = success ? 'success' : 'failed';
        if (!success && failReason !== undefined) entry.failReason = failReason;
    }

    clear(): void {
        this.entries = [];
    }
}

export const actionLog = new ActionLogStore();
