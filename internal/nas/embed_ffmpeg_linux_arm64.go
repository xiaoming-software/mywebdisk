//go:build linux && arm64

package nas

import _ "embed"

//go:embed ffmpeglib/linux_arm64/ffmpeg
var embeddedFFmpeg []byte

func embeddedFFmpegName() string { return "ffmpeg" }
