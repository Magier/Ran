export type FilesystemEntry = {
	path: string;
	name: string;
	kind: 'directory' | 'file';
};

export type FilesystemVolumeMount = {
	mountPoint: string;
	name: string;
	container?: string;
	readOnly: boolean;
	isHostPath: boolean;
	hostPath?: string;
	mountType?: string;
	origin: 'configured' | 'runtime';
};

export function normalizeFilesystemPath(path: string): string {
	if (path === '/') return path;
	return path.replace(/\/+$/, '');
}

function parentPath(path: string): string {
	const normalized = normalizeFilesystemPath(path);
	const separator = normalized.lastIndexOf('/');
	if (separator <= 0) return normalized.startsWith('/') ? '/' : '.';
	return normalized.slice(0, separator);
}

function baseName(path: string): string {
	const normalized = normalizeFilesystemPath(path);
	return normalized.slice(normalized.lastIndexOf('/') + 1) || '/';
}

export function filesystemChildren(
	directory: string,
	files: string[],
	directories: string[],
	mountPoints: string[] = []
): FilesystemEntry[] {
	const parent = normalizeFilesystemPath(directory);
	const entries = new Map<string, FilesystemEntry>();
	const knownDirectories = new Set(directories.map(normalizeFilesystemPath));

	for (const mountPoint of mountPoints) {
		let path = normalizeFilesystemPath(mountPoint);
		while (path.startsWith('/') && path !== '/') {
			knownDirectories.add(path);
			path = parentPath(path);
		}
	}

	for (const normalized of knownDirectories) {
		if (normalized !== parent && parentPath(normalized) === parent) {
			entries.set(normalized, { path: normalized, name: baseName(normalized), kind: 'directory' });
		}
	}
	for (const path of files) {
		const normalized = normalizeFilesystemPath(path);
		if (parentPath(normalized) === parent && !entries.has(normalized)) {
			entries.set(normalized, { path: normalized, name: baseName(normalized), kind: 'file' });
		}
	}

	return [...entries.values()].sort(
		(a, b) =>
			Number(b.kind === 'directory') - Number(a.kind === 'directory') ||
			a.name.localeCompare(b.name)
	);
}

export function filesystemVolumeMountsAt(
	path: string,
	volumeMounts: FilesystemVolumeMount[]
): FilesystemVolumeMount[] {
	const normalized = normalizeFilesystemPath(path);
	return volumeMounts.filter((mount) => normalizeFilesystemPath(mount.mountPoint) === normalized);
}
