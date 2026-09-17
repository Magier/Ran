import { describe, expect, it, vi } from 'vitest';
import { RanAPI } from './ran_api';

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
});
