package nas

import (
	"crypto/sha1"
	"encoding/hex"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
)

// Must match the ffmpeg-static release tag used by build.sh.
const ffmpegEmbedTag = "b6.1.1"

var (
	ffmpegOnce sync.Once
	ffmpegPath string
	ffmpegErr  error
)

// ResolveFFmpeg returns a path to an ffmpeg binary.
// Prefer the platform binary embedded by ./build.sh; fall back to PATH.
func ResolveFFmpeg() (string, error) {
	ffmpegOnce.Do(func() {
		if p, err := extractEmbeddedFFmpeg(); err == nil && p != "" {
			ffmpegPath = p
			return
		}
		if p, err := exec.LookPath("ffmpeg"); err == nil {
			ffmpegPath = p
			return
		}
		ffmpegErr = fmt.Errorf("未嵌入 ffmpeg，且 PATH 中也找不到（请用 ./build.sh 编译）")
	})
	return ffmpegPath, ffmpegErr
}

func extractEmbeddedFFmpeg() (string, error) {
	if len(embeddedFFmpeg) == 0 {
		return "", fmt.Errorf("empty embedded ffmpeg")
	}
	sum := sha1.Sum(embeddedFFmpeg)
	sub := filepath.Join("mywebdisk", "ffmpeg-"+ffmpegEmbedTag+"-"+hex.EncodeToString(sum[:8]))
	candidates := make([]string, 0, 2)
	if cacheRoot, err := os.UserCacheDir(); err == nil && cacheRoot != "" {
		candidates = append(candidates, filepath.Join(cacheRoot, sub))
	}
	candidates = append(candidates, filepath.Join(os.TempDir(), sub))

	var lastErr error
	for _, dir := range candidates {
		dest, err := writeFFmpegBinary(dir)
		if err == nil {
			return dest, nil
		}
		lastErr = err
	}
	if lastErr == nil {
		lastErr = fmt.Errorf("no cache dir")
	}
	return "", lastErr
}

func writeFFmpegBinary(dir string) (string, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return "", err
	}
	dest := filepath.Join(dir, embeddedFFmpegName())
	if st, err := os.Stat(dest); err == nil && st.Size() == int64(len(embeddedFFmpeg)) {
		return dest, nil
	}
	tmp := dest + ".tmp"
	if err := os.WriteFile(tmp, embeddedFFmpeg, 0o755); err != nil {
		return "", err
	}
	if err := os.Rename(tmp, dest); err != nil {
		_ = os.Remove(tmp)
		return "", err
	}
	return dest, nil
}
