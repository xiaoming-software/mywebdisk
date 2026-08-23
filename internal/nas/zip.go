package nas

import (
	"archive/zip"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"time"
)

func ZipFolder(root, dir, name string, progress func(int)) string {
	base := strings.TrimSpace(name)
	if !validEntryName(base) {
		return "invalid"
	}
	fromAbs, err := ResolveUnderRoot(root, dir)
	if err != nil {
		return "invalid"
	}
	srcAbs, err := filepath.Abs(filepath.Join(fromAbs, base))
	if err != nil {
		return "invalid"
	}
	zipName := base + ".zip"
	if !validEntryName(zipName) {
		return "invalid"
	}
	dstAbs, err := filepath.Abs(filepath.Join(fromAbs, zipName))
	if err != nil {
		return "invalid"
	}
	rootAbs, err := filepath.Abs(root)
	if err != nil {
		return "invalid"
	}
	for _, p := range []string{srcAbs, dstAbs} {
		rel, relErr := filepath.Rel(rootAbs, p)
		if relErr != nil || rel == "." || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
			return "invalid"
		}
	}
	info, err := os.Lstat(srcAbs)
	if err != nil {
		if os.IsNotExist(err) {
			return "missing"
		}
		return "failed"
	}
	if !info.IsDir() {
		return "invalid"
	}
	if _, err := os.Lstat(dstAbs); err == nil {
		return "exists"
	} else if !os.IsNotExist(err) {
		return "failed"
	}

	total := zipUncompressedBytes(srcAbs)
	out, err := os.OpenFile(dstAbs, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		if os.IsExist(err) {
			return "exists"
		}
		return "failed"
	}
	failed := true
	defer func() {
		if failed {
			out.Close()
			_ = os.Remove(dstAbs)
		}
	}()

	zw := zip.NewWriter(out)
	var wrote uint64
	counter := &zipProgress{
		wrote:    &wrote,
		total:    total,
		progress: progress,
	}
	if progress != nil {
		progress(0)
	}
	walkErr := filepath.WalkDir(srcAbs, func(p string, d fs.DirEntry, err error) error {
		if err != nil || d == nil {
			return nil
		}
		if p == srcAbs {
			return nil
		}
		name := d.Name()
		if name == "" || strings.HasPrefix(name, ".") {
			if d.IsDir() {
				return fs.SkipDir
			}
			return nil
		}
		if d.Type()&os.ModeSymlink != 0 {
			return nil
		}
		rel, relErr := filepath.Rel(srcAbs, p)
		if relErr != nil {
			return nil
		}
		entry := filepath.ToSlash(rel)
		if entry == "" || strings.HasPrefix(entry, "../") {
			return nil
		}
		if d.IsDir() {
			_, err := zw.Create(entry + "/")
			return err
		}
		header := &zip.FileHeader{
			Name:     entry,
			Method:   zip.Deflate,
			Modified: time.Now(),
		}
		if meta, infoErr := d.Info(); infoErr == nil {
			header.Modified = meta.ModTime()
		}
		w, err := zw.CreateHeader(header)
		if err != nil {
			return err
		}
		f, err := os.Open(p)
		if err != nil {
			return err
		}
		counter.w = w
		_, copyErr := io.Copy(counter, f)
		f.Close()
		return copyErr
	})
	closeErr := zw.Close()
	fileErr := out.Close()
	if walkErr != nil || closeErr != nil || fileErr != nil {
		return "failed"
	}
	failed = false
	if progress != nil {
		progress(100)
	}
	return ""
}

func zipUncompressedBytes(srcAbs string) uint64 {
	var n uint64
	_ = filepath.WalkDir(srcAbs, func(p string, d fs.DirEntry, err error) error {
		if err != nil || d == nil {
			return nil
		}
		if p == srcAbs {
			return nil
		}
		name := d.Name()
		if name == "" || strings.HasPrefix(name, ".") {
			if d.IsDir() {
				return fs.SkipDir
			}
			return nil
		}
		if d.IsDir() || d.Type()&os.ModeSymlink != 0 {
			return nil
		}
		if meta, infoErr := d.Info(); infoErr == nil && meta.Size() > 0 {
			n += uint64(meta.Size())
		}
		return nil
	})
	return n
}

type zipProgress struct {
	w        io.Writer
	wrote    *uint64
	total    uint64
	last     time.Time
	lastPct  int
	progress func(int)
}

func (c *zipProgress) Write(p []byte) (int, error) {
	n, err := c.w.Write(p)
	if n > 0 {
		*c.wrote += uint64(n)
		c.maybeReport()
	}
	return n, err
}

func (c *zipProgress) maybeReport() {
	if c.progress == nil || c.total == 0 {
		return
	}
	pct := int((*c.wrote * 100) / c.total)
	if pct > 99 {
		pct = 99
	}
	now := time.Now()
	if pct == c.lastPct && now.Sub(c.last) < 200*time.Millisecond {
		return
	}
	c.lastPct = pct
	c.last = now
	c.progress(pct)
}
