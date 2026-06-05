package workspace

import (
	"os"
	"path/filepath"
	"testing"
)

func TestResolve(t *testing.T) {
	root := t.TempDir()
	ws, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}

	cases := []struct {
		rel   string
		ok    bool
		join  string
	}{
		{"", true, ""},
		{"/", true, ""},
		{"docs/readme.txt", true, "docs/readme.txt"},
		{"../etc/passwd", false, ""},
		{"foo/../../etc/passwd", false, ""},
	}

	for _, c := range cases {
		abs, err := ws.Resolve(c.rel)
		if c.ok {
			if err != nil {
				t.Fatalf("Resolve(%q): %v", c.rel, err)
			}
			want := filepath.Join(root, filepath.FromSlash(c.join))
			if abs != want {
				t.Fatalf("Resolve(%q) = %q, want %q", c.rel, abs, want)
			}
		} else if err == nil {
			t.Fatalf("Resolve(%q) expected error", c.rel)
		}
	}
}

func TestOpenRequiresDirectory(t *testing.T) {
	f := filepath.Join(t.TempDir(), "file.txt")
	if err := os.WriteFile(f, []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(f); err == nil {
		t.Fatal("expected error for file path")
	}
}
