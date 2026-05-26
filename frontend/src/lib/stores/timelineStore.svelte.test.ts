import { describe, it, expect, beforeEach } from 'vitest';
import { TimelineStore, type TtpActionEntry, type EntityEntry } from '$lib/stores/timelineStore.svelte';

function makeTtpEntry(overrides: Partial<Omit<TtpActionEntry, 'kind'>> = {}): Omit<TtpActionEntry, 'kind'> {
    return {
        id: 'cmd-abc',
        ttpId: 'list-env',
        ttpName: 'List Environment Variables',
        targetId: 'pod-1',
        targetName: 'my-pod',
        status: 'pending',
        timestamp: new Date('2026-05-25T10:00:00Z'),
        ...overrides
    };
}

function makeEntityEntry(overrides: Partial<EntityEntry> = {}): EntityEntry {
    return {
        kind: 'discovery',
        id: 'ns/default/pod/web-app',
        entityId: 'ns/default/pod/web-app',
        entityName: 'web-app',
        entityKind: 'Pod',
        timestamp: new Date('2026-05-25T10:01:00Z'),
        ...overrides
    };
}

describe('TimelineStore', () => {
    let store: TimelineStore;

    beforeEach(() => {
        store = new TimelineStore();
    });

    it('starts empty with timeline closed', () => {
        expect(store.entries).toHaveLength(0);
        expect(store.open).toBe(false);
        expect(store.pendingCount).toBe(0);
    });

    it('addTtpAction prepends entry with kind ttp-action (newest first)', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2' }));
        expect(store.entries).toHaveLength(2);
        expect(store.entries[0].id).toBe('cmd-2');
        expect(store.entries[0].kind).toBe('ttp-action');
        expect(store.entries[1].id).toBe('cmd-1');
    });

    it('addEntityEvent prepends discovery entries', () => {
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/web-app' }));
        expect(store.entries).toHaveLength(1);
        expect(store.entries[0].kind).toBe('discovery');
    });

    it('addEntityEvent prepends credential entries', () => {
        store.addEntityEvent(makeEntityEntry({ kind: 'credential', entityId: 'ns/default/secret/db-pass', id: 'ns/default/secret/db-pass', entityKind: 'Secret', entityName: 'db-pass' }));
        expect(store.entries[0].kind).toBe('credential');
    });

    it('addEntityEvent deduplicates by entityId', () => {
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/web-app' }));
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/web-app' }));
        expect(store.entries).toHaveLength(1);
    });

    it('addEntityEvent does not deduplicate different entityIds', () => {
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/web-app', id: 'ns/default/pod/web-app' }));
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/api', id: 'ns/default/pod/api' }));
        expect(store.entries).toHaveLength(2);
    });

    it('pendingCount counts only pending ttp-action entries', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1', status: 'pending' }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2', status: 'pending' }));
        store.addEntityEvent(makeEntityEntry());
        expect(store.pendingCount).toBe(2);
        store.resolveTtpAction('cmd-1', true);
        expect(store.pendingCount).toBe(1);
    });

    it('resolveTtpAction marks matching entry as success', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-abc', true);
        const entry = store.entries[0];
        expect(entry.kind).toBe('ttp-action');
        if (entry.kind === 'ttp-action') {
            expect(entry.status).toBe('success');
            expect(entry.failReason).toBeUndefined();
        }
    });

    it('resolveTtpAction marks entry as failed with reason', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-abc', false, 'permission denied');
        const entry = store.entries[0];
        if (entry.kind === 'ttp-action') {
            expect(entry.status).toBe('failed');
            expect(entry.failReason).toBe('permission denied');
        }
    });

    it('resolveTtpAction with unknown id is a no-op', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-unknown', true);
        const entry = store.entries[0];
        if (entry.kind === 'ttp-action') {
            expect(entry.status).toBe('pending');
        }
    });

    it('resolveTtpAction on already-resolved entry is a no-op', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'success' }));
        store.resolveTtpAction('cmd-abc', false, 'should not change');
        const entry = store.entries[0];
        if (entry.kind === 'ttp-action') {
            expect(entry.status).toBe('success');
        }
    });

    it('clear removes all entries', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addEntityEvent(makeEntityEntry());
        store.clear();
        expect(store.entries).toHaveLength(0);
    });

    it('addEntityEvent does not suppress entity when a ttp-action has the same id', () => {
        // Add a ttp-action whose cmd id happens to equal an entity id
        store.addTtpAction(makeTtpEntry({ id: 'ns/default/pod/web-app' }));
        // Entity with same id should still be added (different kind)
        store.addEntityEvent(makeEntityEntry({ entityId: 'ns/default/pod/web-app', id: 'ns/default/pod/web-app' }));
        expect(store.entries).toHaveLength(2);
        expect(store.entries.some((e) => e.kind === 'discovery')).toBe(true);
    });

    it('mixed entries interleave by insertion order, newest first', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addEntityEvent(makeEntityEntry({ entityId: 'pod-a', id: 'pod-a' }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2' }));
        expect(store.entries[0].id).toBe('cmd-2');
        expect(store.entries[1].id).toBe('pod-a');
        expect(store.entries[2].id).toBe('cmd-1');
    });
});
