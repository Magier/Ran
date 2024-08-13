package tui

import (
	"bufio"
	"fmt"
	"os"
	"strings"

	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal"
)

func SimpleTUI(bus bus.MessageBus, camp *campaign.Campaign) {
	reader := bufio.NewReader(os.Stdin)
	fmt.Println("---------------------")
	fmt.Println("Ran TUI")
	fmt.Println("---------------------")

	for running := true; running; {
		fmt.Print("-> ")
		text, _ := reader.ReadString('\n')

		cmd, args := parseCommand(text)

		switch cmd {
		case "exit":
			running = false
		case "quit":
			running = false
		case "help":
			showHelp()
		case "listen":
			bus.Publish(domain.StartListener{Port: 1337})
		case "sessions":
			// TODO: listen sessions from c2
			sessions := camp.GetSessions()

			if len(sessions) == 0 {
				fmt.Println("No sessions active")
			} else {
				for _, s := range sessions {
					fmt.Printf("Session: %s\n", s.Id)
				}
			}
		case "stop":
			// TODO: stop specified sessions
			fmt.Println("stopping sessions is not yet supported :(")
		default:
			fmt.Println("Unkwnon command:", cmd)
			fmt.Println("Args:", args)
		}
	}
	fmt.Println("Bye~~")
}

func parseCommand(text string) (string, []string) {
	text = strings.Trim(text, "\n")
	parts := strings.Split(text, " ")
	cmd := parts[0]
	args := parts[1:]

	return strings.ToLower(cmd), args
}

func showHelp() {
	fmt.Println("404 help not found")
}
