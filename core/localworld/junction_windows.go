//go:build windows

package localworld

import (
	"errors"
	"os"
	"path/filepath"

	"golang.org/x/sys/windows"
)

// createWorldLink makes link a junction to target; junctions need no privilege, unlike symlinks.
func createWorldLink(link, target string) error {
	abs, err := filepath.Abs(target)
	if err != nil {
		return err
	}
	data, err := junctionReparseData(abs)
	if err != nil {
		return err
	}
	if err := os.Mkdir(link, 0o700); err != nil {
		return err
	}
	if err := setReparsePoint(link, data); err != nil {
		_ = os.Remove(link)
		return err
	}
	return nil
}

func setReparsePoint(dir string, data []byte) error {
	path, err := windows.UTF16PtrFromString(dir)
	if err != nil {
		return err
	}
	handle, err := windows.CreateFile(path, windows.GENERIC_WRITE, 0, nil, windows.OPEN_EXISTING,
		windows.FILE_FLAG_OPEN_REPARSE_POINT|windows.FILE_FLAG_BACKUP_SEMANTICS, 0)
	if err != nil {
		return err
	}
	var returned uint32
	ioErr := windows.DeviceIoControl(handle, windows.FSCTL_SET_REPARSE_POINT, &data[0], uint32(len(data)), nil, 0, &returned, nil)
	return errors.Join(ioErr, windows.CloseHandle(handle))
}
