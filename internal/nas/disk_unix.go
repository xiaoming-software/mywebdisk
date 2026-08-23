//go:build darwin || linux || freebsd || netbsd || openbsd

package nas

import "syscall"

func DiskUsage(path string) (total, free uint64, err error) {
	var st syscall.Statfs_t
	if err = syscall.Statfs(path, &st); err != nil {
		return 0, 0, err
	}
	bsize := uint64(st.Bsize)
	total = uint64(st.Blocks) * bsize
	free = uint64(st.Bavail) * bsize
	return total, free, nil
}
