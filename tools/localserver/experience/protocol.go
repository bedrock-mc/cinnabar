package experience

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"slices"
)

// The Go mirror of the adapter protocol in crates/experience-runtime/src/protocol.rs, which
// defines every message. A frame is a 4-byte little-endian length followed by that many bytes
// of JSON; bytes inside messages are lowercase hex. The golden fixtures in testdata/protocol,
// written by Rust, keep this mirror honest.

// protocolVersion is the adapter protocol this package speaks. It must equal the Rust runtime's
// PROTOCOL_VERSION, which TestFrameLimitMatchesRust checks against the limits fixture.
const protocolVersion = 4

// BlockPos is a block position.
type BlockPos struct {
	X int32 `json:"x"`
	Y int32 `json:"y"`
	Z int32 `json:"z"`
}

// Face is a block face.
type Face string

const (
	FaceDown  Face = "down"
	FaceUp    Face = "up"
	FaceNorth Face = "north"
	FaceSouth Face = "south"
	FaceWest  Face = "west"
	FaceEast  Face = "east"
)

// faces lists every Face in protocol.rs order.
var faces = []Face{FaceDown, FaceUp, FaceNorth, FaceSouth, FaceWest, FaceEast}

func (f Face) MarshalJSON() ([]byte, error)     { return marshalEnum(f, faces) }
func (f *Face) UnmarshalJSON(data []byte) error { return unmarshalEnum(data, f, faces) }

// Info describes the world and the event that a callback runs for.
type Info struct {
	WorldID       string `json:"world_id"`
	DimensionID   string `json:"dimension_id"`
	Tick          uint64 `json:"tick"`
	EventSequence uint64 `json:"event_sequence"`
}

// Cell is one snapshot cell. ID is empty when the cell is not loaded; Data is lowercase hex.
type Cell struct {
	Pos    BlockPos `json:"pos"`
	Loaded bool     `json:"loaded"`
	ID     string   `json:"id"`
	Owned  bool     `json:"owned"`
	Data   *string  `json:"data"`
}

// Cause is what made a block change.
type Cause string

const (
	CausePlayer      Cause = "player"
	CauseGuest       Cause = "guest"
	CauseEnvironment Cause = "environment"
)

// causes lists every Cause in protocol.rs order.
var causes = []Cause{CausePlayer, CauseGuest, CauseEnvironment}

func (c Cause) MarshalJSON() ([]byte, error)     { return marshalEnum(c, causes) }
func (c *Cause) UnmarshalJSON(data []byte) error { return unmarshalEnum(data, c, causes) }

// Change is a committed block change; PreviousData is lowercase hex.
type Change struct {
	Pos          BlockPos `json:"pos"`
	Actor        *string  `json:"actor"`
	Cause        Cause    `json:"cause"`
	BeforeID     string   `json:"before_id"`
	AfterID      string   `json:"after_id"`
	PreviousData *string  `json:"previous_data"`
}

// Scalar is one value of a client-channel record, in the form the client part's wire protocol
// gives it: a leaf, or on wire v2 a list or record of values. Exactly one field is set.
type Scalar struct {
	Bool    *bool
	Integer *int64
	Text    *string
	Choice  *uint16
	// List holds the items of a list field, all of its one item type.
	List *[]Scalar
	// Record holds one value per field of a record field, in order.
	Record *[]Scalar
}

// Call is the guest callback that a CallbackRequest runs. Exactly one field is set.
type Call struct {
	Place         *PlaceCall
	Break         *BreakCall
	Interact      *InteractCall
	Neighbor      *NeighborCall
	ClientMessage *ClientMessageCall
	Epoch         *EpochCall
}

// PlaceCall follows a successful player placement.
type PlaceCall struct {
	Change Change `json:"change"`
}

// BreakCall follows a player break; the change carries the old id and data.
type BreakCall struct {
	Change Change `json:"change"`
}

// InteractCall is a player's interaction with a block.
type InteractCall struct {
	Player string   `json:"player"`
	Pos    BlockPos `json:"pos"`
	Face   Face     `json:"face"`
}

// NeighborCall reports that the block at Neighbor changed next to the block at Pos.
type NeighborCall struct {
	Pos      BlockPos `json:"pos"`
	Neighbor BlockPos `json:"neighbor"`
}

// ClientMessageCall is a typed message that Player's client part sent on Channel, revision
// Schema. Its callback's actor is Player. With Focus, the block of Player's focus, its snapshot
// is the one an interaction with that block would have; without, it is empty.
type ClientMessageCall struct {
	Player  string    `json:"player"`
	Channel string    `json:"channel"`
	Schema  uint16    `json:"schema"`
	Payload []Scalar  `json:"payload"`
	Focus   *BlockPos `json:"focus"`
}

// EpochCall tells the guest that Player's client part moved to a new world epoch and kept
// running. Its callback's actor is Player, and its snapshot that of Focus like a client
// message's.
type EpochCall struct {
	Player string    `json:"player"`
	Focus  *BlockPos `json:"focus"`
}

// Request is a message from the adapter to the runtime. Exactly one field is set.
type Request struct {
	Load     *LoadRequest
	Callback *CallbackRequest
	Shutdown *ShutdownRequest
}

// LoadRequest loads the artifact in Dir. It is the first request of a session.
type LoadRequest struct {
	Dir string `json:"dir"`
}

// CallbackRequest runs Call on a snapshot. Its result echoes Seq.
type CallbackRequest struct {
	Seq        uint64  `json:"seq"`
	Info       Info    `json:"info"`
	Actor      *string `json:"actor"`
	WorldMinY  int32   `json:"world_min_y"`
	WorldMaxY  int32   `json:"world_max_y"`
	DataBudget uint64  `json:"data_budget"`
	Snapshot   []Cell  `json:"snapshot"`
	Call       Call    `json:"call"`
}

// ShutdownRequest ends the session.
type ShutdownRequest struct{}

// Texture binds a material slot to a texture file; Path is absolute.
type Texture struct {
	Slot string `json:"slot"`
	Path string `json:"path"`
}

// Mining is how a block is mined. Exactly one field is set.
type Mining struct {
	Unbreakable *Unbreakable
	Breakable   *Breakable
}

// Unbreakable is a block that cannot be mined.
type Unbreakable struct{}

// Breakable is a block mined with Hardness.
type Breakable struct {
	Hardness float32 `json:"hardness"`
}

// BlockDef is a validated block of the Experience.
type BlockDef struct {
	ID          string    `json:"id"`
	DisplayName string    `json:"display_name"`
	Textures    []Texture `json:"textures"`
	Mining      Mining    `json:"mining"`
}

// FailKind is why a callback failed.
type FailKind string

const (
	FailTrap     FailKind = "trap"
	FailFuel     FailKind = "fuel"
	FailDeadline FailKind = "deadline"
	FailLimit    FailKind = "limit"
)

// failKinds lists every FailKind in protocol.rs order.
var failKinds = []FailKind{FailTrap, FailFuel, FailDeadline, FailLimit}

func (k FailKind) MarshalJSON() ([]byte, error)     { return marshalEnum(k, failKinds) }
func (k *FailKind) UnmarshalJSON(data []byte) error { return unmarshalEnum(data, k, failKinds) }

// Op is a staged operation. Exactly one field is set.
type Op struct {
	SetBlock     *SetBlockOp
	SetBlockData *SetBlockDataOp
	Tell         *TellOp
	SendClient   *SendClientOp
}

// SetBlockOp sets the block at Pos to ID.
type SetBlockOp struct {
	Pos BlockPos `json:"pos"`
	ID  string   `json:"id"`
}

// SetBlockDataOp sets the data of the block at Pos; Data is lowercase hex, and nil clears it.
type SetBlockDataOp struct {
	Pos  BlockPos `json:"pos"`
	Data *string  `json:"data"`
}

// TellOp sends Text to Player.
type TellOp struct {
	Player string `json:"player"`
	Text   string `json:"text"`
}

// SendClientOp sends Payload on Channel, revision Schema, to Player's client part once the rest
// of its result has committed.
type SendClientOp struct {
	Player  string   `json:"player"`
	Channel string   `json:"channel"`
	Schema  uint16   `json:"schema"`
	Payload []Scalar `json:"payload"`
}

// Outcome is the outcome of a callback. Exactly one field is set.
type Outcome struct {
	Committed *Committed
	Rejected  *Rejected
	Failed    *Failed
}

// Committed holds the ops that the adapter applies.
type Committed struct {
	Ops []Op `json:"ops"`
}

// Rejected is a guest error. It is not a strike.
type Rejected struct {
	Reason string `json:"reason"`
}

// Failed is a callback that the runtime stopped. It is a strike against the Experience.
type Failed struct {
	Kind   FailKind `json:"kind"`
	Reason string   `json:"reason"`
}

// Response is a message from the runtime to the adapter. Exactly one field is set.
type Response struct {
	Loaded     *Loaded
	LoadFailed *LoadFailed
	Result     *Result
}

// Loaded answers load with the artifact's identity and its validated blocks. Focus is set when
// the Experience's world takes its player's focus in client messages and epochs; one that does
// not is never given one.
type Loaded struct {
	Protocol uint32     `json:"protocol"`
	ID       string     `json:"id"`
	Version  string     `json:"version"`
	Blocks   []BlockDef `json:"blocks"`
	Focus    bool       `json:"focus"`
}

// LoadFailed answers load when the artifact does not load; the runtime then exits.
type LoadFailed struct {
	Reason string `json:"reason"`
}

// Result answers the callback with the same Seq.
type Result struct {
	Seq     uint64  `json:"seq"`
	Outcome Outcome `json:"outcome"`
}

// encodeFrame encodes req as a frame body, refusing one larger than maxFrameBytes.
func encodeFrame(req Request) ([]byte, error) {
	body, err := json.Marshal(req)
	if err != nil {
		return nil, err
	}
	if len(body) > maxFrameBytes {
		return nil, fmt.Errorf("frame of %d bytes exceeds %d", len(body), maxFrameBytes)
	}
	return body, nil
}

// writeFrame writes body, from encodeFrame, behind its length.
func writeFrame(w io.Writer, body []byte) error {
	if _, err := w.Write(binary.LittleEndian.AppendUint32(nil, uint32(len(body)))); err != nil {
		return err
	}
	_, err := w.Write(body)
	return err
}

// readFrame reads one response frame. A length over maxFrameBytes is refused before the body is
// read.
func readFrame(r io.Reader) (Response, error) {
	var prefix [4]byte
	if _, err := io.ReadFull(r, prefix[:]); err != nil {
		return Response{}, fmt.Errorf("reading a frame length: %w", err)
	}
	length := binary.LittleEndian.Uint32(prefix[:])
	if length > maxFrameBytes {
		return Response{}, fmt.Errorf("frame of %d bytes exceeds %d", length, maxFrameBytes)
	}
	body := make([]byte, length)
	if _, err := io.ReadFull(r, body); err != nil {
		return Response{}, fmt.Errorf("reading a frame of %d bytes: %w", length, err)
	}
	var resp Response
	if err := decodeStrict(body, &resp); err != nil {
		return Response{}, fmt.Errorf("decoding a frame: %w", err)
	}
	return resp, nil
}

// decodeStrict decodes data, which holds exactly one JSON value, into v and refuses object
// members that v has no field for. encoding/json does not pass that setting into UnmarshalJSON
// methods, so every union decodes its variant with decodeStrict again.
func decodeStrict(data []byte, v any) error {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(v); err != nil {
		return err
	}
	if _, err := decoder.Token(); err != io.EOF {
		return errors.New("data after the JSON value")
	}
	return nil
}

// marshalEnum encodes v, which must be one of values, as a JSON string.
func marshalEnum[T ~string](v T, values []T) ([]byte, error) {
	if !slices.Contains(values, v) {
		return nil, fmt.Errorf("unknown %T %q", v, string(v))
	}
	return json.Marshal(string(v))
}

// unmarshalEnum decodes data, a JSON string that must be one of values, into v.
func unmarshalEnum[T ~string](data []byte, v *T, values []T) error {
	var s string
	if err := json.Unmarshal(data, &s); err != nil {
		return err
	}
	if !slices.Contains(values, T(s)) {
		return fmt.Errorf("unknown %T %q", *v, s)
	}
	*v = T(s)
	return nil
}

// The "type" that names each union variant on the wire, as protocol.rs names it.
const (
	typeLoad          = "load"
	typeCallback      = "callback"
	typeShutdown      = "shutdown"
	typePlace         = "place"
	typeBreak         = "break"
	typeInteract      = "interact"
	typeNeighbor      = "neighbor"
	typeClientMessage = "client_message"
	typeEpoch         = "epoch"
	typeUnbreakable   = "unbreakable"
	typeBreakable     = "breakable"
	typeSetBlock      = "set_block"
	typeSetBlockData  = "set_block_data"
	typeTell          = "tell"
	typeSendClient    = "send_client"
	typeCommitted     = "committed"
	typeRejected      = "rejected"
	typeFailed        = "failed"
	typeLoaded        = "loaded"
	typeLoadFailed    = "load_failed"
	typeResult        = "result"
	typeBool          = "bool"
	typeInteger       = "integer"
	typeText          = "text"
	typeChoice        = "choice"
	typeList          = "list"
	typeRecord        = "record"
)

// variantTag is the "type" member of a union variant on the wire.
type variantTag struct {
	Type string `json:"type"`
}

// Each variant's wire form embeds variantTag and the variant, so encoding/json writes and reads
// one object holding "type" and the variant's members. This works because neither has JSON
// methods and no variant has a "type" field.
type (
	loadWire struct {
		variantTag
		*LoadRequest
	}
	callbackWire struct {
		variantTag
		*CallbackRequest
	}
	shutdownWire struct {
		variantTag
		*ShutdownRequest
	}
	placeWire struct {
		variantTag
		*PlaceCall
	}
	breakWire struct {
		variantTag
		*BreakCall
	}
	interactWire struct {
		variantTag
		*InteractCall
	}
	neighborWire struct {
		variantTag
		*NeighborCall
	}
	clientMessageWire struct {
		variantTag
		*ClientMessageCall
	}
	epochWire struct {
		variantTag
		*EpochCall
	}
	unbreakableWire struct {
		variantTag
		*Unbreakable
	}
	breakableWire struct {
		variantTag
		*Breakable
	}
	setBlockWire struct {
		variantTag
		*SetBlockOp
	}
	setBlockDataWire struct {
		variantTag
		*SetBlockDataOp
	}
	tellWire struct {
		variantTag
		*TellOp
	}
	sendClientWire struct {
		variantTag
		*SendClientOp
	}
	committedWire struct {
		variantTag
		*Committed
	}
	rejectedWire struct {
		variantTag
		*Rejected
	}
	failedWire struct {
		variantTag
		*Failed
	}
	loadedWire struct {
		variantTag
		*Loaded
	}
	loadFailedWire struct {
		variantTag
		*LoadFailed
	}
	resultWire struct {
		variantTag
		*Result
	}
)

// scalarWire is a Scalar on the wire: "type" names its field, and "value" holds it.
type scalarWire[T any] struct {
	Type  string `json:"type"`
	Value *T     `json:"value"`
}

// unionTag returns the "type" of data, a JSON object.
func unionTag(data []byte) (string, error) {
	var head struct {
		Type *string `json:"type"`
	}
	if err := json.Unmarshal(data, &head); err != nil {
		return "", err
	}
	if head.Type == nil {
		return "", errors.New(`no "type"`)
	}
	return *head.Type, nil
}

// fresh points *p at a new T and returns it.
func fresh[T any](p **T) *T {
	*p = new(T)
	return *p
}

func (c Call) MarshalJSON() ([]byte, error) {
	switch {
	case c.Place != nil:
		return json.Marshal(placeWire{variantTag{typePlace}, c.Place})
	case c.Break != nil:
		return json.Marshal(breakWire{variantTag{typeBreak}, c.Break})
	case c.Interact != nil:
		return json.Marshal(interactWire{variantTag{typeInteract}, c.Interact})
	case c.Neighbor != nil:
		return json.Marshal(neighborWire{variantTag{typeNeighbor}, c.Neighbor})
	case c.ClientMessage != nil:
		return json.Marshal(clientMessageWire{variantTag{typeClientMessage}, c.ClientMessage})
	case c.Epoch != nil:
		return json.Marshal(epochWire{variantTag{typeEpoch}, c.Epoch})
	}
	return nil, errors.New("empty call")
}

func (c *Call) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*c = Call{}
	switch tag {
	case typePlace:
		return decodeStrict(data, &placeWire{PlaceCall: fresh(&c.Place)})
	case typeBreak:
		return decodeStrict(data, &breakWire{BreakCall: fresh(&c.Break)})
	case typeInteract:
		return decodeStrict(data, &interactWire{InteractCall: fresh(&c.Interact)})
	case typeNeighbor:
		return decodeStrict(data, &neighborWire{NeighborCall: fresh(&c.Neighbor)})
	case typeClientMessage:
		return decodeStrict(data, &clientMessageWire{ClientMessageCall: fresh(&c.ClientMessage)})
	case typeEpoch:
		return decodeStrict(data, &epochWire{EpochCall: fresh(&c.Epoch)})
	}
	return fmt.Errorf("unknown call type %q", tag)
}

// MarshalJSON writes the client wire protocol's form, escaping no HTML: the encoder that embeds
// it escapes HTML only when it is set to.
func (s Scalar) MarshalJSON() ([]byte, error) {
	switch {
	case s.Bool != nil:
		return marshalUnescaped(scalarWire[bool]{typeBool, s.Bool})
	case s.Integer != nil:
		return marshalUnescaped(scalarWire[int64]{typeInteger, s.Integer})
	case s.Text != nil:
		return marshalUnescaped(scalarWire[string]{typeText, s.Text})
	case s.Choice != nil:
		return marshalUnescaped(scalarWire[uint16]{typeChoice, s.Choice})
	case s.List != nil:
		return marshalUnescaped(scalarWire[[]Scalar]{typeList, s.List})
	case s.Record != nil:
		return marshalUnescaped(scalarWire[[]Scalar]{typeRecord, s.Record})
	}
	return nil, errors.New("empty scalar")
}

// marshalUnescaped encodes v like json.Marshal, but leaves <, > and & as they are.
func marshalUnescaped(v any) ([]byte, error) {
	var out bytes.Buffer
	encoder := json.NewEncoder(&out)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimSuffix(out.Bytes(), []byte("\n")), nil
}

func (s *Scalar) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*s = Scalar{}
	switch tag {
	case typeBool:
		return scalarValue(data, &s.Bool)
	case typeInteger:
		return scalarValue(data, &s.Integer)
	case typeText:
		return scalarValue(data, &s.Text)
	case typeChoice:
		return scalarValue(data, &s.Choice)
	case typeList:
		return scalarValue(data, &s.List)
	case typeRecord:
		return scalarValue(data, &s.Record)
	}
	return fmt.Errorf("unknown scalar type %q", tag)
}

// scalarValue decodes data, a Scalar on the wire whose value must be present and a T, into *p.
func scalarValue[T any](data []byte, p **T) error {
	var wire scalarWire[T]
	if err := decodeStrict(data, &wire); err != nil {
		return err
	}
	if wire.Value == nil {
		return errors.New(`a scalar without a "value"`)
	}
	*p = wire.Value
	return nil
}

func (r Request) MarshalJSON() ([]byte, error) {
	switch {
	case r.Load != nil:
		return json.Marshal(loadWire{variantTag{typeLoad}, r.Load})
	case r.Callback != nil:
		return json.Marshal(callbackWire{variantTag{typeCallback}, r.Callback})
	case r.Shutdown != nil:
		return json.Marshal(shutdownWire{variantTag{typeShutdown}, r.Shutdown})
	}
	return nil, errors.New("empty request")
}

func (r *Request) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*r = Request{}
	switch tag {
	case typeLoad:
		return decodeStrict(data, &loadWire{LoadRequest: fresh(&r.Load)})
	case typeCallback:
		return decodeStrict(data, &callbackWire{CallbackRequest: fresh(&r.Callback)})
	case typeShutdown:
		return decodeStrict(data, &shutdownWire{ShutdownRequest: fresh(&r.Shutdown)})
	}
	return fmt.Errorf("unknown request type %q", tag)
}

func (m Mining) MarshalJSON() ([]byte, error) {
	switch {
	case m.Unbreakable != nil:
		return json.Marshal(unbreakableWire{variantTag{typeUnbreakable}, m.Unbreakable})
	case m.Breakable != nil:
		return json.Marshal(breakableWire{variantTag{typeBreakable}, m.Breakable})
	}
	return nil, errors.New("empty mining")
}

func (m *Mining) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*m = Mining{}
	switch tag {
	case typeUnbreakable:
		return decodeStrict(data, &unbreakableWire{Unbreakable: fresh(&m.Unbreakable)})
	case typeBreakable:
		return decodeStrict(data, &breakableWire{Breakable: fresh(&m.Breakable)})
	}
	return fmt.Errorf("unknown mining type %q", tag)
}

func (o Op) MarshalJSON() ([]byte, error) {
	switch {
	case o.SetBlock != nil:
		return json.Marshal(setBlockWire{variantTag{typeSetBlock}, o.SetBlock})
	case o.SetBlockData != nil:
		return json.Marshal(setBlockDataWire{variantTag{typeSetBlockData}, o.SetBlockData})
	case o.Tell != nil:
		return json.Marshal(tellWire{variantTag{typeTell}, o.Tell})
	case o.SendClient != nil:
		return json.Marshal(sendClientWire{variantTag{typeSendClient}, o.SendClient})
	}
	return nil, errors.New("empty op")
}

func (o *Op) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*o = Op{}
	switch tag {
	case typeSetBlock:
		return decodeStrict(data, &setBlockWire{SetBlockOp: fresh(&o.SetBlock)})
	case typeSetBlockData:
		return decodeStrict(data, &setBlockDataWire{SetBlockDataOp: fresh(&o.SetBlockData)})
	case typeTell:
		return decodeStrict(data, &tellWire{TellOp: fresh(&o.Tell)})
	case typeSendClient:
		return decodeStrict(data, &sendClientWire{SendClientOp: fresh(&o.SendClient)})
	}
	return fmt.Errorf("unknown op type %q", tag)
}

func (o Outcome) MarshalJSON() ([]byte, error) {
	switch {
	case o.Committed != nil:
		return json.Marshal(committedWire{variantTag{typeCommitted}, o.Committed})
	case o.Rejected != nil:
		return json.Marshal(rejectedWire{variantTag{typeRejected}, o.Rejected})
	case o.Failed != nil:
		return json.Marshal(failedWire{variantTag{typeFailed}, o.Failed})
	}
	return nil, errors.New("empty outcome")
}

func (o *Outcome) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*o = Outcome{}
	switch tag {
	case typeCommitted:
		return decodeStrict(data, &committedWire{Committed: fresh(&o.Committed)})
	case typeRejected:
		return decodeStrict(data, &rejectedWire{Rejected: fresh(&o.Rejected)})
	case typeFailed:
		return decodeStrict(data, &failedWire{Failed: fresh(&o.Failed)})
	}
	return fmt.Errorf("unknown outcome type %q", tag)
}

func (r Response) MarshalJSON() ([]byte, error) {
	switch {
	case r.Loaded != nil:
		return json.Marshal(loadedWire{variantTag{typeLoaded}, r.Loaded})
	case r.LoadFailed != nil:
		return json.Marshal(loadFailedWire{variantTag{typeLoadFailed}, r.LoadFailed})
	case r.Result != nil:
		return json.Marshal(resultWire{variantTag{typeResult}, r.Result})
	}
	return nil, errors.New("empty response")
}

func (r *Response) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*r = Response{}
	switch tag {
	case typeLoaded:
		return decodeStrict(data, &loadedWire{Loaded: fresh(&r.Loaded)})
	case typeLoadFailed:
		return decodeStrict(data, &loadFailedWire{LoadFailed: fresh(&r.LoadFailed)})
	case typeResult:
		return decodeStrict(data, &resultWire{Result: fresh(&r.Result)})
	}
	return fmt.Errorf("unknown response type %q", tag)
}
