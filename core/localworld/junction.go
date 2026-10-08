package localworld

import (
	"encoding/binary"
	"errors"
	"strings"
	"unicode/utf16"
)

const (
	reparseTagMountPoint  = 0xA0000003
	maxReparseBufferBytes = 16 * 1024
)

// junctionReparseData is the mount-point reparse buffer aiming a junction at the absolute Windows path target.
// The path travels as UTF-16 data, so no character in it (`&`, `^`, `%`, spaces) needs escaping.
func junctionReparseData(target string) ([]byte, error) {
	target = strings.ReplaceAll(target, "/", `\`)
	target = strings.TrimPrefix(target, `\\?\`)
	var substitute string
	switch {
	case len(target) >= 3 && target[1] == ':' && target[2] == '\\' && isASCIILetter(target[0]):
		substitute = `\??\` + target
	case strings.HasPrefix(target, `UNC\`):
		substitute, target = `\??\`+target, `\`+strings.TrimPrefix(target, "UNC")
	case strings.HasPrefix(target, `\\`) && len(target) > 2:
		substitute = `\??\UNC\` + target[2:]
	default:
		return nil, errors.New("junction target must be an absolute path")
	}
	sub, print := utf16.Encode([]rune(substitute)), utf16.Encode([]rune(target))
	subBytes, printBytes := 2*len(sub), 2*len(print)
	dataLen := 8 + subBytes + 2 + printBytes + 2
	if 8+dataLen > maxReparseBufferBytes {
		return nil, errors.New("junction target path is too long")
	}
	buf := make([]byte, 8+dataLen)
	le := binary.LittleEndian
	le.PutUint32(buf[0:], reparseTagMountPoint)
	le.PutUint16(buf[4:], uint16(dataLen))
	le.PutUint16(buf[10:], uint16(subBytes))
	le.PutUint16(buf[12:], uint16(subBytes+2))
	le.PutUint16(buf[14:], uint16(printBytes))
	at := 16
	for _, units := range [][]uint16{sub, print} {
		for _, u := range units {
			le.PutUint16(buf[at:], u)
			at += 2
		}
		at += 2 // NUL terminator
	}
	return buf, nil
}

func isASCIILetter(c byte) bool { return c|0x20 >= 'a' && c|0x20 <= 'z' }
