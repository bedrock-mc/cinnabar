package hostbridge

import (
	"encoding/json"
	"net"
	"strconv"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

// metadata consumes only Rust's authenticated presentation and the original signed raw client object.
func metadata(open hostcontract.Open) (login.IdentityData, login.ClientData, error) {
	identity := login.IdentityData{
		XUID: open.Identity.Xuid, Identity: open.Identity.Uuid, DisplayName: open.Identity.DisplayName,
		PlayFabID: open.Identity.PlayfabId, PlayFabTitleID: open.Identity.PlayfabTitleId,
	}
	id, err := uuid.Parse(identity.Identity)
	if err != nil || id == uuid.Nil || identity.XUID == "" || identity.DisplayName == "" || identity.Validate() != nil {
		return login.IdentityData{}, login.ClientData{}, errProtocol
	}
	var data login.ClientData
	if len(open.ClientData) == 0 || json.Unmarshal(open.ClientData, &data) != nil || !skinBounds(data) {
		return login.IdentityData{}, login.ClientData{}, errProtocol
	}
	// Dragonfly never routes using this field. Keep the actual metadata, but never resolve it to validate skin/locale.
	validation := data
	validation.ServerAddress = ""
	if validation.Validate() != nil {
		return login.IdentityData{}, login.ClientData{}, errProtocol
	}
	return identity, data, nil
}

// skinBounds checks the consumed image arithmetic before the Go validator or Dragonfly allocates pixels.
func skinBounds(data login.ClientData) bool {
	valid := func(width, height int) bool {
		limit := uint64(hostcontract.MAX_PACKET_BYTES) / 4
		return width >= 0 && height >= 0 && uint64(width) <= limit && uint64(height) <= limit && uint64(width)*uint64(height) <= limit
	}
	if !valid(data.SkinImageWidth, data.SkinImageHeight) || !valid(data.CapeImageWidth, data.CapeImageHeight) {
		return false
	}
	for _, animation := range data.AnimatedImageData {
		if !valid(animation.ImageWidth, animation.ImageHeight) {
			return false
		}
	}
	return true
}

// peerAddr is an actual connection marker, not a string parser or a network destination.
type peerAddr uint64

func (p peerAddr) Network() string { return "cinnabar-native-world" }
func (p peerAddr) String() string  { return "native-world:" + strconv.FormatUint(uint64(p), 10) }

// IsRemote identifies only addresses minted by this concrete authorized connection owner.
func IsRemote(addr net.Addr) bool {
	_, ok := addr.(peerAddr)
	return ok
}
