package localworld

import (
	"slices"
	"strconv"
	"strings"
	"testing"
)

func TestBDSPlayerLimitConfiguration(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name       string
		maxPlayers int
	}{
		{name: "shared world", maxPlayers: 8},
		{name: "larger shared world", maxPlayers: 16},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			spec := testSpec()
			count := strconv.Itoa(test.maxPlayers)
			props := string(serverProperties(spec, 5000, test.maxPlayers, false))
			if !strings.Contains(props, "max-players="+count+"\n") {
				t.Fatalf("native properties did not preserve player limit:\n%s", props)
			}
			args := containerArgs(
				spec,
				testImage,
				"test-version",
				t.TempDir(),
				5000,
				test.maxPlayers,
				false,
				0,
			)
			if !slices.Contains(args, "MAX_PLAYERS="+count) {
				t.Fatalf("container environment did not preserve player limit: %v", args)
			}
		})
	}
}
