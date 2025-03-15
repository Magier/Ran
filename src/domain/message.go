package domain

type Message interface {
	GetID() string
	String() string
}
