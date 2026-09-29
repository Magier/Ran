const SENSITIVE_ARGUMENT = /(auth|credential|jwt|password|secret|token)/i;
const JWT = /^eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/;

export function actionDisplayName(
	title: string | undefined,
	ttpName: string,
	args: Record<string, string> = {}
): string {
	if (!title?.trim()) return ttpName;

	return title.replace(
		/\$\{([A-Za-z_][A-Za-z0-9_]*)(?:\s*\|\|\s*([^}]*?))?\}/g,
		(placeholder, name: string, fallback: string | undefined) => {
			const value = args[name]?.trim();
			if (value && displayArgumentValue(name, value) !== '[redacted]') return value;
			return fallback?.trim() || placeholder;
		}
	);
}

export function displayArgumentValue(name: string, value: string): string {
	if (SENSITIVE_ARGUMENT.test(name) || JWT.test(value.trim())) return '[redacted]';
	return value;
}
