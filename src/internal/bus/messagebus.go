package bus

import (
	"context"
	"log/slog"

	domain "github.com/Magier/Ran/domain"
)

type MessageBus interface {
	Execute(cmd domain.Message) error
	RegisterCommand(cmd domain.Message, fn interface{})
	Publish(events ...domain.Message) error
	// Publish(ctx context.Context, events ...domain.Event) error
	Subscribe(event domain.Message, handler domain.EventHandler)
}

type MessageBusProvider struct {
	channel     chan domain.Message
	subscribers map[string][]domain.EventHandler
}

func (b *MessageBusProvider) HandleEvents(ctx context.Context) error {
	for msg := range b.channel {
		// fmt.Printf("🚌 handling event %s\n", event.MessageName())
		if len(b.subscribers[msg.MessageName()]) == 0 {
			// fmt.Printf("🙉 %s: no subs\n", event.MessageName())
		} else {
			slog.Info("🔊 " + msg.MessageName())
		}
		for _, handler := range b.subscribers[msg.MessageName()] {
			event := msg.(domain.Event)
			msg, err := handler(ctx, event)
			if err != nil {
				err := b.Publish(domain.ErrorMsg{Level: domain.LevelError, Msg: err.Error()})
				if err != nil {
					slog.Error("Couldn't publish error message: ", "error", err.Error())
				}
				return err
			}
			if msg != nil {
				err = b.Publish(msg)
				if err != nil {
					slog.Error("Bus", "error publishing message after handler: ", err.Error())
				}
			}
		}
	}
	return nil
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

func (b *MessageBusProvider) RegisterCommand(cmd domain.Message, fn interface{}) {
	_ = 4
}

func (b *MessageBusProvider) Publish(events ...domain.Message) error {
	// func (h *MessageBusProvider) Publish(ctx context.Context, events ...domain.Event) error {
	for _, event := range events {
		b.channel <- event
	}
	return nil
}

func (b *MessageBusProvider) Subscribe(event domain.Message, handler domain.EventHandler) {
	// h.mu.Lock()
	// defer h.mu.Unlock()
	b.subscribers[event.MessageName()] = append(
		b.subscribers[event.MessageName()],
		handler,
	)
}

func CreateMessageBus() *MessageBusProvider {
	return &MessageBusProvider{
		channel:     make(chan domain.Message, 100),
		subscribers: make(map[string][]domain.EventHandler),
	}
}
