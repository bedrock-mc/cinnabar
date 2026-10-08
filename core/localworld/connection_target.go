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
	Address    string
	Transport  Transport
	LANAddress string
	LevelName  string // selected BDS level ID, used to match its LAN advertisement
}
