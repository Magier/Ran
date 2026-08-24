import { describe, expect, it } from 'vitest';

import {
	buildPlanDownload,
	defaultPlanDescription,
	defaultPlanName,
	planFilename
} from './planDownload';

describe('buildPlanDownload', () => {
	it('keeps plan YAML intact and creates a portable filename', () => {
		const yaml = 'id: exported-plan\nsteps: []\n';
		expect(buildPlanDownload(yaml, undefined, new Date('2026-08-24T09:10:11.123Z'))).toEqual({
			data: yaml,
			filename: 'execution-2026-08-24t09-10-11-123z.plan.yaml',
			mimeType: 'application/yaml'
		});
	});

	it('uses a filesystem-safe plan name', () => {
		expect(buildPlanDownload('', 'My First / Plan').filename).toBe('my-first-plan.plan.yaml');
	});

	it('derives and normalizes plan filenames', () => {
		expect(planFilename('My First / Plan')).toBe('my-first-plan.plan.yaml');
		expect(buildPlanDownload('', 'Ignored', new Date(), 'Custom File').filename).toBe(
			'custom-file.plan.yaml'
		);
	});

	it('provides a timestamped default plan name', () => {
		expect(defaultPlanName(new Date('2026-08-24T09:10:11.123Z'))).toBe(
			'execution_2026-08-24T09-10-11-123Z'
		);
	});

	it('provides a timestamped default description', () => {
		expect(defaultPlanDescription(new Date('2026-08-24T09:10:11.123Z'))).toBe(
			'Plan exported from campaign execution history on 2026-08-24T09:10:11.123Z.'
		);
	});
});
