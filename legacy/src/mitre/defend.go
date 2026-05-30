package mitre

type DefendTactic string

// Source: https://d3fend.mitre.org/
const (
	Model   DefendTactic = "Model"   // d3f:model
	Harden  DefendTactic = "Harden"  // d3f:harden
	Detect  DefendTactic = "Detect"  // d3f:detect
	Isolate DefendTactic = "Isolate" // d3f:isolate
	Deceive DefendTactic = "Deceive" // d3f:deceive
	Evict   DefendTactic = "Evict"   // d3f:evict
	Restore DefendTactic = "Restore" // d3f:restore
)
