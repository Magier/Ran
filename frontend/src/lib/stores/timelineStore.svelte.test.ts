import { describe, it, expect, beforeEach } from 'vitest';
import {
    TimelineStore,
    type TtpActionEntry,
    type EntityEntry,
    type ActionGroup,
    type TopEntry
} from '$lib/stores/timelineStore.svelte';

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
        expect(store.topEntries).toHaveLength(0);
        expect(store.open).toBe(false);
        expect(store.pendingCount).toBe(0);
    });

    // addTtpAction
    it('addTtpAction creates an ActionGroup prepended to topEntries', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        expect(store.topEntries).toHaveLength(1);
        const entry = store.topEntries[0];
        expect(entry.kind).toBe('action-group');
        if (entry.kind === 'action-group') {
            expect(entry.action.id).toBe('cmd-1');
            expect(entry.effects).toHaveLength(0);
            expect(entry.collapsed).toBe(true);
        }
    });

    it('addTtpAction prepends newest first', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2' }));
        expect(store.topEntries[0].kind).toBe('action-group');
        if (store.topEntries[0].kind === 'action-group') {
            expect(store.topEntries[0].action.id).toBe('cmd-2');
        }
    });

    // addEntityEvent — grouping
    it('addEntityEvent with matching cmdId appends to group effects', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: 'cmd-abc' }));
        expect(store.topEntries).toHaveLength(1); // still one top-level entry
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.effects).toHaveLength(1);
            expect(entry.effects[0].entityName).toBe('web-app');
        }
    });

    it('addEntityEvent without cmdId prepends as standalone', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: undefined }));
        expect(store.topEntries).toHaveLength(2);
        expect(store.topEntries[0].kind).toBe('discovery');
    });

    it('addEntityEvent with unmatched cmdId prepends as standalone', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.addEntityEvent(makeEntityEntry({ id: 'x', entityId: 'x', cmdId: 'cmd-unknown' }));
        expect(store.topEntries).toHaveLength(2);
        expect(store.topEntries[0].kind).toBe('discovery');
    });

    // deduplication
    it('addEntityEvent deduplicates standalone entries by id', () => {
        store.addEntityEvent(makeEntityEntry({ id: 'pod-a', entityId: 'pod-a', cmdId: undefined }));
        store.addEntityEvent(makeEntityEntry({ id: 'pod-a', entityId: 'pod-a', cmdId: undefined }));
        expect(store.topEntries).toHaveLength(1);
    });

    it('addEntityEvent deduplicates group effects by id', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: 'cmd-abc' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: 'cmd-abc' })); // same id
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.effects).toHaveLength(1);
        }
    });

    it('addEntityEvent does not suppress entity when a group has the same id as the entity', () => {
        store.addTtpAction(makeTtpEntry({ id: 'ns/default/pod/web-app' }));
        store.addEntityEvent(makeEntityEntry({ id: 'ns/default/pod/web-app', entityId: 'ns/default/pod/web-app', cmdId: undefined }));
        expect(store.topEntries).toHaveLength(2);
        expect(store.topEntries[0].kind).toBe('discovery');
    });

    // pendingCount
    it('pendingCount counts only pending action groups', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1', status: 'pending' }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2', status: 'pending' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: undefined }));
        expect(store.pendingCount).toBe(2);
        store.resolveTtpAction('cmd-1', true);
        expect(store.pendingCount).toBe(1);
    });

    // resolveTtpAction
    it('resolveTtpAction marks matching group action as success', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-abc', true);
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.action.status).toBe('success');
            expect(entry.action.failReason).toBeUndefined();
        }
    });

    it('resolveTtpAction marks entry as failed with reason', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-abc', false, 'permission denied');
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.action.status).toBe('failed');
            expect(entry.action.failReason).toBe('permission denied');
        }
    });

    it('resolveTtpAction with unknown id is a no-op', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'pending' }));
        store.resolveTtpAction('cmd-unknown', true);
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.action.status).toBe('pending');
        }
    });

    it('resolveTtpAction on already-resolved entry is a no-op', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc', status: 'success' }));
        store.resolveTtpAction('cmd-abc', false, 'should not change');
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.action.status).toBe('success');
        }
    });

    // toggleGroup
    it('toggleGroup flips collapsed from true to false', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.toggleGroup('cmd-abc');
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.collapsed).toBe(false);
        }
    });

    it('toggleGroup flips collapsed from false to true', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        store.toggleGroup('cmd-abc'); // false
        store.toggleGroup('cmd-abc'); // true
        const entry = store.topEntries[0];
        if (entry.kind === 'action-group') {
            expect(entry.collapsed).toBe(true);
        }
    });

    it('toggleGroup with unknown id is a no-op', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-abc' }));
        expect(() => store.toggleGroup('cmd-unknown')).not.toThrow();
    });

    // clear
    it('clear removes all entries', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addEntityEvent(makeEntityEntry({ cmdId: undefined }));
        store.clear();
        expect(store.topEntries).toHaveLength(0);
        // index is also cleared: new entity with old cmdId goes to standalone
        store.addEntityEvent(makeEntityEntry({ cmdId: 'cmd-1' }));
        expect(store.topEntries).toHaveLength(1);
        expect(store.topEntries[0].kind).toBe('discovery');
    });

    // mixed ordering
    it('mixed entries interleave by insertion order, newest first', () => {
        store.addTtpAction(makeTtpEntry({ id: 'cmd-1' }));
        store.addEntityEvent(makeEntityEntry({ id: 'pod-a', entityId: 'pod-a', cmdId: undefined }));
        store.addTtpAction(makeTtpEntry({ id: 'cmd-2' }));
        const ids = store.topEntries.map((e) =>
            e.kind === 'action-group' ? e.action.id : e.id
        );
        expect(ids).toEqual(['cmd-2', 'pod-a', 'cmd-1']);
    });
});
