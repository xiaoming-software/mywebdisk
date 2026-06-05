package protocol

import "time"

// StreamType matches webrpc local callback framing.
const (
	StreamTypeFile = 1 // 文件流
	StreamTypeData = 2 // 数据流
)

// Request is a JSON control message on a data stream.
type Request struct {
	ID      string `json:"id"`
	Op      string `json:"op"`
	Path    string `json:"path"`
	Size    int64  `json:"size,omitempty"`
	Content string `json:"content,omitempty"`
	From    string `json:"from,omitempty"`
	To      string `json:"to,omitempty"`
}

// Response is returned on the data stream for most operations.
type Response struct {
	ID    string `json:"id"`
	OK    bool   `json:"ok"`
	Error string `json:"error,omitempty"`
	Data  any    `json:"data,omitempty"`
}

type FileEntry struct {
	Name    string    `json:"name"`
	Path    string    `json:"path"`
	IsDir   bool      `json:"isDir"`
	Size    int64     `json:"size"`
	ModTime time.Time `json:"modTime"`
}

type ListData struct {
	Path  string      `json:"path"`
	Items []FileEntry `json:"items"`
}

type StatData struct {
	Path    string    `json:"path"`
	IsDir   bool      `json:"isDir"`
	Size    int64     `json:"size"`
	ModTime time.Time `json:"modTime"`
}

const (
	OpList     = "list"
	OpStat     = "stat"
	OpMkdir    = "mkdir"
	OpDelete   = "delete"
	OpRename   = "rename"
	OpDownload       = "download"
	OpDownloadStream = "download_stream"
	OpUpload         = "upload"
	OpUploadDone     = "upload_done"
	OpWrite    = "write"
	OpHealth   = "health"
)

// MaxWriteBytes is the largest text payload accepted for in-memory write.
const MaxWriteBytes = 50 * 1024 * 1024
