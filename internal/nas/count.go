package nas

import (
	"io/fs"
	"path/filepath"
)

// CountFiles walks root and counts non-directory entries (files, symlinks, etc).
func CountFiles(root string) int64 {
	var n int64
	_ = filepath.WalkDir(root, func(_ string, d fs.DirEntry, err error) error {
		if err != nil {
			return nil
		}
		if d != nil && !d.IsDir() {
			n++
		}
		return nil
	})
	return n
}
