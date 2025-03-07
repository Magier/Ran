//go:build ignore

package main

import (
	"fmt"
	"os"
)

func main() {
	// load the attack-stix.json file
	attackStixUrl := os.Args[1]
	// download the file
	fmt.Println("Downloading the attack-stix.json file from: ", attackStixUrl)

	// parse the file
	// filter the attack-patterns and tactics
	// generate the mappings

	// delete the file attackStix mapping file

}
