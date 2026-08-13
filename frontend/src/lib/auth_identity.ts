export interface AuthIdentityRef {
	id: string;
}

export function selectDefaultAuthIdentity(
	identities: AuthIdentityRef[],
	targetId: string,
	bestServiceAccountId?: string
): string {
	const targetIdentity = identities.find((identity) => identity.id === targetId);
	if (targetIdentity) return targetIdentity.id;
	if (identities.length === 1) return identities[0].id;
	if (bestServiceAccountId && identities.some((identity) => identity.id === bestServiceAccountId)) {
		return bestServiceAccountId;
	}
	return '';
}
