package disk

import (
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"log"
	"os"
	"path/filepath"
	"sync"

	"github.com/mywebdisk/mywebdisk-server/internal/protocol"
	"github.com/mywebdisk/mywebdisk-server/internal/workspace"
)

const sendTimeoutMs = 30000

// Handler serves workspace file operations over webrpc streams.
type Handler struct {
	WS     *workspace.Workspace
	Client Sender // *webrpc.Client

	mu      sync.Mutex
	uploads map[uint32]*uploadSession // sessionId -> active upload
}

type uploadSession struct {
	path          string
	reqID         string
	absPath       string
	file          *os.File
	bytesReceived int64
}

func NewHandler(ws *workspace.Workspace, client Sender) *Handler {
	return &Handler{
		WS:      ws,
		Client:  client,
		uploads: make(map[uint32]*uploadSession),
	}
}

func (h *Handler) HandleData(sessionID uint32, payload []byte) {
	var req protocol.Request
	if err := json.Unmarshal(payload, &req); err != nil {
		h.reply(sessionID, protocol.Response{
			ID:    "",
			OK:    false,
			Error: "invalid JSON request",
		})
		return
	}

	switch req.Op {
	case protocol.OpHealth:
		h.reply(sessionID, protocol.Response{
			ID: req.ID, OK: true,
			Data: map[string]string{"status": "ok", "workspace": h.WS.Root},
		})
	case protocol.OpList:
		h.handleList(sessionID, req)
	case protocol.OpStat:
		h.handleStat(sessionID, req)
	case protocol.OpMkdir:
		h.handleMkdir(sessionID, req)
	case protocol.OpDelete:
		h.handleDelete(sessionID, req)
	case protocol.OpRename:
		h.handleRename(sessionID, req)
	case protocol.OpDownload:
		h.handleDownload(sessionID, req)
	case protocol.OpDownloadStream:
		h.handleDownloadStream(sessionID, req)
	case protocol.OpUpload:
		h.handleUploadPrepare(sessionID, req)
	case protocol.OpUploadDone:
		h.handleUploadDone(sessionID, req)
	case protocol.OpWrite:
		h.handleWrite(sessionID, req)
	default:
		h.reply(sessionID, protocol.Response{
			ID: req.ID, OK: false, Error: "unknown op: " + req.Op,
		})
	}
}

func (h *Handler) HandleFile(sessionID uint32, fileName, fileData []byte) {
	h.mu.Lock()
	sess, ok := h.uploads[sessionID]
	h.mu.Unlock()

	if !ok || sess.file == nil {
		log.Printf("session %d: unexpected file stream %q", sessionID, fileName)
		return
	}

	h.mu.Lock()
	defer h.mu.Unlock()
	sess, ok = h.uploads[sessionID]
	if !ok || sess.file == nil {
		return
	}
	n, err := sess.file.Write(fileData)
	if err != nil {
		log.Printf("session %d: upload append %q: %v", sessionID, sess.absPath, err)
		return
	}
	sess.bytesReceived += int64(n)
	log.Printf(
		"session %d: upload chunk %d bytes (total %d) id=%s path=%q",
		sessionID, len(fileData), sess.bytesReceived, sess.reqID, sess.path,
	)
}

func (h *Handler) handleUploadPrepare(sessionID uint32, req protocol.Request) {
	if req.Path == "" {
		h.reply(sessionID, protocol.Response{ID: req.ID, OK: false, Error: "path is required"})
		return
	}
	abs, err := h.WS.Resolve(req.Path)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if err := os.MkdirAll(filepath.Dir(abs), 0o755); err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}

	h.mu.Lock()
	if old, ok := h.uploads[sessionID]; ok && old.file != nil {
		_ = old.file.Close()
		delete(h.uploads, sessionID)
	}
	h.mu.Unlock()

	f, err := os.OpenFile(abs, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o644)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}

	h.mu.Lock()
	h.uploads[sessionID] = &uploadSession{
		path:    req.Path,
		reqID:   req.ID,
		absPath: abs,
		file:    f,
	}
	h.mu.Unlock()

	h.reply(sessionID, protocol.Response{
		ID: req.ID, OK: true,
		Data: map[string]string{"message": "send file stream on this session"},
	})
}

func (h *Handler) handleUploadDone(sessionID uint32, req protocol.Request) {
	h.mu.Lock()
	sess, ok := h.uploads[sessionID]
	if !ok || sess.reqID != req.ID {
		h.mu.Unlock()
		h.reply(sessionID, protocol.Response{
			ID: req.ID, OK: false, Error: "no active upload for this request",
		})
		return
	}
	delete(h.uploads, sessionID)
	h.mu.Unlock()

	if err := sess.file.Close(); err != nil {
		log.Printf("session %d: upload close %q: %v", sessionID, sess.absPath, err)
		h.reply(sessionID, protocol.Response{ID: req.ID, OK: false, Error: "failed to finalize upload"})
		return
	}
	sess.file = nil

	info, err := os.Stat(sess.absPath)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	expected := req.Size
	if expected <= 0 {
		expected = sess.bytesReceived
	}
	if info.Size() != expected {
		h.reply(sessionID, protocol.Response{
			ID: req.ID, OK: false,
			Error: "upload size mismatch",
			Data: map[string]any{
				"expected": expected,
				"received": info.Size(),
			},
		})
		return
	}

	apiPath, _ := h.WS.Rel(sess.absPath)
	h.reply(sessionID, protocol.Response{
		ID: req.ID, OK: true,
		Data: map[string]any{"path": apiPath, "size": info.Size()},
	})
	log.Printf("session %d: uploaded %s (%d bytes) id=%s", sessionID, apiPath, info.Size(), req.ID)
}

func (h *Handler) handleWrite(sessionID uint32, req protocol.Request) {
	if req.Path == "" {
		h.reply(sessionID, protocol.Response{ID: req.ID, OK: false, Error: "path is required"})
		return
	}
	if len(req.Content) > protocol.MaxWriteBytes {
		h.reply(sessionID, protocol.Response{
			ID: req.ID, OK: false,
			Error: "content exceeds maximum write size",
		})
		return
	}
	abs, err := h.WS.Resolve(req.Path)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if info, err := os.Stat(abs); err == nil && info.IsDir() {
		h.reply(sessionID, protocol.Response{ID: req.ID, OK: false, Error: "cannot write a directory"})
		return
	}
	if err := os.MkdirAll(filepath.Dir(abs), 0o755); err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	// 文件若只读（常见于复制/迁移工作区后），先尝试恢复写权限再写入
	if info, err := os.Stat(abs); err == nil && !info.IsDir() {
		if info.Mode().Perm()&0200 == 0 {
			_ = os.Chmod(abs, (info.Mode().Perm() | 0600))
		}
	}
	if err := os.WriteFile(abs, []byte(req.Content), 0o644); err != nil {
		if os.IsPermission(err) {
			h.reply(sessionID, protocol.Response{
				ID: req.ID, OK: false,
				Error: "permission denied: workspace files are read-only; check --path directory permissions",
			})
			return
		}
		h.replyErr(sessionID, req.ID, err)
		return
	}
	apiPath, _ := h.WS.Rel(abs)
	respBody, err := json.Marshal(protocol.Response{
		ID: req.ID, OK: true,
		Data: map[string]any{"path": apiPath, "size": len(req.Content)},
	})
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if ret := h.Client.SendData(sessionID, respBody, sendTimeoutMs); ret == 0 {
		log.Printf("session %d: write response SendData failed id=%s", sessionID, req.ID)
	}
	log.Printf("session %d: write id=%s path=%q bytes=%d", sessionID, req.ID, apiPath, len(req.Content))
}

func (h *Handler) handleDownloadStream(sessionID uint32, req protocol.Request) {
	abs, err := h.WS.Resolve(req.Path)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	info, err := os.Stat(abs)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if info.IsDir() {
		h.reply(sessionID, protocol.Response{ID: req.ID, OK: false, Error: "cannot download a directory"})
		return
	}
	if info.Size() > protocol.MaxVideoStreamBytes {
		h.reply(sessionID, protocol.Response{
			ID: req.ID, OK: false,
			Error: "file exceeds maximum inline media size (200 MB)",
		})
		return
	}

	h.reply(sessionID, protocol.Response{
		ID: req.ID, OK: true,
		Data: map[string]any{
			"stream":    true,
			"totalSize": info.Size(),
			"chunkSize": protocol.StreamChunkSize,
		},
	})

	go func() {
		f, err := os.Open(abs)
		if err != nil {
			h.sendStreamError(sessionID, req.ID, err.Error())
			return
		}
		defer f.Close()

		buf := make([]byte, protocol.StreamChunkSize)
		var offset int64
		for {
			n, readErr := f.Read(buf)
			if n > 0 {
				frame, encErr := protocol.EncodeStreamFrame(req.ID, protocol.StreamFlagChunk, offset, buf[:n])
				if encErr != nil {
					h.sendStreamError(sessionID, req.ID, encErr.Error())
					return
				}
				if ret := h.Client.SendData(sessionID, frame, sendTimeoutMs); ret == 0 {
					log.Printf("session %d: stream chunk SendData failed offset=%d", sessionID, offset)
					return
				}
				offset += int64(n)
			}
			if readErr == io.EOF {
				break
			}
			if readErr != nil {
				h.sendStreamError(sessionID, req.ID, readErr.Error())
				return
			}
		}
		done, encErr := protocol.EncodeStreamFrame(req.ID, protocol.StreamFlagDone, offset, nil)
		if encErr != nil {
			h.sendStreamError(sessionID, req.ID, encErr.Error())
			return
		}
		if ret := h.Client.SendData(sessionID, done, sendTimeoutMs); ret == 0 {
			log.Printf("session %d: stream done SendData failed", sessionID)
		}
		log.Printf("session %d: download_stream id=%s path=%q bytes=%d", sessionID, req.ID, req.Path, offset)
	}()
}

func (h *Handler) sendStreamError(sessionID uint32, reqID, message string) {
	frame, err := protocol.EncodeStreamFrame(reqID, protocol.StreamFlagError, 0, []byte(message))
	if err != nil {
		log.Printf("session %d: encode stream error frame: %v", sessionID, err)
		return
	}
	if ret := h.Client.SendData(sessionID, frame, sendTimeoutMs); ret == 0 {
		log.Printf("session %d: stream error SendData failed", sessionID)
	}
}

func (h *Handler) handleDownload(sessionID uint32, req protocol.Request) {
	abs, err := h.WS.Resolve(req.Path)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	info, err := os.Stat(abs)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if info.IsDir() {
		h.reply(sessionID, protocol.Response{ID: req.ID, OK: false, Error: "cannot download a directory"})
		return
	}

	// 同步发出 ack，确保对端先进入接收状态再开始 SendFile，避免桶与 fileSent 乱序丢失
	ackBody, err := json.Marshal(protocol.Response{
		ID: req.ID, OK: true, Data: map[string]string{"message": "sending file"},
	})
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if ret := h.Client.SendData(sessionID, ackBody, sendTimeoutMs); ret == 0 {
		log.Printf("session %d: download ack SendData failed id=%s", sessionID, req.ID)
		h.replyErr(sessionID, req.ID, errors.New("send download ack failed"))
		return
	}

	go func() {
		ret := h.Client.SendFile(sessionID, abs)
		if ret == 0 {
			log.Printf("session %d: SendFile %s failed, ret=%d", sessionID, abs, ret)
			doneBody, mErr := json.Marshal(protocol.Response{
				ID: req.ID, OK: false, Error: "send file failed",
				Data: map[string]any{"fileSent": false},
			})
			if mErr == nil {
				_ = h.Client.SendData(sessionID, doneBody, sendTimeoutMs)
			}
			return
		}
		doneBody, err := json.Marshal(protocol.Response{
			ID: req.ID, OK: true,
			Data: map[string]any{"fileSent": true, "size": info.Size()},
		})
		if err != nil {
			log.Printf("session %d: marshal fileSent: %v", sessionID, err)
			return
		}
		if ret := h.Client.SendData(sessionID, doneBody, sendTimeoutMs); ret == 0 {
			log.Printf("session %d: fileSent SendData failed id=%s", sessionID, req.ID)
		}
		log.Printf("session %d: download complete id=%s path=%q bytes=%d", sessionID, req.ID, req.Path, info.Size())
	}()
}

func (h *Handler) handleList(sessionID uint32, req protocol.Request) {
	log.Printf("session %d: list id=%s path=%q", sessionID, req.ID, req.Path)
	abs, err := h.WS.Resolve(req.Path)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	info, err := os.Stat(abs)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if !info.IsDir() {
		h.reply(sessionID, protocol.Response{ID: req.ID, OK: false, Error: "not a directory"})
		return
	}

	entries, err := os.ReadDir(abs)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}

	items := make([]protocol.FileEntry, 0, len(entries))
	for _, e := range entries {
		fi, err := e.Info()
		if err != nil {
			continue
		}
		childRel, err := h.WS.Rel(filepath.Join(abs, e.Name()))
		if err != nil {
			continue
		}
		items = append(items, protocol.FileEntry{
			Name:    e.Name(),
			Path:    childRel,
			IsDir:   e.IsDir(),
			Size:    fi.Size(),
			ModTime: fi.ModTime(),
		})
	}

	apiPath, _ := h.WS.Rel(abs)
	h.reply(sessionID, protocol.Response{
		ID: req.ID, OK: true,
		Data: protocol.ListData{Path: apiPath, Items: items},
	})
}

func (h *Handler) handleStat(sessionID uint32, req protocol.Request) {
	abs, err := h.WS.Resolve(req.Path)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	info, err := os.Stat(abs)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	apiPath, _ := h.WS.Rel(abs)
	h.reply(sessionID, protocol.Response{
		ID: req.ID, OK: true,
		Data: protocol.StatData{
			Path: apiPath, IsDir: info.IsDir(),
			Size: info.Size(), ModTime: info.ModTime(),
		},
	})
}

func (h *Handler) handleMkdir(sessionID uint32, req protocol.Request) {
	abs, err := h.WS.Resolve(req.Path)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if err := os.MkdirAll(abs, 0o755); err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	apiPath, _ := h.WS.Rel(abs)
	h.reply(sessionID, protocol.Response{
		ID: req.ID, OK: true, Data: map[string]string{"path": apiPath},
	})
}

func (h *Handler) handleDelete(sessionID uint32, req protocol.Request) {
	abs, err := h.WS.Resolve(req.Path)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if abs == h.WS.Root {
		h.reply(sessionID, protocol.Response{ID: req.ID, OK: false, Error: "cannot delete workspace root"})
		return
	}
	info, err := os.Stat(abs)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if info.IsDir() {
		err = os.RemoveAll(abs)
	} else {
		err = os.Remove(abs)
	}
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	h.reply(sessionID, protocol.Response{ID: req.ID, OK: true})
}

func (h *Handler) handleRename(sessionID uint32, req protocol.Request) {
	fromAbs, err := h.WS.Resolve(req.From)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	toAbs, err := h.WS.Resolve(req.To)
	if err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if err := os.MkdirAll(filepath.Dir(toAbs), 0o755); err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	if err := os.Rename(fromAbs, toAbs); err != nil {
		h.replyErr(sessionID, req.ID, err)
		return
	}
	apiPath, _ := h.WS.Rel(toAbs)
	h.reply(sessionID, protocol.Response{
		ID: req.ID, OK: true, Data: map[string]string{"path": apiPath},
	})
}

func (h *Handler) reply(sessionID uint32, resp protocol.Response) {
	body, err := json.Marshal(resp)
	if err != nil {
		log.Printf("session %d: marshal response: %v", sessionID, err)
		return
	}
	go func() {
		ret := h.Client.SendData(sessionID, body, sendTimeoutMs)
		if ret == 0 {
			log.Printf("session %d: SendData failed, ret=%d", sessionID, ret)
		}
	}()
}

func (h *Handler) replyErr(sessionID uint32, id string, err error) {
	msg := err.Error()
	if errors.Is(err, workspace.ErrOutsideWorkspace) {
		msg = "path is outside workspace"
	} else if errors.Is(err, fs.ErrNotExist) || os.IsNotExist(err) {
		msg = "not found"
	}
	h.reply(sessionID, protocol.Response{ID: id, OK: false, Error: msg})
}
