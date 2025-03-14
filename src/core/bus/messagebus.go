package bus

import (
	"context"
	"fmt"
	"log/slog"
	"reflect"

	domain "github.com/Magier/Ran/domain"
)

type MessageBus interface {
	// Execute(cmd domain.Message) error
	HandleEvents(ctx context.Context)
	Publish(events ...domain.Message) error
	// Publish(ctx context.Context, events ...domain.Event) error
	Subscribe(event domain.Message, handler domain.MessageHandler) func()
	SubscribeToName(name string, handler domain.MessageHandler) func()
}

type MessageBusProvider struct {
	channel     chan domain.Message
	subscribers map[string][]domain.MessageHandler
}

func (b *MessageBusProvider) HandleEvents(ctx context.Context) {
	for msg := range b.channel {
		if msg == nil {
			slog.Error("Received empty message!")
			continue
		}

		// "*" subscribers listen to all events
		subscribers := append(b.subscribers[msgName(msg)], b.subscribers[domain.ALL_EVENTS]...)
		if len(subscribers) == 0 {
			slog.Debug("No subscribers for event " + msgName(msg))
		} else {
			icon := "🔊"
			if _, isCmd := msg.(domain.Command); isCmd {
				icon = "🎮"
			}
			slog.Debug(icon + " " + msg.String())
		}
		for _, handler := range subscribers {
			msg, err := handler(ctx, msg)
			if err != nil {
				slog.Error(err.Error())
			}
			if msg != nil {
				err = b.Publish(msg)
				if err != nil {
					slog.Error("Bus", "error publishing message after handler: ", err.Error())
				}
			}
		}
	}
}

// func (b *MessageBusProvider) Execute(cmd domain.Message) (chan struct{}, error) {
// func (b *MessageBusProvider) Execute(cmd domain.Message) error {
// ch := make(chan struct{}, 1)
// // lookup command provider
// go func() {
// 	defer close(ch)
// 	// TODO: do the command
// 	ch <- struct{}{}
// }()
// return ch, nil

func (b *MessageBusProvider) Publish(messages ...domain.Message) error {
	// func (h *MessageBusProvider) Publish(ctx context.Context, events ...domain.Event) error {
	for _, msg := range messages {
		b.channel <- msg
	}
	return nil
}

func (b *MessageBusProvider) SubscribeToName(name string, handler domain.MessageHandler) func() {
	// h.mu.Lock()
	// defer h.mu.Unlock()
	// name := msgName(event)
	b.subscribers[name] = append(
		b.subscribers[name],
		handler,
	)

	return func() {
		handlers := b.subscribers[name]
		currentHandlerPtr := reflect.ValueOf(handler).Pointer()

		newHandlers := make([]domain.MessageHandler, 0, len(handlers))
		for _, h := range handlers {
			if reflect.ValueOf(h).Pointer() != currentHandlerPtr {
				newHandlers = append(newHandlers, h)
			}
		}
		b.subscribers[name] = newHandlers
	}
}
func (b *MessageBusProvider) Subscribe(event domain.Message, handler domain.MessageHandler) func() {
	name := msgName(event)
	return b.SubscribeToName(name, handler)
}

func CreateMessageBus() *MessageBusProvider {
	return &MessageBusProvider{
		channel: make(chan domain.Message, 100),
		subscribers: map[string][]domain.MessageHandler{
			domain.ALL_EVENTS: {}, // a wildcard event, where subscribers will receive all events
		},
	}
}

func msgName(msg domain.Message) string {
	return fmt.Sprintf("%T", msg)
}
