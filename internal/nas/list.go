package nas

import (
	"fmt"
	"os"
	"path"
	"path/filepath"
	"strings"
)

func ResolveUnderRoot(root, req string) (string, error) {
	rootAbs, err := filepath.Abs(root)
	if err != nil {
		return "", err
	}
	rel := strings.TrimSpace(req)
	if rel == "" {
		rel = "/"
	}
	rel = path.Clean("/" + strings.ReplaceAll(rel, "\\", "/"))
	for _, part := range strings.Split(rel, "/") {
		if part == "" {
			continue
		}
		if part == ".." || strings.HasPrefix(part, ".") {
			return "", fmt.Errorf("非法路径")
		}
	}
	target := rootAbs
	if rel != "/" {
		target = filepath.Join(rootAbs, filepath.FromSlash(strings.TrimPrefix(rel, "/")))
	}
	targetAbs, err := filepath.Abs(target)
	if err != nil {
		return "", err
	}
	relToRoot, err := filepath.Rel(rootAbs, targetAbs)
	if err != nil || strings.HasPrefix(relToRoot, "..") {
		return "", fmt.Errorf("越界路径")
	}
	return targetAbs, nil
}

type PathItem struct {
	IfFile   bool    `json:"ifFile"`
	Size     *uint64 `json:"size,omitempty"`
	LastDate int64   `json:"lastDate"`
}

func ResolveFile(root, dir, name string) (string, error) {
	base := filepath.Base(strings.TrimSpace(name))
	if base == "" || base == "." || base == ".." || strings.HasPrefix(base, ".") {
		return "", fmt.Errorf("非法文件名")
	}
	if strings.ContainsAny(base, `/\`) {
		return "", fmt.Errorf("非法文件名")
	}
	dirAbs, err := ResolveUnderRoot(root, dir)
	if err != nil {
		return "", err
	}
	full := filepath.Join(dirAbs, base)
	fullAbs, err := filepath.Abs(full)
	if err != nil {
		return "", err
	}
	rootAbs, err := filepath.Abs(root)
	if err != nil {
		return "", err
	}
	relToRoot, err := filepath.Rel(rootAbs, fullAbs)
	if err != nil || strings.HasPrefix(relToRoot, "..") {
		return "", fmt.Errorf("越界路径")
	}
	info, err := os.Stat(fullAbs)
	if err != nil {
		return "", err
	}
	if info.IsDir() {
		return "", fmt.Errorf("是目录")
	}
	return fullAbs, nil
}

func ListDir(root, req string) (map[string]PathItem, error) {
	dir, err := ResolveUnderRoot(root, req)
	if err != nil {
		return nil, err
	}
	info, err := os.Stat(dir)
	if err != nil {
		return nil, err
	}
	if !info.IsDir() {
		return nil, fmt.Errorf("不是目录")
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}
	out := make(map[string]PathItem, len(entries))
	for _, item := range entries {
		name := item.Name()
		if name == "" || strings.HasPrefix(name, ".") {
			continue
		}
		isFile := !item.IsDir()
		entry := PathItem{IfFile: isFile}
		if meta, err := item.Info(); err == nil {
			entry.LastDate = meta.ModTime().UnixMilli()
			if isFile {
				n := uint64(0)
				if meta.Size() > 0 {
					n = uint64(meta.Size())
				}
				entry.Size = &n
			}
		} else if isFile {
			n := uint64(0)
			entry.Size = &n
		}
		out[name] = entry
	}
	return out, nil
}

func ParseStreamPath(raw string) string {
	s := strings.TrimSpace(strings.ReplaceAll(raw, "\\", "/"))
	if s == "" {
		return ""
	}
	if i := strings.LastIndex(s, "/nas-out/"); i >= 0 {
		s = strings.TrimPrefix(s[i+len("/nas-out/"):], "/")
		if s == "" {
			return ""
		}
		return path.Clean("/" + s)
	}
	if isLocalAbs(s) {
		return "/" + path.Base(s)
	}
	if strings.HasPrefix(s, "/") {
		return path.Clean(s)
	}
	return "/" + path.Base(s)
}

func isLocalAbs(s string) bool {
	if strings.HasPrefix(s, "/Users/") || strings.HasPrefix(s, "/home/") ||
		strings.HasPrefix(s, "/var/") || strings.HasPrefix(s, "/tmp/") ||
		strings.HasPrefix(s, "/private/") {
		return true
	}
	if len(s) >= 3 && s[1] == ':' && (s[2] == '/' || s[2] == '\\') {
		return true
	}
	return false
}

func HasExt(name string) bool {
	base := filepath.Base(strings.TrimSpace(name))
	i := strings.LastIndex(base, ".")
	return i > 0 && i < len(base)-1
}

func Create(root, dir, name string) string {
	base := strings.TrimSpace(name)
	if base == "" || base != filepath.Base(base) {
		return "invalid"
	}
	dest, err := ResolveWriteTarget(root, dir, base)
	if err != nil {
		return "invalid"
	}
	if _, err := os.Lstat(dest); err == nil {
		return "exists"
	} else if !os.IsNotExist(err) {
		return "failed"
	}
	if HasExt(base) {
		if err := os.WriteFile(dest, []byte{}, 0o644); err != nil {
			return "failed"
		}
		return ""
	}
	if err := os.Mkdir(dest, 0o755); err != nil {
		return "failed"
	}
	return ""
}

func Move(root, fromDir, name, toDir string) string {
	base := strings.TrimSpace(name)
	if base == "" || base != filepath.Base(base) || base == "." || base == ".." || strings.HasPrefix(base, ".") {
		return "invalid"
	}
	if strings.ContainsAny(base, `/\`) {
		return "invalid"
	}
	fromAbs, err := ResolveUnderRoot(root, fromDir)
	if err != nil {
		return "invalid"
	}
	toAbs, err := ResolveUnderRoot(root, toDir)
	if err != nil {
		return "invalid"
	}
	src := filepath.Join(fromAbs, base)
	dst := filepath.Join(toAbs, base)
	srcAbs, err := filepath.Abs(src)
	if err != nil {
		return "invalid"
	}
	dstAbs, err := filepath.Abs(dst)
	if err != nil {
		return "invalid"
	}
	if srcAbs == dstAbs {
		return ""
	}
	info, err := os.Lstat(srcAbs)
	if err != nil {
		if os.IsNotExist(err) {
			return "missing"
		}
		return "failed"
	}
	if info.IsDir() {
		rel, relErr := filepath.Rel(srcAbs, dstAbs)
		if relErr == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
			return "invalid"
		}
	}
	if _, err := os.Lstat(dstAbs); err == nil {
		return "exists"
	} else if !os.IsNotExist(err) {
		return "failed"
	}
	if err := os.Rename(srcAbs, dstAbs); err != nil {
		return "failed"
	}
	return ""
}

func Delete(root, dir, name string) string {
	base := strings.TrimSpace(name)
	if base == "" || base != filepath.Base(base) || base == "." || base == ".." || strings.HasPrefix(base, ".") {
		return "invalid"
	}
	if strings.ContainsAny(base, `/\`) {
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
	rootAbs, err := filepath.Abs(root)
	if err != nil {
		return "invalid"
	}
	relToRoot, err := filepath.Rel(rootAbs, srcAbs)
	if err != nil || relToRoot == "." || relToRoot == ".." || strings.HasPrefix(relToRoot, ".."+string(filepath.Separator)) {
		return "invalid"
	}
	if _, err := os.Lstat(srcAbs); err != nil {
		if os.IsNotExist(err) {
			return "missing"
		}
		return "failed"
	}
	if err := os.RemoveAll(srcAbs); err != nil {
		return "failed"
	}
	return ""
}

func Rename(root, dir, oldName, newName string) string {
	from := strings.TrimSpace(oldName)
	to := strings.TrimSpace(newName)
	if !validEntryName(from) || !validEntryName(to) {
		return "invalid"
	}
	fromAbs, err := ResolveUnderRoot(root, dir)
	if err != nil {
		return "invalid"
	}
	srcAbs, err := filepath.Abs(filepath.Join(fromAbs, from))
	if err != nil {
		return "invalid"
	}
	dstAbs, err := filepath.Abs(filepath.Join(fromAbs, to))
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
	srcInfo, err := os.Lstat(srcAbs)
	if err != nil {
		if os.IsNotExist(err) {
			return "missing"
		}
		return "failed"
	}
	if dstInfo, dstErr := os.Lstat(dstAbs); dstErr == nil {
		if !os.SameFile(srcInfo, dstInfo) {
			return "exists"
		}
		if srcAbs == dstAbs {
			return ""
		}
	} else if !os.IsNotExist(dstErr) {
		return "failed"
	}
	if err := os.Rename(srcAbs, dstAbs); err != nil {
		return "failed"
	}
	return ""
}

func validEntryName(name string) bool {
	base := strings.TrimSpace(name)
	if base == "" || base != filepath.Base(base) || base == "." || base == ".." || strings.HasPrefix(base, ".") {
		return false
	}
	if strings.ContainsAny(base, `/\`) {
		return false
	}
	return true
}

func ResolveWriteTarget(root, dir, name string) (string, error) {
	base := filepath.Base(strings.TrimSpace(name))
	if base == "" || base == "." || base == ".." || strings.HasPrefix(base, ".") {
		return "", fmt.Errorf("非法文件名")
	}
	if strings.ContainsAny(base, `/\`) {
		return "", fmt.Errorf("非法文件名")
	}
	return resolveWriteAbs(root, dir, base)
}

func ResolveWritePath(root, rawName string) (string, error) {
	rel := ParseStreamPath(rawName)
	if rel == "" || rel == "/" {
		return "", fmt.Errorf("非法路径")
	}
	if path.Dir(rel) == "/" {
		return "", fmt.Errorf("缺少目录，拒绝按文件名写入根目录")
	}
	dir := path.Dir(rel)
	name := path.Base(rel)
	if name == "" || name == "." || name == ".." || strings.HasPrefix(name, ".") {
		return "", fmt.Errorf("非法文件名")
	}
	return resolveWriteAbs(root, dir, name)
}

func resolveWriteAbs(root, dir, name string) (string, error) {
	dirAbs, err := ResolveUnderRoot(root, dir)
	if err != nil {
		return "", err
	}
	full := filepath.Join(dirAbs, name)
	fullAbs, err := filepath.Abs(full)
	if err != nil {
		return "", err
	}
	rootAbs, err := filepath.Abs(root)
	if err != nil {
		return "", err
	}
	relToRoot, err := filepath.Rel(rootAbs, fullAbs)
	if err != nil || strings.HasPrefix(relToRoot, "..") {
		return "", fmt.Errorf("越界路径")
	}
	if err := os.MkdirAll(filepath.Dir(fullAbs), 0o755); err != nil {
		return "", err
	}
	return fullAbs, nil
}
