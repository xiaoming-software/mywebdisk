//go:build windows && amd64

package nas

import _ "embed"

//go:embed ffmpeglib/windows_amd64/ffmpeg.exe
var embeddedFFmpeg []byte

func embeddedFFmpegName() string { return "ffmpeg.exe" }
