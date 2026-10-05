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
 * EntityInfo shortcuts are direct observations of the selected entity.
 * Executor-bound and legacy effects qualify only when execution stays on the
 * selected entity. Target-bound effects qualify independently of placement.
 */
function runsOnlyOnSelectedTarget(ttp: TTP): boolean {
	return (
		ttp.procedures.length > 0 &&
		ttp.procedures.every((procedure) => procedure.runOnTarget !== false)
	);
}

function effectObservesSelectedEntity(ttp: TTP, subject: EffectSubject): boolean {
	return subject === 'target' || runsOnlyOnSelectedTarget(ttp);
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
