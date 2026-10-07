import type { TTP } from '$lib/api/index';

const FIELD_KIND_EXCLUDE: Record<string, string[]> = {
	service_account_name: ['ServiceAccount']
};

type EffectObservation = 'executor' | 'identity' | 'target-local';

interface EffectFieldMapping {
	fields: string[];
	observation: EffectObservation;
}

const EFFECT_FIELD_MAP: Record<string, EffectFieldMapping> = {
	'linux.mounts': { fields: ['mounts'], observation: 'executor' },
	'sys.envVar': { fields: ['envVars'], observation: 'executor' },
	'sys.ip': { fields: ['ips'], observation: 'executor' },
	'sys.files': { fields: ['files', 'binaries'], observation: 'executor' },
	'sys.userID': { fields: ['user_id'], observation: 'executor' },
	rawServiceaccountToken: { fields: ['service_account_name'], observation: 'target-local' },
	'k8s.SelfSubjectRulesReview': { fields: ['can'], observation: 'identity' }
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

function effectObservesSelectedEntity(ttp: TTP, declaration: EffectDeclaration): boolean {
	if (ttp.procedures.length === 0) return false;
	if (declaration.subject === 'target') return true;
	if (declaration.subject === 'executor') return runsOnlyOnSelectedTarget(ttp);
	return (
		EFFECT_FIELD_MAP[declaration.kind]?.observation === 'identity' || runsOnlyOnSelectedTarget(ttp)
	);
}

export function quickActionsForField(label: string, kind: string | undefined, ttps: TTP[]): TTP[] {
	if (kind && (FIELD_KIND_EXCLUDE[label] ?? []).includes(kind)) return [];

	return ttps.filter((ttp) =>
		(ttp.effects ?? []).some((effect) => {
			const declaration = parseEffectDeclaration(effect);
			return (
				effectObservesSelectedEntity(ttp, declaration) &&
				(EFFECT_FIELD_MAP[declaration.kind]?.fields ?? []).includes(label)
			);
		})
	);
}

export function quickActionFields(kind: string | undefined, ttps: TTP[]): Set<string> {
	const fields = new Set<string>();
	for (const field of Object.values(EFFECT_FIELD_MAP).flatMap((mapping) => mapping.fields)) {
		if (quickActionsForField(field, kind, ttps).length > 0) fields.add(field);
	}
	return fields;
}
