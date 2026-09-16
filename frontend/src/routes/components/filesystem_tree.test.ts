import { describe, expect, it } from 'vitest';
import {
	filesystemChildren,
	filesystemVolumeMountsAt,
	normalizeFilesystemPath
} from './filesystem_tree';

describe('filesystem tree', () => {
	it('returns only immediate children with directories first', () => {
		expect(
			filesystemChildren(
				'/',
				['/etc/hosts', '/README', '/var/log/app.log'],
				['/var', '/etc', '/var/log']
			)
		).toEqual([
			{ path: '/etc', name: 'etc', kind: 'directory' },
			{ path: '/var', name: 'var', kind: 'directory' },
			{ path: '/README', name: 'README', kind: 'file' }
		]);
	});

	it('normalizes trailing directory separators', () => {
		expect(normalizeFilesystemPath('/var/log///')).toBe('/var/log');
		expect(normalizeFilesystemPath('/')).toBe('/');
	});

	it('lets a discovered directory override an older file classification', () => {
		expect(filesystemChildren('/var', ['/var/run'], ['/var/run'])).toEqual([
			{ path: '/var/run', name: 'run', kind: 'directory' }
		]);
	});

	it('builds directory ancestors for mount points before files are scanned', () => {
		expect(filesystemChildren('/', [], [], ['/var/run/secrets'])).toEqual([
			{ path: '/var', name: 'var', kind: 'directory' }
		]);
		expect(filesystemChildren('/var', [], [], ['/var/run/secrets'])).toEqual([
			{ path: '/var/run', name: 'run', kind: 'directory' }
		]);
	});

	it('matches volume mounts by their exact normalized mount point', () => {
		const mounts = [
			{
				mountPoint: '/var/data/',
				name: 'data',
				container: 'app',
				readOnly: false,
				isHostPath: false,
				origin: 'configured' as const
			}
		];

		expect(filesystemVolumeMountsAt('/var/data', mounts)).toEqual(mounts);
		expect(filesystemVolumeMountsAt('/var', mounts)).toEqual([]);
	});
});
