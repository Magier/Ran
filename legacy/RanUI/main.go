package main

import (
	"embed"
	"log/slog"
	"runtime"

	"github.com/wailsapp/wails/v2"
	"github.com/wailsapp/wails/v2/pkg/logger"
	"github.com/wailsapp/wails/v2/pkg/menu"
	"github.com/wailsapp/wails/v2/pkg/menu/keys"
	"github.com/wailsapp/wails/v2/pkg/options"
	"github.com/wailsapp/wails/v2/pkg/options/assetserver"
)

//go:embed all:frontend/build
var assets embed.FS

func main() {
	// Create an instance of the app structure
	app := NewApp()

	// AppMenu := menu.AppMenu()
	AppMenu := menu.NewMenu()
	// Wails' Linux menu backend crashes on role-only top-level menus like EditMenu().
	// Keep the explicit File menu below and only attach the built-in role menus where supported.
	if runtime.GOOS != "linux" {
		AppMenu.Append(menu.EditMenu())
	}

	slog.Info("Starting RanUI application", runtime.GOOS, runtime.GOARCH)
	if runtime.GOOS == "darwin" {
		AppMenu.Append(menu.AppMenu()) // On macOS platform, this must be done right after `NewMenu()`
	}

	FileMenu := AppMenu.AddSubmenu("File")
	FileMenu.AddText("Open Flow", keys.CmdOrCtrl("o"), func(_ *menu.CallbackData) {
		// TODO: load the Attack Flow
	})
	FileMenu.AddText("Save Flow", keys.CmdOrCtrl("s"), func(_ *menu.CallbackData) {
		app.SaveFlow()
	})

	// Create application with options
	err := wails.Run(&options.App{
		Title:  "RanUI",
		Width:  1600,
		Height: 1024,
		AssetServer: &assetserver.Options{
			Assets: assets,
		},
		LogLevel: logger.INFO,
		// BackgroundColour: &options.RGBA{R: 0, G: 0, B: 0, A: 1},
		OnStartup:  app.startup,
		OnDomReady: app.ClientReady,
		ErrorFormatter: func(err error) any {
			// Return whatever JSON you want the frontend to see
			return map[string]any{
				"message": err.Error(),
				"code":    "GO_BOUND_METHOD_ERROR",
			}
		},
		Menu: AppMenu,
		Bind: []interface{}{
			app,
		},
	})

	if err != nil {
		println("Error:", err.Error())
	}
}
