package webrpc

import (
	"fmt"
	"os"
	"path/filepath"
)

func loadEmbeddedLibrary() (string, error) {
	if len(embeddedRuntimeLib) == 0 {
		return "", fmt.Errorf("embedded webrpc runtime library is empty (run sdk/build-embed-lib.sh before build)")
	}
	dir, err := os.MkdirTemp("", "mywebdisk-webrpc-*")
	if err != nil {
		return "", err
	}
	path := filepath.Join(dir, embeddedLibName())
	if err := os.WriteFile(path, embeddedRuntimeLib, 0o700); err != nil {
		_ = os.RemoveAll(dir)
		return "", err
	}
	return path, nil
}
