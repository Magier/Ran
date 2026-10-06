import type { TTP } from '$lib/api/index';

const FIELD_KIND_EXCLUDE: Record<string, string[]> = {
	service_account_name: ['ServiceAccount']
};

const EFFECT_FIELD_MAP: Record<string, string[]> = {
	'linux.mounts': ['mounts'],
	'sys.envVar': ['envVars'],
	'sys.ip': ['ips'],
	'sys.files': ['files', 'binaries'],
	'sys.userID': ['user_id'],
	rawServiceaccountToken: ['service_account_name'],
	'k8s.SelfSubjectRulesReview': ['can']
};

type EffectSubject = 'executor' | 'target' | undefined;

interface EffectDeclaration {
	subject: EffectSubject;
	kind: string;
}

function parseEffectDeclaration(effect: string): EffectDeclaration {
	const match = effect.trim().match(/^(executor|target)::(.*)$/);
	const subject = match?.[1] as EffectSubject;
	const expression = (match?.[2] ?? effect).trim();
	const argumentStart = expression.indexOf('(');
	return {
		subject,
		kind: (argumentStart >= 0 ? expression.slice(0, argumentStart) : expression).trim()
	};
}

/**
 * Executor-bound and legacy shortcuts must execute on the selected entity,
 * not the operator host or another source. Explicit target-bound effects may
 * observe that entity independently of placement, but still need a procedure.
 */
function runsOnlyOnSelectedTarget(ttp: TTP): boolean {
	return (
		ttp.procedures.length > 0 &&
		ttp.procedures.every((procedure) => {
			if (procedure.runOnTarget === true) return true;
			if (procedure.runOnTarget === false || procedure.isLocalCommand === true) return false;
			if (procedure.http_request !== undefined || procedure.k8s_request !== undefined) return false;
			return !procedure.command.includes('${K8S_AUTH}') && !procedure.command.includes('kubectl ');
		})
	);
}

function effectObservesSelectedEntity(ttp: TTP, subject: EffectSubject): boolean {
	return ttp.procedures.length > 0 && (subject === 'target' || runsOnlyOnSelectedTarget(ttp));
}

export function quickActionsForField(label: string, kind: string | undefined, ttps: TTP[]): TTP[] {
	if (kind && (FIELD_KIND_EXCLUDE[label] ?? []).includes(kind)) return [];

	return ttps.filter((ttp) =>
		(ttp.effects ?? []).some((effect) => {
			const declaration = parseEffectDeclaration(effect);
			return (
				effectObservesSelectedEntity(ttp, declaration.subject) &&
				(EFFECT_FIELD_MAP[declaration.kind] ?? []).includes(label)
			);
		})
	);
}

export function quickActionFields(kind: string | undefined, ttps: TTP[]): Set<string> {
	const fields = new Set<string>();
	for (const field of Object.values(EFFECT_FIELD_MAP).flat()) {
		if (quickActionsForField(field, kind, ttps).length > 0) fields.add(field);
	}
	return fields;
}
