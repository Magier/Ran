import { describe, it, expect, beforeEach } from 'vitest';
import { ActionLogStore, type ActionLogEntry } from '$lib/stores/actionLogStore.svelte';

function makeEntry(overrides: Partial<ActionLogEntry> = {}): ActionLogEntry {
    return {
        id: 'test-id',
        ttpId: 'list-env',
        ttpName: 'List Environment Variables',
        targetId: 'pod-1',
        targetName: 'my-pod',
        status: 'pending',
        startedAt: new Date('2026-05-22T10:00:00Z'),
        ...overrides
    };
}

describe('ActionLogStore', () => {
    let store: ActionLogStore;

    beforeEach(() => {
        store = new ActionLogStore();
    });

    it('starts empty with drawer closed', () => {
        expect(store.entries).toHaveLength(0);
        expect(store.drawerOpen).toBe(false);
        expect(store.pendingCount).toBe(0);
    });

    it('addEntry prepends entries (newest first)', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'action-a' }));
        store.addEntry(makeEntry({ id: 'b', ttpId: 'action-b' }));
        expect(store.entries).toHaveLength(2);
        expect(store.entries[0].id).toBe('b');
        expect(store.entries[1].id).toBe('a');
    });

    it('pendingCount counts only pending entries', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'action-a', status: 'pending' }));
        store.addEntry(makeEntry({ id: 'b', ttpId: 'action-b', status: 'pending' }));
        expect(store.pendingCount).toBe(2);
        store.resolveEntry('action-a', true);
        expect(store.pendingCount).toBe(1);
    });

    it('resolveEntry marks the most recent pending entry for the ttpId as success', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'list-env', status: 'pending' }));
        store.resolveEntry('list-env', true);
        expect(store.entries[0].status).toBe('success');
        expect(store.entries[0].failReason).toBeUndefined();
    });

    it('resolveEntry marks entry as failed and stores failReason', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'list-env', status: 'pending' }));
        store.resolveEntry('list-env', false, 'permission denied');
        expect(store.entries[0].status).toBe('failed');
        expect(store.entries[0].failReason).toBe('permission denied');
    });

    it('resolveEntry with unknown ttpId is a no-op', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'list-env', status: 'pending' }));
        store.resolveEntry('unknown-ttp', true);
        expect(store.entries[0].status).toBe('pending');
    });

    it('resolveEntry on already-resolved entry is a no-op', () => {
        store.addEntry(makeEntry({ id: 'a', ttpId: 'list-env', status: 'success' }));
        store.resolveEntry('list-env', false, 'should not change');
        expect(store.entries[0].status).toBe('success');
    });

    it('clear removes all entries', () => {
        store.addEntry(makeEntry({ id: 'a' }));
        store.addEntry(makeEntry({ id: 'b' }));
        store.clear();
        expect(store.entries).toHaveLength(0);
    });
});
