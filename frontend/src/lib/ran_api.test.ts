import { describe, expect, it, vi } from 'vitest';
import { RanAPI } from './ran_api';

describe('RanAPI event subscriptions', () => {
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
