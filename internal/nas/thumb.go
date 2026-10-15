package nas

import (
	"bytes"
	"crypto/sha1"
	"encoding/hex"
	"fmt"
	"image"
	"image/color"
	"image/draw"
	_ "image/gif"
	"image/jpeg"
	_ "image/png"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

const (
	DefaultThumbEdge = 192
	thumbCacheDir    = ".mywebdisk_thumbs"
	thumbJPEGQuality = 72
)

// ThumbResult holds a JPEG thumbnail payload.
type ThumbResult struct {
	JPEG   []byte
	Edge   int
	Cached bool
}

func isImageExt(ext string) bool {
	switch ext {
	case ".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp":
		return true
	default:
		return false
	}
}

func isVideoExt(ext string) bool {
	switch ext {
	case ".mp4", ".m4v", ".mov", ".qt",
		".mkv", ".webm", ".avi", ".wmv", ".asf",
		".flv", ".f4v", ".3gp", ".3g2",
		".ts", ".m2ts", ".mts", ".mpeg", ".mpg", ".mpe", ".vob",
		".ogv",
		".rm", ".rmvb",
		".divx", ".xvid":
		return true
	default:
		return false
	}
}

// CanThumb reports whether the file name is a supported thumb source.
func CanThumb(name string) bool {
	ext := strings.ToLower(filepath.Ext(name))
	return isImageExt(ext) || isVideoExt(ext)
}

func thumbCachePath(root, absFile string, modUnix, size int64, maxEdge int) (string, error) {
	rootAbs, err := filepath.Abs(root)
	if err != nil {
		return "", err
	}
	rel, err := filepath.Rel(rootAbs, absFile)
	if err != nil {
		return "", err
	}
	sum := sha1.Sum([]byte(fmt.Sprintf("%s|%d|%d|%d", filepath.ToSlash(rel), modUnix, size, maxEdge)))
	dir := filepath.Join(rootAbs, thumbCacheDir)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return "", err
	}
	return filepath.Join(dir, hex.EncodeToString(sum[:])+".jpg"), nil
}

// MakeThumb builds (or reuses) a JPEG thumbnail for an existing file under root.
func MakeThumb(root, dir, name string, maxEdge int) (ThumbResult, error) {
	if maxEdge <= 0 {
		maxEdge = DefaultThumbEdge
	}
	if maxEdge > 512 {
		maxEdge = 512
	}
	local, err := ResolveFile(root, dir, name)
	if err != nil {
		return ThumbResult{}, err
	}
	st, err := os.Stat(local)
	if err != nil {
		return ThumbResult{}, err
	}
	if st.IsDir() {
		return ThumbResult{}, fmt.Errorf("是目录")
	}
	cache, err := thumbCachePath(root, local, st.ModTime().Unix(), st.Size(), maxEdge)
	if err != nil {
		return ThumbResult{}, err
	}
	if b, err := os.ReadFile(cache); err == nil && len(b) > 0 {
		return ThumbResult{JPEG: b, Edge: maxEdge, Cached: true}, nil
	}

	ext := strings.ToLower(filepath.Ext(name))
	var jpegBytes []byte
	switch {
	case isImageExt(ext):
		jpegBytes, err = thumbFromImageFile(local, maxEdge)
	case isVideoExt(ext):
		jpegBytes, err = thumbFromVideoFile(local, cache+".frame.jpg", maxEdge)
	default:
		err = fmt.Errorf("不支持的类型")
	}
	if err != nil {
		return ThumbResult{}, err
	}
	_ = os.WriteFile(cache, jpegBytes, 0o644)
	return ThumbResult{JPEG: jpegBytes, Edge: maxEdge, Cached: false}, nil
}

func thumbFromImageFile(path string, maxEdge int) ([]byte, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	img, _, err := image.Decode(f)
	if err != nil {
		return nil, err
	}
	return encodeJPEGThumb(img, maxEdge)
}

func thumbFromVideoFile(path, tmpFrame string, maxEdge int) ([]byte, error) {
	ffmpeg, err := ResolveFFmpeg()
	if err != nil {
		return nil, err
	}
	_ = os.Remove(tmpFrame)
	// Scale so the longer edge fits maxEdge; keep aspect ratio.
	vf := fmt.Sprintf("scale='min(%d,iw)':'min(%d,ih)':force_original_aspect_ratio=decrease", maxEdge, maxEdge)
	run := func(seekFront bool) error {
		args := []string{
			"-hide_banner", "-loglevel", "error",
			"-y",
		}
		if seekFront {
			args = append(args, "-ss", "1")
		}
		args = append(args,
			"-i", path,
			"-an",
			"-frames:v", "1",
			"-vf", vf,
			tmpFrame,
		)
		cmd := exec.Command(ffmpeg, args...)
		var stderr bytes.Buffer
		cmd.Stderr = &stderr
		if err := cmd.Run(); err != nil {
			msg := strings.TrimSpace(stderr.String())
			if msg == "" {
				return err
			}
			return fmt.Errorf("%v (%s)", err, msg)
		}
		return nil
	}
	if err := run(true); err != nil {
		// Retry from the start for very short clips.
		if err2 := run(false); err2 != nil {
			return nil, fmt.Errorf("ffmpeg: %v", err2)
		}
	}
	defer os.Remove(tmpFrame)
	return thumbFromImageFile(tmpFrame, maxEdge)
}

func encodeJPEGThumb(src image.Image, maxEdge int) ([]byte, error) {
	scaled := scaleToMaxEdge(src, maxEdge)
	var buf bytes.Buffer
	if err := jpeg.Encode(&buf, scaled, &jpeg.Options{Quality: thumbJPEGQuality}); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func scaleToMaxEdge(src image.Image, maxEdge int) image.Image {
	b := src.Bounds()
	w, h := b.Dx(), b.Dy()
	if w <= 0 || h <= 0 {
		return src
	}
	if w <= maxEdge && h <= maxEdge {
		return src
	}
	var nw, nh int
	if w >= h {
		nw = maxEdge
		nh = h * maxEdge / w
	} else {
		nh = maxEdge
		nw = w * maxEdge / h
	}
	if nw < 1 {
		nw = 1
	}
	if nh < 1 {
		nh = 1
	}
	dst := image.NewRGBA(image.Rect(0, 0, nw, nh))
	draw.Draw(dst, dst.Bounds(), &image.Uniform{C: color.Black}, image.Point{}, draw.Src)
	// Nearest-neighbor scale (no extra deps).
	for y := 0; y < nh; y++ {
		sy := b.Min.Y + y*h/nh
		for x := 0; x < nw; x++ {
			sx := b.Min.X + x*w/nw
			dst.Set(x, y, src.At(sx, sy))
		}
	}
	return dst
}
