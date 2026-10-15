//go:build darwin && arm64

package nas

import _ "embed"

//go:embed ffmpeglib/darwin_arm64/ffmpeg
var embeddedFFmpeg []byte

func embeddedFFmpegName() string { return "ffmpeg" }
