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

/**
 * EntityInfo shortcuts are direct observations of the selected entity. An
 * action that executes from another system remains applicable in the Armory,
 * but must not be offered as an inline field action for this entity.
 */
function runsOnlyOnSelectedTarget(ttp: TTP): boolean {
	return (
		ttp.procedures.length > 0 &&
		ttp.procedures.every((procedure) => procedure.runOnTarget !== false)
	);
}

export function quickActionsForField(label: string, kind: string | undefined, ttps: TTP[]): TTP[] {
	if (kind && (FIELD_KIND_EXCLUDE[label] ?? []).includes(kind)) return [];

	return ttps.filter(
		(ttp) =>
			runsOnlyOnSelectedTarget(ttp) &&
			(ttp.effects ?? []).some((effect) => (EFFECT_FIELD_MAP[effect] ?? []).includes(label))
	);
}

export function quickActionFields(kind: string | undefined, ttps: TTP[]): Set<string> {
	const fields = new Set<string>();
	for (const field of Object.values(EFFECT_FIELD_MAP).flat()) {
		if (quickActionsForField(field, kind, ttps).length > 0) fields.add(field);
	}
	return fields;
}
