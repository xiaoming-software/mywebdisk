//go:build darwin || linux

package webrpc

/*
#cgo darwin LDFLAGS: -ldl
#cgo linux LDFLAGS: -ldl

#include <dlfcn.h>
#include <stdlib.h>
#include <stdint.h>

typedef uintptr_t GoUintptr;

static void *p_new, *p_free, *p_login, *p_port, *p_open, *p_close, *p_sess, *p_send, *p_file;

static int webrpc_load(const char *path) {
    void *lib = dlopen(path, RTLD_NOW);
    if (!lib) return -1;
    *(void**)&p_new = dlsym(lib, "WebrpcClient_New");
    *(void**)&p_free = dlsym(lib, "WebrpcClient_Free");
    *(void**)&p_login = dlsym(lib, "WebrpcClient_LoginStatus");
    *(void**)&p_port = dlsym(lib, "WebrpcClient_GetReceivePort");
    *(void**)&p_open = dlsym(lib, "WebrpcClient_OpenSession");
    *(void**)&p_close = dlsym(lib, "WebrpcClient_CloseSession");
    *(void**)&p_sess = dlsym(lib, "WebrpcClient_SessionSize");
    *(void**)&p_send = dlsym(lib, "WebrpcClient_SendData");
    *(void**)&p_file = dlsym(lib, "WebrpcClient_SendFile");
    if (!p_new || !p_free || !p_login || !p_port || !p_open || !p_close || !p_sess || !p_send || !p_file) return -2;
    return 0;
}

static GoUintptr w_new(char *a, char *b, char *c) {
    return ((GoUintptr (*)(char*,char*,char*))p_new)(a,b,c);
}
static void w_free(GoUintptr h) { ((void (*)(GoUintptr))p_free)(h); }
static int w_login(GoUintptr h) { return ((int (*)(GoUintptr))p_login)(h); }
static int w_port(GoUintptr h) { return ((int (*)(GoUintptr))p_port)(h); }
static unsigned int w_open(GoUintptr h, char *t, char *p) {
    return ((unsigned int (*)(GoUintptr,char*,char*))p_open)(h,t,p);
}
static int w_close(GoUintptr h, unsigned int sid) {
    return ((int (*)(GoUintptr,unsigned int))p_close)(h,sid);
}
static unsigned short w_sess(GoUintptr h) { return ((unsigned short (*)(GoUintptr))p_sess)(h); }
static int w_send(GoUintptr h, unsigned int sid, char *d, int n, long long t) {
    return ((int (*)(GoUintptr,unsigned int,char*,int,long long))p_send)(h,sid,d,n,t);
}
static int w_file(GoUintptr h, unsigned int sid, char *p) {
    return ((int (*)(GoUintptr,unsigned int,char*))p_file)(h,sid,p);
}
*/
import "C"

import (
	"fmt"
	"unsafe"
)

type handle uintptr

func openEmbedded() error {
	path, err := loadEmbeddedLibrary()
	if err != nil {
		return err
	}
	cs := C.CString(path)
	defer C.free(unsafe.Pointer(cs))
	if C.webrpc_load(cs) != 0 {
		return fmt.Errorf("load embedded webrpc at %s failed", path)
	}
	return nil
}

func cNew(token, passwd, permission string) handle {
	ct := C.CString(token)
	cp := C.CString(passwd)
	cperm := C.CString(permission)
	defer C.free(unsafe.Pointer(ct))
	defer C.free(unsafe.Pointer(cp))
	defer C.free(unsafe.Pointer(cperm))
	return handle(C.w_new(ct, cp, cperm))
}

func cFree(h handle) { C.w_free(C.GoUintptr(h)) }

func cLoginStatus(h handle) int { return int(C.w_login(C.GoUintptr(h))) }

func cReceivePort(h handle) int { return int(C.w_port(C.GoUintptr(h))) }

func cOpenSession(h handle, toToken, permission string) uint32 {
	ct := C.CString(toToken)
	cp := C.CString(permission)
	defer C.free(unsafe.Pointer(ct))
	defer C.free(unsafe.Pointer(cp))
	return uint32(C.w_open(C.GoUintptr(h), ct, cp))
}

func cCloseSession(h handle, sessionID uint32) int {
	return int(C.w_close(C.GoUintptr(h), C.uint(sessionID)))
}

func cSessionSize(h handle) int { return int(C.w_sess(C.GoUintptr(h))) }

func cSendData(h handle, sessionID uint32, data []byte, timeoutMs int64) int {
	ch := C.GoUintptr(h)
	if len(data) == 0 {
		return int(C.w_send(ch, C.uint(sessionID), nil, 0, C.longlong(timeoutMs)))
	}
	return int(C.w_send(ch, C.uint(sessionID), (*C.char)(unsafe.Pointer(&data[0])), C.int(len(data)), C.longlong(timeoutMs)))
}

func cSendFile(h handle, sessionID uint32, filePath string) int {
	cs := C.CString(filePath)
	defer C.free(unsafe.Pointer(cs))
	return int(C.w_file(C.GoUintptr(h), C.uint(sessionID), cs))
}
