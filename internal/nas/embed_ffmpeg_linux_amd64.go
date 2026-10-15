//go:build linux && amd64

package nas

import _ "embed"

//go:embed ffmpeglib/linux_amd64/ffmpeg
var embeddedFFmpeg []byte

func embeddedFFmpegName() string { return "ffmpeg" }
