//go:build windows

package webrpc

import (
	"fmt"
	"syscall"
	"unsafe"
)

type handle uintptr

var (
	hLib     syscall.Handle
	fnNew    uintptr
	fnFree   uintptr
	fnLogin  uintptr
	fnPort   uintptr
	fnOpen   uintptr
	fnClose  uintptr
	fnSess   uintptr
	fnSend   uintptr
	fnFile   uintptr
)

func openEmbedded() error {
	path, err := loadEmbeddedLibrary()
	if err != nil {
		return err
	}
	h, err := syscall.LoadLibrary(path)
	if err != nil {
		return fmt.Errorf("LoadLibrary: %w", err)
	}
	hLib = h

	resolve := func(name string) (uintptr, error) {
		p, err := syscall.GetProcAddress(hLib, name)
		if err != nil {
			return 0, err
		}
		return p, nil
	}

	var e error
	if fnNew, e = resolve("WebrpcClient_New"); e != nil {
		return e
	}
	if fnFree, e = resolve("WebrpcClient_Free"); e != nil {
		return e
	}
	if fnLogin, e = resolve("WebrpcClient_LoginStatus"); e != nil {
		return e
	}
	if fnPort, e = resolve("WebrpcClient_GetReceivePort"); e != nil {
		return e
	}
	if fnOpen, e = resolve("WebrpcClient_OpenSession"); e != nil {
		return e
	}
	if fnClose, e = resolve("WebrpcClient_CloseSession"); e != nil {
		return e
	}
	if fnSess, e = resolve("WebrpcClient_SessionSize"); e != nil {
		return e
	}
	if fnSend, e = resolve("WebrpcClient_SendData"); e != nil {
		return e
	}
	if fnFile, e = resolve("WebrpcClient_SendFile"); e != nil {
		return e
	}
	return nil
}

func cNew(token, passwd, permission string) handle {
	ct, _ := syscall.BytePtrFromString(token)
	cp, _ := syscall.BytePtrFromString(passwd)
	cperm, _ := syscall.BytePtrFromString(permission)
	r, _, _ := syscall.SyscallN(fnNew, uintptr(unsafe.Pointer(ct)), uintptr(unsafe.Pointer(cp)), uintptr(unsafe.Pointer(cperm)))
	return handle(r)
}

func cFree(h handle) { _, _, _ = syscall.SyscallN(fnFree, uintptr(h)) }

func cLoginStatus(h handle) int {
	r, _, _ := syscall.SyscallN(fnLogin, uintptr(h))
	return int(r)
}

func cReceivePort(h handle) int {
	r, _, _ := syscall.SyscallN(fnPort, uintptr(h))
	return int(r)
}

func cOpenSession(h handle, toToken, permission string) uint32 {
	ct, _ := syscall.BytePtrFromString(toToken)
	cp, _ := syscall.BytePtrFromString(permission)
	r, _, _ := syscall.SyscallN(fnOpen, uintptr(h), uintptr(unsafe.Pointer(ct)), uintptr(unsafe.Pointer(cp)))
	return uint32(r)
}

func cCloseSession(h handle, sessionID uint32) int {
	r, _, _ := syscall.SyscallN(fnClose, uintptr(h), uintptr(sessionID))
	return int(r)
}

func cSessionSize(h handle) int {
	r, _, _ := syscall.SyscallN(fnSess, uintptr(h))
	return int(r)
}

func cSendData(h handle, sessionID uint32, data []byte, timeoutMs int64) int {
	var p uintptr
	if len(data) > 0 {
		p = uintptr(unsafe.Pointer(&data[0]))
	}
	r, _, _ := syscall.SyscallN(fnSend, uintptr(h), uintptr(sessionID), p, uintptr(len(data)), uintptr(timeoutMs))
	return int(r)
}

func cSendFile(h handle, sessionID uint32, filePath string) int {
	cs, _ := syscall.BytePtrFromString(filePath)
	r, _, _ := syscall.SyscallN(fnFile, uintptr(h), uintptr(sessionID), uintptr(unsafe.Pointer(cs)))
	return int(r)
}
