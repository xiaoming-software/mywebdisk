package nas

import (
	"os"
	"path/filepath"
	"testing"
)

func TestResolveFFmpeg(t *testing.T) {
	p, err := ResolveFFmpeg()
	if err != nil {
		t.Fatalf("ResolveFFmpeg: %v", err)
	}
	if p == "" {
		t.Fatal("empty ffmpeg path")
	}
	t.Logf("ffmpeg=%s", p)
}

func TestVideoThumb(t *testing.T) {
	candidates := []string{
		filepath.Join("..", "..", "..", "File2File-App", "tmp", "sample.mp4"),
		"/Users/a1/Documents/GitHub/File2File-App/tmp/sample.mp4",
	}
	var sample string
	for _, c := range candidates {
		if st, err := os.Stat(c); err == nil && st.Size() > 0 {
			sample = c
			break
		}
	}
	if sample == "" {
		t.Skip("sample.mp4 not found")
	}

	root := t.TempDir()
	dir := filepath.Join(root, "clips")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	dst := filepath.Join(dir, "sample.mp4")
	b, err := os.ReadFile(sample)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(dst, b, 0o644); err != nil {
		t.Fatal(err)
	}

	res, err := MakeThumb(root, "/clips", "sample.mp4", 128)
	if err != nil {
		t.Fatalf("MakeThumb video: %v", err)
	}
	if len(res.JPEG) < 100 {
		t.Fatalf("jpeg too small: %d", len(res.JPEG))
	}
	t.Logf("video thumb jpeg=%d bytes cached=%v", len(res.JPEG), res.Cached)
}

func TestCanThumbVideoExts(t *testing.T) {
	for _, name := range []string{"a.mp4", "b.MOV", "c.mkv", "d.webm", "e.avi", "f.wmv", "g.flv", "h.3gp", "i.ts", "j.m2ts", "k.mpeg", "l.ogv", "m.rmvb"} {
		if !CanThumb(name) {
			t.Errorf("expected CanThumb(%q)", name)
		}
	}
}
