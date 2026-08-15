export async function saveFile(data: string, filename: string, mimeType: string): Promise<void> {
	// Check if the File System Access API is available (Chrome)
	if ('showSaveFilePicker' in window) {
		console.debug('Using File System Access API to save file');
		try {
			const handle = await saveFilePicker(filename);
			const writable = await handle.createWritable();
			await writable.write(data);
			await writable.close();
			return;
		} catch (err) {
			// User cancelled or API failed, fall through to traditional download
			if ((err as Error).name === 'AbortError') {
				return;
			}
		}
	} else {
		console.debug('File System Access API not available, using traditional download');

		// Traditional download fallback
		const blob = new Blob([data], { type: mimeType });
		const url = URL.createObjectURL(blob);
		const a = document.createElement('a');
		a.href = url;
		a.download = filename;
		document.body.appendChild(a);
		a.click();
		document.body.removeChild(a);
		URL.revokeObjectURL(url);
	}
}

export async function saveFilePicker(filename: string): Promise<FileSystemFileHandle> {
	const options = {
		suggestedName: filename
	};
	return await (window as any).showSaveFilePicker(options);
}
