import { afterEach, describe, expect, it, vi } from 'vitest';
import { RanAPI } from './ran_api';

class FakeEventSource {
	static readonly CONNECTING = 0;
	static readonly OPEN = 1;
	static readonly CLOSED = 2;
	static instances: FakeEventSource[] = [];

	readyState = FakeEventSource.CONNECTING;
	onopen: ((event: Event) => void) | null = null;
	onmessage: ((event: MessageEvent) => void) | null = null;
	onerror: ((event: Event) => void) | null = null;
	listeners = new Set<string>();

	constructor(readonly url: string) {
		FakeEventSource.instances.push(this);
	}

	addEventListener(type: string) {
		this.listeners.add(type);
	}

	close() {
		this.readyState = FakeEventSource.CLOSED;
	}

	open() {
		this.readyState = FakeEventSource.OPEN;
		this.onopen?.(new Event('open'));
	}

	fail() {
		this.readyState = FakeEventSource.CONNECTING;
		this.onerror?.(new Event('error'));
	}
}

afterEach(() => {
	vi.useRealTimers();
	vi.unstubAllGlobals();
	FakeEventSource.instances = [];
});

describe('RanAPI event subscriptions', () => {
	it('notifies listeners when the backend connection changes state', () => {
		const api = new RanAPI();
		const statefulApi = api as unknown as {
			setConnectionState(state: 'connecting' | 'connected' | 'disconnected'): void;
		};
		const listener = vi.fn();
		const unsubscribe = api.onConnectionStateChange(listener);

		expect(listener).toHaveBeenLastCalledWith('connecting');

		statefulApi.setConnectionState('disconnected');
		statefulApi.setConnectionState('connected');

		expect(listener.mock.calls).toEqual([['connecting'], ['disconnected'], ['connected']]);

		unsubscribe();
		statefulApi.setConnectionState('disconnected');
		expect(listener).toHaveBeenCalledTimes(3);
	});

	it('keeps independent handlers for the same event type', () => {
		const api = new RanAPI();
		const first = vi.fn();
		const second = vi.fn();
		api.on('ttp-executed', first);
		api.on('ttp-executed', second);

		(api as any).handleSSEMessage(
			new MessageEvent('ttp-executed', {
				data: JSON.stringify({ type: 'ttp-executed', data: { CmdId: 'cmd-1' } })
			})
		);

		expect(first).toHaveBeenCalledWith({ CmdId: 'cmd-1' });
		expect(second).toHaveBeenCalledWith({ CmdId: 'cmd-1' });

		api.off('ttp-executed', first);
		(api as any).handleSSEMessage(
			new MessageEvent('ttp-executed', {
				data: JSON.stringify({ type: 'ttp-executed', data: { CmdId: 'cmd-2' } })
			})
		);

		expect(first).toHaveBeenCalledTimes(1);
		expect(second).toHaveBeenLastCalledWith({ CmdId: 'cmd-2' });
	});

	it('replaces a stuck EventSource and reattaches named listeners', async () => {
		vi.useFakeTimers();
		vi.stubGlobal('EventSource', FakeEventSource);
		const api = new RanAPI();
		const states: string[] = [];
		api.onConnectionStateChange((state) => states.push(state));
		api.on('ttp-executed', vi.fn());

		const initialConnection = api.connect('/events');
		const first = FakeEventSource.instances[0];
		first.open();
		await initialConnection;
		expect(first.listeners.has('ttp-executed')).toBe(true);

		first.fail();
		expect(states.at(-1)).toBe('disconnected');
		await vi.advanceTimersByTimeAsync(1_000);

		const replacement = FakeEventSource.instances[1];
		expect(first.readyState).toBe(FakeEventSource.CLOSED);
		expect(replacement.url).toBe('/events');
		replacement.open();

		expect(replacement.listeners.has('ttp-executed')).toBe(true);
		expect(states.at(-1)).toBe('connected');
	});
});
