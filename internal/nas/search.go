package nas

import (
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

const (
	searchResultLimit = 80
	searchScanLimit   = 400
)

type SearchHit struct {
	Name     string `json:"name"`
	FilePath string `json:"filePath"`
	IsDir    bool   `json:"isDir"`
}

func Search(root, keyword string) ([]SearchHit, bool) {
	q := strings.ToLower(strings.TrimSpace(keyword))
	if q == "" {
		return nil, false
	}
	rootAbs, err := filepath.Abs(root)
	if err != nil {
		return nil, false
	}
	info, err := os.Stat(rootAbs)
	if err != nil || !info.IsDir() {
		return nil, false
	}
	hits := make([]SearchHit, 0, 32)
	truncated := false
	_ = filepath.WalkDir(rootAbs, func(p string, d fs.DirEntry, err error) error {
		if err != nil || d == nil {
			return nil
		}
		name := d.Name()
		if p == rootAbs {
			return nil
		}
		if name == "" || strings.HasPrefix(name, ".") {
			if d.IsDir() {
				return fs.SkipDir
			}
			return nil
		}
		if !strings.Contains(strings.ToLower(name), q) {
			return nil
		}
		parent, verr := virtualDir(rootAbs, filepath.Dir(p))
		if verr != nil {
			return nil
		}
		hits = append(hits, SearchHit{
			Name:     name,
			FilePath: parent,
			IsDir:    d.IsDir(),
		})
		if len(hits) >= searchScanLimit {
			truncated = true
			return fs.SkipAll
		}
		return nil
	})
	sort.SliceStable(hits, func(i, j int) bool {
		si := searchScore(hits[i].Name, q)
		sj := searchScore(hits[j].Name, q)
		if si != sj {
			return si < sj
		}
		if hits[i].Name != hits[j].Name {
			return strings.ToLower(hits[i].Name) < strings.ToLower(hits[j].Name)
		}
		return hits[i].FilePath < hits[j].FilePath
	})
	if len(hits) > searchResultLimit {
		hits = hits[:searchResultLimit]
		truncated = true
	}
	return hits, truncated
}

func searchScore(name, q string) int {
	n := strings.ToLower(name)
	if n == q {
		return 0
	}
	if strings.HasPrefix(n, q) {
		return 1
	}
	return 2
}

func virtualDir(rootAbs, abs string) (string, error) {
	rel, err := filepath.Rel(rootAbs, abs)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "", err
	}
	if rel == "." {
		return "/", nil
	}
	return "/" + filepath.ToSlash(rel), nil
}
