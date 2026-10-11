package localworld

// Transport identifies the local server's version-matched connection protocol.
type Transport string

const (
	TransportRakNet        Transport = "raknet"
	TransportNetherNetHTTP Transport = "nethernet"
	TransportNetherNetLAN  Transport = "nethernet-lan"
)

// ConnectionTarget keeps the server address and its transport together.
type ConnectionTarget struct {
	Address    string    `json:"address"`
	Transport  Transport `json:"transport"`
	LANAddress string    `json:"lan_address,omitempty"`
	LevelName  string    `json:"level_name,omitempty"` // selected BDS level ID, used to match its LAN advertisement
}
