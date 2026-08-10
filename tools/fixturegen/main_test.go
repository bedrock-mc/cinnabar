package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

type testManifestEntry struct {
	Name          string `json:"name"`
	File          string `json:"file"`
	ID            uint32 `json:"id"`
	ByteLength    int    `json:"byte_length"`
	SHA256        string `json:"sha256"`
	WireAuthority string `json:"wire_authority,omitempty"`
	WireCommit    string `json:"wire_commit,omitempty"`
}

func TestInteractionFixturesCrossDecodeWithPinnedGophertunnel(t *testing.T) {
	out := t.TempDir()
	if err := generate(out); err != nil {
		t.Fatalf("generate corpus: %v", err)
	}

	filled := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_click_block.bin"))
	assertClickBlockTransaction(t, filled, protocol.BlockPos{13, 71, -29}, 5, 7,
		mgl32.Vec3{13.25, 72.625, -28.75}, mgl32.Vec3{0.125, 0.875, 0.625}, 123_456)
	filledData := filled.TransactionData.(*protocol.UseItemTransactionData)
	if filledData.HeldItem.Stack.NetworkID != 5 || filledData.HeldItem.Stack.Count != 2 || filledData.HeldItem.StackNetworkID != 11 {
		t.Fatalf("filled held item = %+v, want network=5 count=2 stack=11", filledData.HeldItem)
	}

	empty := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_click_block_empty_hand.bin"))
	assertClickBlockTransaction(t, empty, protocol.BlockPos{-8, 63, 21}, 0, 0,
		mgl32.Vec3{-7.75, 64.5, 21.875}, mgl32.Vec3{0.75, 0.25, 0.5}, ^uint32(0))
	emptyData := empty.TransactionData.(*protocol.UseItemTransactionData)
	if emptyData.HeldItem.Stack.NetworkID != 0 || emptyData.HeldItem.Stack.Count != 0 || emptyData.HeldItem.StackNetworkID != 0 {
		t.Fatalf("empty held item = %+v, want canonical empty item", emptyData.HeldItem)
	}

	attack := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_attack_actor.bin"))
	assertActorUseTransaction(t, attack, 0x0102_0304_0506_0708, protocol.UseItemOnEntityActionAttack,
		8, mgl32.Vec3{10.25, 65.625, -4.75}, mgl32.Vec3{0.375, 1.25, -0.125})
	assertFixtureItem(t, attack.TransactionData.(*protocol.UseItemOnEntityTransactionData).HeldItem, 7, 1, 13)

	attackEmpty := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_attack_actor_empty_hand.bin"))
	assertActorUseTransaction(t, attackEmpty, ^uint64(0), protocol.UseItemOnEntityActionAttack,
		0, mgl32.Vec3{-12.5, 70, 31.75}, mgl32.Vec3{-0.5, 0.625, 1.5})
	assertFixtureItem(t, attackEmpty.TransactionData.(*protocol.UseItemOnEntityTransactionData).HeldItem, 0, 0, 0)

	interact := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_interact_actor.bin"))
	assertActorUseTransaction(t, interact, 123_456_789, protocol.UseItemOnEntityActionInteract,
		3, mgl32.Vec3{2.5, 63.875, 9.125}, mgl32.Vec3{0.25, 0.75, 0.5})
	assertFixtureItem(t, interact.TransactionData.(*protocol.UseItemOnEntityTransactionData).HeldItem, 8, 4, 14)

	interactEmpty := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_interact_actor_empty_hand.bin"))
	assertActorUseTransaction(t, interactEmpty, 1, protocol.UseItemOnEntityActionInteract,
		6, mgl32.Vec3{-1.25, 80.5, -16.75}, mgl32.Vec3{1.125, -0.25, 0.875})
	assertFixtureItem(t, interactEmpty.TransactionData.(*protocol.UseItemOnEntityTransactionData).HeldItem, 0, 0, 0)

	closed := decodeClientPacket(t, filepath.Join(out, "container_close.bin"))
	closePacket, ok := closed.(*packet.ContainerClose)
	if !ok {
		t.Fatalf("container close decoded as %T", closed)
	}
	if closePacket.WindowID != 5 || closePacket.ContainerType != 0 || closePacket.ServerSide {
		t.Fatalf("container close = %+v, want client close for window 5/type 0", closePacket)
	}
}

func assertActorUseTransaction(t *testing.T, transaction *packet.InventoryTransaction, runtimeID uint64,
	action, slot int32, player, hit mgl32.Vec3) {
	t.Helper()
	if transaction.LegacyRequestID != 0 || len(transaction.LegacySetItemSlots) != 0 || len(transaction.Actions) != 0 {
		t.Fatalf("legacy/actions = (%d, %d, %d), want all empty", transaction.LegacyRequestID,
			len(transaction.LegacySetItemSlots), len(transaction.Actions))
	}
	data, ok := transaction.TransactionData.(*protocol.UseItemOnEntityTransactionData)
	if !ok {
		t.Fatalf("transaction data = %T, want use-item-on-entity", transaction.TransactionData)
	}
	if data.TargetEntityRuntimeID != runtimeID || data.ActionType != action || data.HotBarSlot != slot ||
		data.Position != player || data.ClickedPosition != hit {
		t.Fatalf("actor-use data = %+v", data)
	}
}

func assertFixtureItem(t *testing.T, item protocol.ItemInstance, networkID int32, count uint16, stackNetworkID int32) {
	t.Helper()
	if item.Stack.NetworkID != networkID || item.Stack.Count != count || item.StackNetworkID != stackNetworkID {
		t.Fatalf("held item = %+v, want network=%d count=%d stack=%d", item, networkID, count, stackNetworkID)
	}
}

func decodeClientFixture(t *testing.T, path string) *packet.InventoryTransaction {
	t.Helper()
	decoded := decodeClientPacket(t, path)
	transaction, ok := decoded.(*packet.InventoryTransaction)
	if !ok {
		t.Fatalf("inventory transaction decoded as %T", decoded)
	}
	return transaction
}

func decodeClientPacket(t *testing.T, path string) packet.Packet {
	t.Helper()
	raw := readFile(t, path)
	entries, err := packet.NewDecoder(bytes.NewReader(raw)).Decode()
	if err != nil {
		t.Fatalf("decode raw batch %s: %v", filepath.Base(path), err)
	}
	if len(entries) != 1 {
		t.Fatalf("%s entries = %d, want 1", filepath.Base(path), len(entries))
	}
	body := bytes.NewBuffer(entries[0])
	var header packet.Header
	if err := header.Read(body); err != nil {
		t.Fatalf("decode %s header: %v", filepath.Base(path), err)
	}
	constructor, ok := packet.NewClientPool()[header.PacketID]
	if !ok {
		t.Fatalf("packet %d is absent from client pool", header.PacketID)
	}
	decoded := constructor()
	decoded.Marshal(protocol.NewReader(body, 0, true))
	if body.Len() != 0 {
		t.Fatalf("%s leaves %d trailing bytes", filepath.Base(path), body.Len())
	}
	return decoded
}

func assertClickBlockTransaction(t *testing.T, transaction *packet.InventoryTransaction, block protocol.BlockPos,
	face, slot int32, player, click mgl32.Vec3, runtimeID uint32) {
	t.Helper()
	if transaction.LegacyRequestID != 0 || len(transaction.LegacySetItemSlots) != 0 || len(transaction.Actions) != 0 {
		t.Fatalf("legacy/actions = (%d, %d, %d), want all empty", transaction.LegacyRequestID,
			len(transaction.LegacySetItemSlots), len(transaction.Actions))
	}
	data, ok := transaction.TransactionData.(*protocol.UseItemTransactionData)
	if !ok {
		t.Fatalf("transaction data = %T, want use-item", transaction.TransactionData)
	}
	if data.ActionType != protocol.UseItemActionClickBlock || data.TriggerType != protocol.TriggerTypePlayerInput ||
		data.BlockPosition != block || data.BlockFace != face || data.HotBarSlot != slot || data.Position != player ||
		data.ClickedPosition != click || data.BlockRuntimeID != runtimeID ||
		data.ClientPrediction != protocol.ClientPredictionFailure || data.ClientCooldownState != protocol.ClientCooldownStateOff {
		t.Fatalf("click-block data = %+v", data)
	}
}

func TestGenerateIsDeterministicAndWritesPinnedRawBatches(t *testing.T) {
	firstDir := t.TempDir()
	secondDir := t.TempDir()
	if err := generate(firstDir); err != nil {
		t.Fatalf("generate first corpus: %v", err)
	}
	if err := generate(secondDir); err != nil {
		t.Fatalf("generate second corpus: %v", err)
	}

	firstManifestBytes := readFile(t, filepath.Join(firstDir, "manifest.json"))
	secondManifestBytes := readFile(t, filepath.Join(secondDir, "manifest.json"))
	if !bytes.Equal(firstManifestBytes, secondManifestBytes) {
		t.Fatal("manifest differs between identical generator runs")
	}
	if len(firstManifestBytes) == 0 || firstManifestBytes[len(firstManifestBytes)-1] != '\n' {
		t.Fatal("manifest must end in exactly one newline")
	}

	var manifest []testManifestEntry
	if err := json.Unmarshal(firstManifestBytes, &manifest); err != nil {
		t.Fatalf("decode manifest: %v", err)
	}
	wantNames := []string{
		"NetworkSettings",
		"StartGame",
		"LevelChunk",
		"MovePlayer",
		"PlayerAuthInput",
		"AddActor",
		"Text",
		"TextObjectRawText",
		"TextObjectWhisperRawText",
		"TextObjectAnnouncementRawText",
		"SetTitle",
		"BossEvent",
		"ModalFormRequest",
		"AvailableCommands",
		"AvailableCommandsLive356513",
		"BiomeDefinitionListChunkGeneration",
		"InventoryContent",
		"InventorySlot",
		"PlayerHotBar",
		"ItemStackResponse",
		"InventoryTransactionClickBlock",
		"InventoryTransactionClickBlockEmptyHand",
		"InventoryTransactionAttackActor",
		"InventoryTransactionAttackActorEmptyHand",
		"InventoryTransactionInteractActor",
		"InventoryTransactionInteractActorEmptyHand",
		"ContainerClose",
	}
	wantIDs := []uint32{143, 11, 58, 19, 144, 13, 9, 9, 9, 9, 88, 74, 100, 76, 76, 122, 49, 50, 48, 148, 30, 30, 30, 30, 30, 30, 47}
	wantHeaders := [][]byte{
		{0x8f, 0x49},
		{0x8b, 0x48},
		{0xba, 0x48},
		{0x93, 0x48},
		{0x90, 0x49},
		{0x8d, 0x48},
		{0x89, 0x48},
		{0x89, 0x48},
		{0x89, 0x48},
		{0x89, 0x48},
		{0xd8, 0x48},
		{0xca, 0x48},
		{0xe4, 0x48},
		{0xcc, 0x48},
		{0xcc, 0x48},
		{0xfa, 0x48},
		{0xb1, 0x48},
		{0xb2, 0x48},
		{0xb0, 0x48},
		{0x94, 0x49},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0xaf, 0x48},
	}
	if len(manifest) != len(wantNames) {
		t.Fatalf("manifest entries = %d, want %d", len(manifest), len(wantNames))
	}

	for i, entry := range manifest {
		if entry.Name != wantNames[i] || entry.ID != wantIDs[i] {
			t.Fatalf("entry %d identity = (%q, %d), want (%q, %d)", i, entry.Name, entry.ID, wantNames[i], wantIDs[i])
		}
		first := readFile(t, filepath.Join(firstDir, entry.File))
		second := readFile(t, filepath.Join(secondDir, entry.File))
		if !bytes.Equal(first, second) {
			t.Fatalf("%s differs between identical generator runs", entry.Name)
		}
		if len(first) != entry.ByteLength {
			t.Fatalf("%s byte length = %d, manifest says %d", entry.Name, len(first), entry.ByteLength)
		}
		digest := sha256.Sum256(first)
		if got := hex.EncodeToString(digest[:]); got != entry.SHA256 {
			t.Fatalf("%s sha256 = %s, manifest says %s", entry.Name, got, entry.SHA256)
		}
		if len(first) < 2 || first[0] != 0xfe {
			t.Fatalf("%s does not begin with raw batch header 0xfe", entry.Name)
		}

		payload := bytes.NewBuffer(first[1:])
		var declared uint32
		if err := protocol.Varuint32(payload, &declared); err != nil {
			t.Fatalf("%s length varuint: %v", entry.Name, err)
		}
		if int(declared) != payload.Len() {
			t.Fatalf("%s declared entry length = %d, remaining = %d", entry.Name, declared, payload.Len())
		}
		if got := payload.Bytes()[:2]; !reflect.DeepEqual(got, wantHeaders[i]) {
			t.Fatalf("%s header bytes = %x, want %x", entry.Name, got, wantHeaders[i])
		}
		if entry.Name == "AvailableCommandsLive356513" {
			const packetHeaderBytes = 2
			if got := payload.Len() - packetHeaderBytes; got != 356_513 {
				t.Fatalf("live AvailableCommands body length = %d, want 356513", got)
			}
		}
		if entry.Name == "BiomeDefinitionListChunkGeneration" {
			if entry.ByteLength != 48 {
				t.Fatalf("biome definition fixture length = %d, want 48", entry.ByteLength)
			}
			if entry.SHA256 != "a1a626d9b27cd943bc38fbbc356a09ea711ddb26acad72e284dd8dfaff94fbd4" {
				t.Fatalf("biome definition fixture sha256 = %s", entry.SHA256)
			}
			if entry.WireAuthority != "hashimthearab/gophertunnel" || entry.WireCommit != "c31450ff6e54b163acd72a95583ccaa71c001e6b" {
				t.Fatalf("biome definition fixture provenance = (%q, %q)", entry.WireAuthority, entry.WireCommit)
			}
		}
	}
}

func readFile(t *testing.T, path string) []byte {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return b
}
