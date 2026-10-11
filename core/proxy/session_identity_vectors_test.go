package proxy

import (
	"math"
	"testing"

	"github.com/google/uuid"
)

func TestInitialSelfSignedIDVectors(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name   string
		id     int64
		device string
		want   string
	}{
		{name: "byte order", id: 0x0102030405060708, device: "audit-device-id", want: "62c686b6-9dd2-3c26-ac8d-6da654ca1b3f"},
		{name: "all bits", id: -1, device: "audit-device-id", want: "3e6f158c-edd9-3059-b123-396df53ce814"},
		{name: "signed minimum", id: math.MinInt64, device: "audit-device-id", want: "5edb9ba0-4706-3dd7-a52b-24e7790f4d98"},
		{name: "signed maximum", id: math.MaxInt64, device: "audit-device-id", want: "8c2bc487-6daa-3b9b-8116-dfda6d1a32ce"},
		{name: "zero bits", id: 0, device: "audit-device-id", want: "6e6069e0-2a0c-3fca-97bd-72c46440d781"},
		{name: "utf8 and embedded nul", id: 0x0102030405060708, device: "device-é\x00tail", want: "aff3d1b5-6181-3b7d-b232-d537b387d064"},
		{name: "utf8 prefix", id: 0x0102030405060708, device: "device-é", want: "e1040f02-bde7-3249-86f4-01fdf813781e"},
		{name: "trailing nul", id: 0x0102030405060708, device: "audit-device-id\x00", want: "c41e8c8c-b0a1-343f-ba63-3c9c1a175ec4"},
		{name: "device format", id: 0x0102030405060708, device: "00112233445566778899aabbccddeeff", want: "838a88a9-913f-32d3-8ab3-a5dc55cc55a0"},
		{name: "changed stored ID", id: 0x0102030405060709, device: "00112233445566778899aabbccddeeff", want: "99a48525-eafb-3cbd-b505-f9eaccbe7e19"},
		{name: "changed device", id: 0x0102030405060708, device: "00112233445566778899aabbccddeefe", want: "b8cd9d6a-158b-34ea-9b6b-04ee7c3f4387"},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			got := initialSelfSignedID(test.id, test.device)
			if got != test.want {
				t.Fatalf("SelfSignedId = %q, want %q", got, test.want)
			}
			id, err := uuid.Parse(got)
			if err != nil {
				t.Fatal(err)
			}
			if id.Version() != 3 || id.Variant() != uuid.RFC4122 {
				t.Fatalf("version/variant = %v/%v", id.Version(), id.Variant())
			}
		})
	}
}
