package bus

import (
	"context"
	"fmt"
	"log/slog"
	"reflect"
	"sync"

	domain "github.com/Magier/Ran/domain"
)

type MessageBus interface {
	// Execute(cmd domain.Message) error
	HandleEvents(ctx context.Context)
	Publish(events ...domain.Message) error
	// Publish(ctx context.Context, events ...domain.Event) error
	Subscribe(event domain.Message, handler domain.MessageHandler) func()
	SubscribeToName(name string, handler domain.MessageHandler) func()
	SubscribeOnce(event domain.Message, handler domain.MessageHandler) func()
	SubscribeOnceToName(name string, handler domain.MessageHandler) func()
	SubscribeUntil(event domain.Message, shouldUnsubscribe func(domain.Message) bool, handler domain.MessageHandler) func()
	SubscribeUntilToName(name string, shouldUnsubscribe func(domain.Message) bool, handler domain.MessageHandler) func()
}

type MessageBusProvider struct {
	channel     chan domain.Message
	subscribers map[string][]domain.MessageHandler
	mu          sync.Mutex
}

func (b *MessageBusProvider) HandleEvents(ctx context.Context) {
	for msg := range b.channel {
		if msg == nil {
			slog.Error("Received empty message!")
			continue
		}

		// ensure errors are properly logged and not just propagated
		if ev, ok := msg.(domain.ErrorMsg); ok && ev.Msg != "" {
			switch ev.Level {
			case domain.LevelWarn:
				slog.Warn(ev.Msg)
			case domain.LevelInfo:
				slog.Info(ev.Msg)
			case domain.LevelDebug:
				slog.Debug(ev.Msg)
			default:
				slog.Error(ev.Msg)
			}
		}

		// "*" subscribers listen to all events
		b.mu.Lock()
		subscribers := append(b.subscribers[msgName(msg)], b.subscribers[domain.ALL_EVENTS]...)
		b.mu.Unlock()

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
			// execute all handlers in parallal, so they don't block each other
			go func() {
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
			}()
		}
	}
}

func (b *MessageBusProvider) Publish(messages ...domain.Message) error {
	// func (h *MessageBusProvider) Publish(ctx context.Context, events ...domain.Event) error {
	for _, msg := range messages {
		b.channel <- msg
	}
	return nil
}

func (b *MessageBusProvider) SubscribeToName(name string, handler domain.MessageHandler) func() {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.subscribers[name] = append(
		b.subscribers[name],
		handler,
	)

	slog.Debug(fmt.Sprintf("Subscribed to event %s", name))

	return func() {
		slog.Debug("Unsubscribing from event " + name)
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

// SubscribeOnceToName subscribes to an event by name and automatically unsubscribes after the first message
func (b *MessageBusProvider) SubscribeOnceToName(name string, handler domain.MessageHandler) func() {
	var unsubFn func()
	wrappedHandler := func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		defer func() {
			if unsubFn != nil {
				unsubFn()
			}
		}()
		return handler(ctx, msg)
	}
	unsubFn = b.SubscribeToName(name, wrappedHandler)
	return unsubFn
}

// SubscribeOnce subscribes to an event and automatically unsubscribes after the first message
func (b *MessageBusProvider) SubscribeOnce(event domain.Message, handler domain.MessageHandler) func() {
	name := msgName(event)
	return b.SubscribeOnceToName(name, handler)
}

// SubscribeUntilToName subscribes to an event by name and unsubscribes when the condition returns true
func (b *MessageBusProvider) SubscribeUntilToName(name string, shouldUnsubscribe func(domain.Message) bool, handler domain.MessageHandler) func() {
	var unsubFn func()
	wrappedHandler := func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		result, err := handler(ctx, msg)
		if shouldUnsubscribe(msg) && unsubFn != nil {
			unsubFn()
		}
		return result, err
	}
	unsubFn = b.SubscribeToName(name, wrappedHandler)
	return unsubFn
}

// SubscribeUntil subscribes to an event and unsubscribes when the condition returns true
func (b *MessageBusProvider) SubscribeUntil(event domain.Message, shouldUnsubscribe func(domain.Message) bool, handler domain.MessageHandler) func() {
	name := msgName(event)
	return b.SubscribeUntilToName(name, shouldUnsubscribe, handler)
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
