package catalog

import (
	"context"
	_ "embed"
	"encoding/json"
	"reflect"
	"strconv"
	"strings"
	"testing"
)

//go:embed native_people_conformance.json
var nativePeopleConformance []byte

// TestNativePeopleConformance exercises the actual decorated-list decoder and projection shared with Rust.
func TestNativePeopleConformance(t *testing.T) {
	var cases []struct {
		Name     string           `json:"name"`
		People   []map[string]any `json:"people"`
		Expected []Person         `json:"expected"`
	}
	corpus := strings.ReplaceAll(string(nativePeopleConformance), "$SIDE", strconv.Itoa(gamerpicSide))
	if err := json.Unmarshal([]byte(corpus), &cases); err != nil {
		t.Fatal(err)
	}
	for _, tc := range cases {
		t.Run(tc.Name, func(t *testing.T) {
			var path string
			got, err := friendPeople(context.Background(), friendsClient(t, tc.People, &path))
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(got, tc.Expected) {
				t.Fatalf("people projection differs: got %#v, want %#v", got, tc.Expected)
			}
		})
	}
}
