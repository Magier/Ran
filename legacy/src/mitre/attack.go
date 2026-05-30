package mitre

type Tactic string

const (
	Reconnaissance      = "Reconnaissance"       // TA0043
	ResourceDevelopment = "Resource Development" // TA0042
	InitialAccess       = "Initial Access"       // TA0001
	Execution           = "Execution"            // TA0002
	Persistence         = "Persistence"          // TA0003
	PrivilegeEscalation = "Privilege Escalation" // TA0004
	DefenseEvasion      = "Defense Evasion"      // TA0005
	CredentialAccess    = "Credential Access"    // TA0006
	Discovery           = "Discovery"            // TA0007
	LateralMovement     = "Lateral Movement"     // TA0008
	Collection          = "Collection"           // TA0009
	CommandAndControl   = "Command And Control"  // TA0011
	Exfiltration        = "Exfiltration"         // TA0010
	Impact              = "Impact"               // TA0040
)
