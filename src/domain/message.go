package domain

type Message interface {
	String() string
	MessageName() string
}
