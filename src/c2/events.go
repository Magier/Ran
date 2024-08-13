package c2

type ListenerReady struct {
	Name string
	Port int
	// protocol string
}

func (c ListenerReady) MessageName() string {
	return "ListenerReady"
}
