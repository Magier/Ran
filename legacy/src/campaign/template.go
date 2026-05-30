package campaign

import "strings"

const NS_NAME_VAR = "${NS}"
const POD_NAME_VAR = "${POD_NAME}"
const NODE_NAME_VAR = "${NODE_NAME}"
const RANDOM_VAR = "${RANDOM}"

func IsTemplateVariable(variable string) bool {
	return strings.HasPrefix(variable, "${") && strings.HasSuffix(variable, "}")
}
