package workspace

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

var (
	ErrOutsideWorkspace = errors.New("path is outside workspace")
	ErrNotFound         = errors.New("path not found")
)

// Workspace is the sandboxed root for all file operations.
type Workspace struct {
	Root string
}

// Open validates and opens a workspace at root.
func Open(root string) (*Workspace, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return nil, fmt.Errorf("resolve workspace path: %w", err)
	}
	info, err := os.Stat(abs)
	if err != nil {
		return nil, fmt.Errorf("workspace path: %w", err)
	}
	if !info.IsDir() {
		return nil, fmt.Errorf("workspace path is not a directory: %s", abs)
	}
	return &Workspace{Root: abs}, nil
}

// Resolve maps a client-relative path (POSIX-style, may be "" or "/") to an absolute path under Root.
func (w *Workspace) Resolve(rel string) (string, error) {
	rel = strings.TrimSpace(rel)
	rel = strings.TrimPrefix(rel, "/")
	rel = filepath.FromSlash(rel)

	clean := filepath.Clean(rel)
	if clean == "." {
		clean = ""
	}
	if strings.HasPrefix(clean, "..") {
		return "", ErrOutsideWorkspace
	}

	abs := filepath.Join(w.Root, clean)
	abs, err := filepath.Abs(abs)
	if err != nil {
		return "", err
	}

	root := w.Root + string(os.PathSeparator)
	if abs != w.Root && !strings.HasPrefix(abs, root) {
		return "", ErrOutsideWorkspace
	}
	return abs, nil
}

// Rel returns the path relative to workspace root for API responses.
func (w *Workspace) Rel(abs string) (string, error) {
	abs, err := filepath.Abs(abs)
	if err != nil {
		return "", err
	}
	rel, err := filepath.Rel(w.Root, abs)
	if err != nil {
		return "", err
	}
	if rel == "." {
		return "", nil
	}
	if strings.HasPrefix(rel, "..") {
		return "", ErrOutsideWorkspace
	}
	return filepath.ToSlash(rel), nil
}
