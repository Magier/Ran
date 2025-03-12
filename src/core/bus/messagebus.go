package bus

import (
	"context"
	"fmt"
	"log/slog"
	"reflect"

	domain "github.com/Magier/Ran/domain"
)

type MessageBus interface {
	Execute(cmd domain.Message) error
	Publish(events ...domain.Message) error
	// Publish(ctx context.Context, events ...domain.Event) error
	Subscribe(event domain.Message, handler domain.MessageHandler) func()
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
		if len(b.subscribers[msgName(msg)]) == 0 {
			slog.Debug("No subscribers for event " + msgName(msg))
		} else {
			icon := "🔊"
			if _, isCmd := msg.(domain.Command); isCmd {
				icon = "🎮"
			}
			slog.Debug(icon + " " + msg.String())
		}
		for _, handler := range b.subscribers[msgName(msg)] {
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
func (b *MessageBusProvider) Execute(cmd domain.Message) error {
	// ch := make(chan struct{}, 1)
	// // lookup command provider
	// go func() {
	// 	defer close(ch)
	// 	// TODO: do the command
	// 	ch <- struct{}{}
	// }()
	// return ch, nil
	return nil
}

func (b *MessageBusProvider) Publish(messages ...domain.Message) error {
	// func (h *MessageBusProvider) Publish(ctx context.Context, events ...domain.Event) error {
	for _, msg := range messages {
		b.channel <- msg
	}
	return nil
}

func (b *MessageBusProvider) Subscribe(event domain.Message, handler domain.MessageHandler) func() {
	// h.mu.Lock()
	// defer h.mu.Unlock()
	key := msgName(event)
	b.subscribers[msgName(event)] = append(
		b.subscribers[msgName(event)],
		handler,
	)

	return func() {
		handlers := b.subscribers[key]
		currentHandlerPtr := reflect.ValueOf(handler).Pointer()

		newHandlers := make([]domain.MessageHandler, 0, len(handlers))
		for _, h := range handlers {
			if reflect.ValueOf(h).Pointer() != currentHandlerPtr {
				newHandlers = append(newHandlers, h)
			}
		}
		b.subscribers[key] = newHandlers
	}
}

func CreateMessageBus() *MessageBusProvider {
	return &MessageBusProvider{
		channel:     make(chan domain.Message, 100),
		subscribers: make(map[string][]domain.MessageHandler),
	}
}

func msgName(msg domain.Message) string {
	return fmt.Sprintf("%T", msg)
}
