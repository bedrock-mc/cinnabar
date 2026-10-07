package localworld

import (
	"encoding/binary"
	"testing"
	"unicode/utf16"
)

func decodeJunction(t *testing.T, buf []byte) (substitute, print string) {
	t.Helper()
	le := binary.LittleEndian
	if le.Uint32(buf) != reparseTagMountPoint || int(le.Uint16(buf[4:]))+8 != len(buf) {
		t.Fatalf("bad header % x", buf[:8])
	}
	field := func(offset, length uint16) string {
		units := make([]uint16, length/2)
		for i := range units {
			units[i] = le.Uint16(buf[16+int(offset)+2*i:])
		}
		if le.Uint16(buf[16+int(offset)+int(length):]) != 0 {
			t.Fatal("name is not NUL-terminated")
		}
		return string(utf16.Decode(units))
	}
	return field(le.Uint16(buf[8:]), le.Uint16(buf[10:])), field(le.Uint16(buf[12:]), le.Uint16(buf[14:]))
}

// Folder names that cmd.exe would mangle (`&`, `^`, `%`, spaces) must reach the junction verbatim.
func TestJunctionReparseDataCarriesShellMetacharactersVerbatim(t *testing.T) {
	for _, tc := range []struct{ target, substitute, print string }{
		{`C:\Users\A & B\worlds\^x %PATH% é`, `\??\C:\Users\A & B\worlds\^x %PATH% é`, `C:\Users\A & B\worlds\^x %PATH% é`},
		{`C:/Users/me/db`, `\??\C:\Users\me\db`, `C:\Users\me\db`},
		{`\\?\D:\long path`, `\??\D:\long path`, `D:\long path`},
		{`\\server\share\w&1`, `\??\UNC\server\share\w&1`, `\\server\share\w&1`},
		{`\\?\UNC\server\share`, `\??\UNC\server\share`, `\\server\share`},
	} {
		buf, err := junctionReparseData(tc.target)
		if err != nil {
			t.Fatalf("%q: %v", tc.target, err)
		}
		if sub, print := decodeJunction(t, buf); sub != tc.substitute || print != tc.print {
			t.Fatalf("%q -> %q, %q", tc.target, sub, print)
		}
	}
	for _, bad := range []string{`relative\db`, `\rooted`, `C:drive-relative`, ""} {
		if _, err := junctionReparseData(bad); err == nil {
			t.Fatalf("accepted non-absolute target %q", bad)
		}
	}
}
