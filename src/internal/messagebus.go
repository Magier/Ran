package bus

import (
	"context"
	"fmt"

	domain "github.com/Magier/Ran/domain"
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
	ctx := context.Background()
	// func (h *MessageBusProvider) Publish(ctx context.Context, events ...domain.Event) error {
	for _, event := range events {
		if len(h.subscribers[event.EventName()]) == 0 {
			fmt.Printf("📢 %s without subs\n", event.EventName())
		}
		for _, handler := range h.subscribers[event.EventName()] {
			err := handler(ctx, event)
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
}

func CreateMessageBus() *MessageBusProvider {
	return &MessageBusProvider{
		channel:     make(chan string),
		subscribers: make(map[string][]domain.EventHandler),
	}
}
