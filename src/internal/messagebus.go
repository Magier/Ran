package bus

import (
	"fmt"

	domain "github.com/Magier/Ran/internal/domain"
)

type MessageBus interface {
	Publish(events ...domain.Event) error
	// Publish(ctx context.Context, events ...domain.Event) error
	Subscribe(event domain.Event, handler domain.EventHandler)
}

type MessageBusProvider struct {
	channel     chan string
	subscribers map[string][]domain.EventHandler
}

func (h *MessageBusProvider) Publish(events ...domain.Event) error {
	// func (h *MessageBusProvider) Publish(ctx context.Context, events ...domain.Event) error {
	for _, event := range events {
		fmt.Print("Publishing message ", event.EventName(), "\n")
		for _, handler := range h.subscribers[event.EventName()] {
			err := handler(event)
			if err != nil {
				return err
			}
		}
	}
	return nil
}

func (b *MessageBusProvider) Subscribe(event domain.Event, handler domain.EventHandler) {
	// h.mu.Lock()
	// defer h.mu.Unlock()
	b.subscribers[event.EventName()] = append(
		b.subscribers[event.EventName()],
		handler,
	)
	fmt.Print("Publishing message to topic: ", event.EventName(), "\n")
}

func CreateMessageBus() *MessageBusProvider {
	return &MessageBusProvider{
		channel:     make(chan string),
		subscribers: make(map[string][]domain.EventHandler),
	}
}
