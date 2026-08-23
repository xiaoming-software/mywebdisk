//go:build windows

package nas

import (
	"syscall"
	"unsafe"
)

var (
	modkernel32            = syscall.NewLazyDLL("kernel32.dll")
	procGetDiskFreeSpaceEx = modkernel32.NewProc("GetDiskFreeSpaceExW")
)

func DiskUsage(path string) (total, free uint64, err error) {
	p, err := syscall.UTF16PtrFromString(path)
	if err != nil {
		return 0, 0, err
	}
	var avail, tot, totalFree uint64
	r1, _, callErr := procGetDiskFreeSpaceEx.Call(
		uintptr(unsafe.Pointer(p)),
		uintptr(unsafe.Pointer(&avail)),
		uintptr(unsafe.Pointer(&tot)),
		uintptr(unsafe.Pointer(&totalFree)),
	)
	if r1 == 0 {
		if callErr != nil {
			return 0, 0, callErr
		}
		return 0, 0, syscall.EINVAL
	}
	return tot, avail, nil
}
