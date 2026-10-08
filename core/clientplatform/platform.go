// Package clientplatform identifies the desktop build to Minecraft services.
package clientplatform

import "github.com/sandertv/gophertunnel/minecraft/service"

const (
	Platform    = service.PlatformWindows10
	SubPlatform = "Win32"
)

// TokenConfig gives every authenticated service the same desktop identity.
func TokenConfig() service.TokenConfig {
	return service.TokenConfig{Device: service.DeviceConfig{Platform: Platform}}
}
