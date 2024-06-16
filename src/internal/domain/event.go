package domain

// type EventHandler func(ctx context.Context, event Event) error
type EventHandler func(event Event) error

type Event interface {
	EventName() string
}
