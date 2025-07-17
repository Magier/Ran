package campaign

import "strings"

const POD_NAME_VAR = "${POD_NAME}"
const NODE_NAME_VAR = "${NODE_NAME}"

func IsTemplateVariable(variable string) bool {
	return strings.HasPrefix(variable, "${") && strings.HasSuffix(variable, "}")
}
