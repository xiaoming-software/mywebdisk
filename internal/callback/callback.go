package callback

import (
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/xiaoming-software/mywebdisk/internal/nas"
)

const (
	streamTypeFile = 1
	streamTypeData = 2
	fileChunkSize  = 10 * 1024 * 1024
)

type SendFunc func(sessionID uint32, data []byte) int
type SendFileFunc func(sessionID uint32, filePath string) int

type fileRecv struct {
	file *os.File
	at   time.Time
}

type pendingPut struct {
	fileName       string
	filePath       string
	dest           string
	expected       uint64
	written        uint64
	file           *os.File
	at             time.Time
	lastReport     time.Time
	lastReportByte uint64
	timer          *time.Timer
	gen            uint64
}

// putFileTTL is an *idle* timeout: reset whenever bytes are written.
// Absolute wall-clock caps would kill multi-GB uploads that are still progressing.
const putFileTTL = 2 * time.Minute
// 进度回传过密会和文件分片抢同一会话，Windows WiFi 驱动容易掉线；按秒级节流即可。
const putProgressEvery = 2 * time.Second
const putProgressBytes = 4 * 1024 * 1024

// putChunkOutcome decides whether a putFile transfer is finished.
// A short chunk is the sender's end-of-file marker. A full chunk finishes
// only when the announced size has been reached. Extra bytes are a failure.
func putChunkOutcome(written, expected uint64, chunkLen uint32) (done bool, failed bool) {
	if expected > 0 && written > expected {
		return true, true
	}
	if expected > 0 && written == expected {
		return true, false
	}
	if chunkLen < fileChunkSize {
		return true, true
	}
	return false, false
}

var (
	fileMu    sync.Mutex
	fileRecvs = map[string]*fileRecv{}
	putMu     sync.Mutex
	puts      = map[uint32]*pendingPut{}
	putGen    uint64
)

type envelope struct {
	Type        json.RawMessage `json:"type"`
	Data        json.RawMessage `json:"data"`
	FileName    string          `json:"fileName"`
	FileNames   []string        `json:"fileNames"`
	FilePath    string          `json:"filePath"`
	TargetPath  string          `json:"targetPath"`
	NewFileName string          `json:"newFileName"`
	NewName     string          `json:"newName"`
	Size        *uint64         `json:"size"`
	// Offset is bytes already transferred (resume). Omitted / 0 = full rewrite.
	Offset  *uint64 `json:"offset"`
	// Length is byte count for readRange (capped server-side).
	Length *uint64 `json:"length"`
	// RequestId correlates unordered webrpc replies (statFile / readRange).
	RequestId string `json:"requestId"`
	MaxEdge   int    `json:"maxEdge"`
}

const maxRangeLength = 1 << 20 // 1 MiB per readRange

// Listen connects to the local webrpc callback port and handles data frames.
func Listen(tcpPort int, root string, send SendFunc, sendFile SendFileFunc) {
	addr := fmt.Sprintf("127.0.0.1:%d", tcpPort)
	backoff := time.Second
	for {
		err := drain(addr, root, send, sendFile)
		log.Printf("webrpc 回调连接结束: %v，%s 后重连", err, backoff)
		time.Sleep(backoff)
		if backoff < 15*time.Second {
			backoff *= 2
		}
	}
}

func drain(addr, root string, send SendFunc, sendFile SendFileFunc) error {
	conn, err := net.Dial("tcp", addr)
	if err != nil {
		return err
	}
	defer conn.Close()
	log.Printf("已连接 webrpc 回调端口 %s", addr)

	for {
		var hdr [5]byte
		if _, err := io.ReadFull(conn, hdr[:]); err != nil {
			if err == io.EOF {
				return fmt.Errorf("连接关闭")
			}
			return fmt.Errorf("读帧头: %w", err)
		}
		sessionID := binary.BigEndian.Uint32(hdr[:4])
		dataType := hdr[4]

		switch dataType {
		case streamTypeData:
			n, err := readUint32(conn)
			if err != nil {
				return fmt.Errorf("读数据长度: %w", err)
			}
			payload := make([]byte, n)
			if _, err := io.ReadFull(conn, payload); err != nil {
				return fmt.Errorf("读数据体: %w", err)
			}
			handleData(sessionID, payload, root, send, sendFile)
		case streamTypeFile:
			nameLen, err := readUint32(conn)
			if err != nil {
				return fmt.Errorf("读文件名长度: %w", err)
			}
			name := make([]byte, nameLen)
			if _, err := io.ReadFull(conn, name); err != nil {
				return fmt.Errorf("读文件名: %w", err)
			}
			dataLen, err := readUint32(conn)
			if err != nil {
				return fmt.Errorf("读文件长度: %w", err)
			}
			if err := writeFileChunk(sessionID, string(name), conn, dataLen, root, send); err != nil {
				return fmt.Errorf("写文件分片: %w", err)
			}
		default:
			log.Printf("未知流类型 session=%d type=%d", sessionID, dataType)
		}
	}
}

func handleData(sessionID uint32, payload []byte, root string, send SendFunc, sendFile SendFileFunc) {
	var env envelope
	if err := json.Unmarshal(payload, &env); err != nil {
		log.Printf("收到数据流 session=%d bytes=%d（非 JSON，忽略）", sessionID, len(payload))
		return
	}
	kind := parseType(env.Type)
	switch kind {
	case "getNasInfo":
		handleGetNasInfo(sessionID, root, send)
	case "getPath":
		handleGetPath(sessionID, env.Data, root, send)
	case "getFile":
		go handleGetFile(sessionID, env.FileName, env.FilePath, env.Offset, root, send, sendFile)
	case "statFile":
		go handleStatFile(sessionID, env.FileName, env.FilePath, env.RequestId, root, send)
	case "readRange":
		go handleReadRange(sessionID, env.FileName, env.FilePath, env.Offset, env.Length, env.RequestId, root, send)
	case "getThumb":
		go handleGetThumb(sessionID, env.FileName, env.FilePath, env.MaxEdge, root, send)
	case "putFile":
		handlePutFile(sessionID, env.FileName, env.FilePath, env.Size, env.Offset, root, send)
	case "createFile":
		handleCreateFile(sessionID, env.Data, root, send)
	case "moveFile":
		handleMoveFile(sessionID, env, root, send)
	case "moveFiles":
		handleMoveFiles(sessionID, env, root, send)
	case "deleteFile":
		handleDeleteFile(sessionID, env, root, send)
	case "renameFile":
		handleRenameFile(sessionID, env, root, send)
	case "searchFile":
		go handleSearchFile(sessionID, env, root, send)
	case "zipFile":
		go handleZipFile(sessionID, env, root, send)
	default:
		log.Printf("收到数据流 session=%d type=%s bytes=%d", sessionID, kind, len(payload))
	}
}

func handlePutFile(sessionID uint32, fileName, filePath string, size *uint64, offset *uint64, root string, send SendFunc) {
	dest, err := nas.ResolveWriteTarget(root, filePath, fileName)
	if err != nil {
		log.Printf("putFile 非法 session=%d path=%s name=%s: %v", sessionID, filePath, fileName, err)
		sendBackPut(send, sessionID, fileName, filePath, 0, 0, true, false, "invalid-path")
		return
	}
	abortPut(sessionID, send, "replaced")
	if size == nil {
		log.Printf("putFile 缺少 size session=%d path=%s name=%s", sessionID, filePath, fileName)
		sendBackPut(send, sessionID, fileName, filePath, 0, 0, true, false, "missing-size")
		return
	}
	var off uint64
	if offset != nil {
		off = *offset
	}
	if *size == 0 {
		if err := os.WriteFile(dest, []byte{}, 0o644); err != nil {
			log.Printf("putFile 清空失败 session=%d dest=%s: %v", sessionID, dest, err)
			sendBackPut(send, sessionID, fileName, filePath, 0, 0, true, false, "write")
			return
		}
		info, statErr := os.Stat(dest)
		if statErr != nil || info.Size() != 0 {
			_ = os.Remove(dest)
			sendBackPut(send, sessionID, fileName, filePath, 0, 0, true, false, "size-mismatch")
			return
		}
		log.Printf("putFile 空文件 session=%d path=%s name=%s dest=%s", sessionID, filePath, fileName, dest)
		sendBackPut(send, sessionID, fileName, filePath, 0, 0, true, true, "")
		return
	}
	if off > *size {
		log.Printf("putFile offset 非法 session=%d offset=%d size=%d", sessionID, off, *size)
		sendBackPut(send, sessionID, fileName, filePath, 0, *size, true, false, "bad-offset")
		return
	}
	if off == *size {
		// Already complete — acknowledge without opening a put slot.
		info, statErr := os.Stat(dest)
		if statErr == nil && uint64(info.Size()) == *size {
			sendBackPut(send, sessionID, fileName, filePath, *size, *size, true, true, "")
			return
		}
		off = 0
	}
	var f *os.File
	if off == 0 {
		f, err = os.OpenFile(dest, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o644)
	} else {
		info, statErr := os.Stat(dest)
		if statErr != nil || uint64(info.Size()) < off {
			log.Printf("putFile 续传本地不足 session=%d dest=%s have=%v want=%d", sessionID, dest, statErr, off)
			sendBackPut(send, sessionID, fileName, filePath, 0, *size, true, false, "resume-mismatch")
			return
		}
		f, err = os.OpenFile(dest, os.O_WRONLY, 0o644)
		if err == nil {
			if _, seekErr := f.Seek(int64(off), io.SeekStart); seekErr != nil {
				_ = f.Close()
				err = seekErr
			} else if uint64(info.Size()) > off {
				// Truncate any bytes beyond resume point.
				if truncErr := f.Truncate(int64(off)); truncErr != nil {
					_ = f.Close()
					err = truncErr
				}
			}
		}
	}
	if err != nil {
		log.Printf("putFile 打开失败 session=%d dest=%s offset=%d: %v", sessionID, dest, off, err)
		sendBackPut(send, sessionID, fileName, filePath, off, *size, true, false, "write")
		return
	}
	putMu.Lock()
	putGen++
	gen := putGen
	st := &pendingPut{
		fileName: fileName,
		filePath: filePath,
		dest:     dest,
		expected: *size,
		written:  off,
		file:     f,
		at:       time.Now(),
		gen:      gen,
	}
	st.timer = time.AfterFunc(putFileTTL, func() { expirePut(sessionID, gen, send) })
	puts[sessionID] = st
	putMu.Unlock()
	log.Printf("putFile session=%d path=%s name=%s dest=%s size=%d offset=%d", sessionID, filePath, fileName, dest, *size, off)
	if off > 0 {
		sendBackPut(send, sessionID, fileName, filePath, off, *size, false, false, "")
	}
}

func abortPut(sessionID uint32, send SendFunc, reason string) {
	putMu.Lock()
	st := puts[sessionID]
	if st == nil {
		putMu.Unlock()
		return
	}
	delete(puts, sessionID)
	if st.timer != nil {
		st.timer.Stop()
	}
	putMu.Unlock()
	closePutFile(st)
	// Keep partial on "replaced" so a follow-up putFile{offset} can resume.
	if reason != "replaced" {
		_ = os.Remove(st.dest)
	}
	log.Printf("putFile 中止 session=%d dest=%s reason=%s written=%d", sessionID, st.dest, reason, st.written)
	sendBackPut(send, sessionID, st.fileName, st.filePath, st.written, st.expected, true, false, reason)
}

func expirePut(sessionID uint32, gen uint64, send SendFunc) {
	putMu.Lock()
	st := puts[sessionID]
	if st == nil || st.gen != gen {
		putMu.Unlock()
		return
	}
	// Sliding idle: a write may have happened after this timer was armed.
	if remaining := putFileTTL - time.Since(st.at); remaining > 0 {
		st.timer = time.AfterFunc(remaining, func() { expirePut(sessionID, gen, send) })
		putMu.Unlock()
		return
	}
	delete(puts, sessionID)
	putMu.Unlock()
	closePutFile(st)
	// Keep partial so clients with driveResume can putFile{offset} again.
	log.Printf("putFile 空闲超时 session=%d dest=%s written=%d size=%d (kept for resume)", sessionID, st.dest, st.written, st.expected)
	sendBackPut(send, sessionID, st.fileName, st.filePath, st.written, st.expected, true, false, "timeout")
}

// touchPutIdleTimer re-arms the idle timer while held under putMu.
// expirePut also re-checks st.at so a late-firing timer cannot kill an active put.
func touchPutIdleTimer(st *pendingPut, sessionID uint32, send SendFunc) {
	if st == nil {
		return
	}
	gen := st.gen
	if st.timer != nil {
		st.timer.Stop()
	}
	st.timer = time.AfterFunc(putFileTTL, func() { expirePut(sessionID, gen, send) })
}

func closePutFile(st *pendingPut) {
	if st != nil && st.file != nil {
		_ = st.file.Close()
		st.file = nil
	}
}

func sendBackPut(send SendFunc, sessionID uint32, fileName, filePath string, written, size uint64, done, ok bool, errText string) {
	if send == nil {
		return
	}
	body, err := json.Marshal(map[string]any{
		"type":     "backPut",
		"fileName": fileName,
		"filePath": filePath,
		"written":  written,
		"size":     size,
		"done":     done,
		"ok":       ok,
		"error":    errText,
	})
	if err != nil {
		log.Printf("序列化 backPut 失败: %v", err)
		return
	}
	ret := send(sessionID, body)
	log.Printf(
		"backPut session=%d path=%s name=%s written=%d size=%d done=%v ok=%v error=%s send=%d",
		sessionID, filePath, fileName, written, size, done, ok, errText, ret,
	)
}

func resolveIncomingDest(sessionID uint32, rawName, root string) (string, error) {
	putMu.Lock()
	st := puts[sessionID]
	if st != nil {
		dest := st.dest
		putMu.Unlock()
		return dest, nil
	}
	putMu.Unlock()
	return nas.ResolveWritePath(root, rawName)
}

func clearPendingPut(sessionID uint32, dest string) {
	putMu.Lock()
	if st := puts[sessionID]; st != nil && st.file == nil && st.dest == dest {
		delete(puts, sessionID)
	}
	putMu.Unlock()
}

func writeFileChunk(sessionID uint32, rawName string, r io.Reader, n uint32, root string, send SendFunc) error {
	putMu.Lock()
	st := puts[sessionID]
	var f *os.File
	if st != nil {
		f = st.file
	}
	putMu.Unlock()
	if st != nil && f != nil {
		return writePutChunk(sessionID, f, r, n, send)
	}
	return writeLooseChunk(sessionID, rawName, r, n, root)
}

func writePutChunk(sessionID uint32, f *os.File, r io.Reader, n uint32, send SendFunc) error {
	buf := make([]byte, 256*1024)
	left := int(n)
	for left > 0 {
		want := len(buf)
		if want > left {
			want = left
		}
		nr, err := io.ReadFull(r, buf[:want])
		if nr > 0 {
			nw, werr := f.Write(buf[:nr])
			if nw > 0 {
				reportPutWrite(sessionID, uint64(nw), n, send, false)
			}
			if werr != nil || nw != nr {
				failPut(sessionID, send, "write")
				if err != nil {
					return err
				}
				return fmt.Errorf("写入不完整")
			}
		}
		if err != nil {
			failPut(sessionID, send, "write")
			return err
		}
		left -= nr
	}
	reportPutWrite(sessionID, 0, n, send, true)
	return nil
}

func reportPutWrite(sessionID uint32, add uint64, chunkLen uint32, send SendFunc, chunkDone bool) {
	putMu.Lock()
	st := puts[sessionID]
	if st == nil {
		putMu.Unlock()
		return
	}
	st.written += add
	st.at = time.Now()
	// Any successful write renews idle TTL so slow multi-GB transfers can finish.
	if add > 0 {
		touchPutIdleTimer(st, sessionID, send)
	}
	written, expected := st.written, st.expected
	name, path, dest := st.fileName, st.filePath, st.dest
	done, failed := false, false
	if chunkDone {
		done, failed = putChunkOutcome(written, expected, chunkLen)
	}
	report := done ||
		time.Since(st.lastReport) >= putProgressEvery ||
		written-st.lastReportByte >= putProgressBytes
	if report {
		st.lastReport = time.Now()
		st.lastReportByte = written
	}
	var file *os.File
	if done {
		if st.timer != nil {
			st.timer.Stop()
		}
		file = st.file
		st.file = nil
		delete(puts, sessionID)
	}
	putMu.Unlock()

	if !done {
		if report {
			sendBackPut(send, sessionID, name, path, written, expected, false, false, "")
		}
		return
	}
	if file != nil {
		// 完整性靠 Close 后 Stat 对大小即可；大文件 Sync 会长时间堵会话。
		_ = file.Close()
	}
	if !failed {
		info, err := os.Stat(dest)
		if err != nil || uint64(info.Size()) != expected || written != expected {
			failed = true
		}
	}
	if failed {
		_ = os.Remove(dest)
		log.Printf("putFile 校验失败 session=%d dest=%s written=%d size=%d", sessionID, dest, written, expected)
		sendBackPut(send, sessionID, name, path, written, expected, true, false, "size-mismatch")
		return
	}
	log.Printf("putFile 完成 session=%d dest=%s bytes=%d", sessionID, dest, written)
	sendBackPut(send, sessionID, name, path, written, expected, true, true, "")
}

func failPut(sessionID uint32, send SendFunc, reason string) {
	putMu.Lock()
	st := puts[sessionID]
	if st == nil {
		putMu.Unlock()
		return
	}
	delete(puts, sessionID)
	if st.timer != nil {
		st.timer.Stop()
	}
	putMu.Unlock()
	closePutFile(st)
	_ = os.Remove(st.dest)
	sendBackPut(send, sessionID, st.fileName, st.filePath, st.written, st.expected, true, false, reason)
}

func writeLooseChunk(sessionID uint32, rawName string, r io.Reader, n uint32, root string) error {
	dest, err := resolveIncomingDest(sessionID, rawName, root)
	if err != nil {
		log.Printf("文件流路径非法 session=%d name=%s: %v", sessionID, rawName, err)
		_, copyErr := io.CopyN(io.Discard, r, int64(n))
		return copyErr
	}
	key := fmt.Sprintf("%d:%s", sessionID, dest)
	fileMu.Lock()
	st := fileRecvs[key]
	if st == nil {
		f, openErr := os.OpenFile(dest, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o644)
		if openErr != nil {
			fileMu.Unlock()
			log.Printf("打开目标文件失败 session=%d dest=%s: %v", sessionID, dest, openErr)
			_, copyErr := io.CopyN(io.Discard, r, int64(n))
			return copyErr
		}
		st = &fileRecv{file: f, at: time.Now()}
		fileRecvs[key] = st
		log.Printf("开始写入文件 session=%d dest=%s", sessionID, dest)
	}
	f := st.file
	fileMu.Unlock()

	written, err := io.CopyN(f, r, int64(n))
	if err != nil {
		fileMu.Lock()
		if cur := fileRecvs[key]; cur != nil {
			cur.file.Close()
			delete(fileRecvs, key)
		}
		fileMu.Unlock()
		return err
	}

	fileMu.Lock()
	if cur := fileRecvs[key]; cur != nil {
		cur.at = time.Now()
		if n < fileChunkSize {
			cur.file.Close()
			delete(fileRecvs, key)
			clearPendingPut(sessionID, dest)
			log.Printf("完成写入文件 session=%d dest=%s last=%d", sessionID, dest, written)
		}
	}
	fileMu.Unlock()
	log.Printf("写入分片 session=%d dest=%s bytes=%d", sessionID, dest, written)
	return nil
}

func handleGetNasInfo(sessionID uint32, root string, send SendFunc) {
	info := nas.Collect(root)
	body, err := nas.MarshalBack(info)
	if err != nil {
		log.Printf("序列化 backNasInfo 失败: %v", err)
		return
	}
	if send == nil {
		log.Printf("getNasInfo session=%d 无 SendData 回调", sessionID)
		return
	}
	ret := send(sessionID, body)
	log.Printf(
		"backNasInfo session=%d diskSize=%d banlenSize=%d fileNum=%d send=%d",
		sessionID, info.DiskSize, info.BanlenSize, info.FileNum, ret,
	)
}

func handleGetPath(sessionID uint32, raw json.RawMessage, root string, send SendFunc) {
	reqPath := parsePathData(raw)
	entries, err := nas.ListDir(root, reqPath)
	if err != nil {
		log.Printf("getPath session=%d path=%s 失败: %v", sessionID, reqPath, err)
		entries = map[string]nas.PathItem{}
	}
	body, err := json.Marshal(map[string]any{
		"type": "backPath",
		"path": reqPath,
		"data": entries,
	})
	if err != nil {
		log.Printf("序列化 backPath 失败: %v", err)
		return
	}
	if send == nil {
		log.Printf("getPath session=%d 无 SendData 回调", sessionID)
		return
	}
	ret := send(sessionID, body)
	log.Printf("backPath session=%d path=%s files=%d send=%d", sessionID, reqPath, len(entries), ret)
}

func handleGetThumb(sessionID uint32, fileName, filePath string, maxEdge int, root string, send SendFunc) {
	if send == nil {
		log.Printf("getThumb session=%d 无 SendData 回调", sessionID)
		return
	}
	fail := func(msg string) {
		body, err := json.Marshal(map[string]any{
			"type":     "backThumb",
			"ok":       false,
			"error":    msg,
			"fileName": fileName,
			"filePath": filePath,
		})
		if err != nil {
			return
		}
		send(sessionID, body)
	}
	if !nas.CanThumb(fileName) {
		fail("unsupported")
		return
	}
	res, err := nas.MakeThumb(root, filePath, fileName, maxEdge)
	if err != nil {
		log.Printf("getThumb session=%d path=%s name=%s 失败: %v", sessionID, filePath, fileName, err)
		fail(err.Error())
		return
	}
	body, err := json.Marshal(map[string]any{
		"type":     "backThumb",
		"ok":       true,
		"fileName": fileName,
		"filePath": filePath,
		"mime":     "image/jpeg",
		"edge":     res.Edge,
		"cached":   res.Cached,
		"data":     base64.StdEncoding.EncodeToString(res.JPEG),
	})
	if err != nil {
		log.Printf("序列化 backThumb 失败: %v", err)
		return
	}
	ret := send(sessionID, body)
	log.Printf(
		"backThumb session=%d path=%s name=%s bytes=%d cached=%v send=%d",
		sessionID, filePath, fileName, len(res.JPEG), res.Cached, ret,
	)
}

func mimeFromName(name string) string {
	lower := strings.ToLower(name)
	switch {
	case strings.HasSuffix(lower, ".mp4"), strings.HasSuffix(lower, ".m4v"):
		return "video/mp4"
	case strings.HasSuffix(lower, ".webm"):
		return "video/webm"
	case strings.HasSuffix(lower, ".mkv"):
		return "video/x-matroska"
	case strings.HasSuffix(lower, ".mov"):
		return "video/quicktime"
	case strings.HasSuffix(lower, ".avi"):
		return "video/x-msvideo"
	case strings.HasSuffix(lower, ".mp3"):
		return "audio/mpeg"
	case strings.HasSuffix(lower, ".m4a"), strings.HasSuffix(lower, ".aac"):
		return "audio/mp4"
	case strings.HasSuffix(lower, ".wav"):
		return "audio/wav"
	case strings.HasSuffix(lower, ".flac"):
		return "audio/flac"
	case strings.HasSuffix(lower, ".ogg"), strings.HasSuffix(lower, ".oga"):
		return "audio/ogg"
	default:
		return "application/octet-stream"
	}
}

func handleStatFile(sessionID uint32, fileName, filePath, requestID, root string, send SendFunc) {
	if send == nil {
		return
	}
	local, err := nas.ResolveFile(root, filePath, fileName)
	ok := err == nil
	var size uint64
	mime := mimeFromName(fileName)
	if ok {
		if info, stErr := os.Stat(local); stErr == nil && !info.IsDir() {
			size = uint64(info.Size())
		} else {
			ok = false
		}
	}
	body, mErr := json.Marshal(map[string]any{
		"type":      "backStat",
		"requestId": requestID,
		"ok":        ok,
		"fileName":  fileName,
		"filePath":  filePath,
		"size":      size,
		"mime":      mime,
	})
	if mErr != nil {
		log.Printf("序列化 backStat 失败: %v", mErr)
		return
	}
	ret := send(sessionID, body)
	log.Printf(
		"backStat session=%d path=%s name=%s ok=%v size=%d req=%s send=%d",
		sessionID, filePath, fileName, ok, size, requestID, ret,
	)
}

func handleReadRange(
	sessionID uint32,
	fileName, filePath string,
	offset, length *uint64,
	requestID, root string,
	send SendFunc,
) {
	if send == nil {
		return
	}
	fail := func(reason string) {
		body, _ := json.Marshal(map[string]any{
			"type":      "backRange",
			"requestId": requestID,
			"ok":        false,
			"fileName":  fileName,
			"filePath":  filePath,
			"error":     reason,
		})
		send(sessionID, body)
	}
	local, err := nas.ResolveFile(root, filePath, fileName)
	if err != nil {
		fail("not-found")
		return
	}
	info, stErr := os.Stat(local)
	if stErr != nil || info.IsDir() {
		fail("stat")
		return
	}
	total := uint64(info.Size())
	var off uint64
	if offset != nil {
		off = *offset
	}
	if off > total {
		fail("offset")
		return
	}
	var want uint64 = maxRangeLength
	if length != nil && *length > 0 {
		want = *length
	}
	if want > maxRangeLength {
		want = maxRangeLength
	}
	remain := total - off
	if want > remain {
		want = remain
	}
	buf := make([]byte, want)
	if want > 0 {
		f, openErr := os.Open(local)
		if openErr != nil {
			fail("open")
			return
		}
		if _, seekErr := f.Seek(int64(off), io.SeekStart); seekErr != nil {
			_ = f.Close()
			fail("seek")
			return
		}
		n, readErr := io.ReadFull(f, buf)
		_ = f.Close()
		if readErr != nil && readErr != io.ErrUnexpectedEOF && readErr != io.EOF {
			fail("read")
			return
		}
		buf = buf[:n]
		want = uint64(n)
	}
	body, mErr := json.Marshal(map[string]any{
		"type":      "backRange",
		"requestId": requestID,
		"ok":        true,
		"fileName":  fileName,
		"filePath":  filePath,
		"offset":    off,
		"length":    want,
		"size":      total,
		"eof":       off+want >= total,
		"data":      base64.StdEncoding.EncodeToString(buf),
	})
	if mErr != nil {
		log.Printf("序列化 backRange 失败: %v", mErr)
		return
	}
	ret := send(sessionID, body)
	log.Printf(
		"backRange session=%d path=%s name=%s off=%d len=%d total=%d req=%s send=%d",
		sessionID, filePath, fileName, off, want, total, requestID, ret,
	)
}

func handleGetFile(sessionID uint32, fileName, filePath string, offset *uint64, root string, send SendFunc, sendFile SendFileFunc) {
	local, err := nas.ResolveFile(root, filePath, fileName)
	if err != nil {
		log.Printf("getFile session=%d path=%s name=%s 失败: %v", sessionID, filePath, fileName, err)
		return
	}
	if sendFile == nil {
		log.Printf("getFile session=%d 无 SendFile 回调", sessionID)
		return
	}
	var off uint64
	if offset != nil {
		off = *offset
	}
	sendPath := local
	var tmp string
	if off > 0 {
		info, statErr := os.Stat(local)
		if statErr != nil {
			log.Printf("getFile session=%d stat 失败: %v", sessionID, statErr)
			return
		}
		total := uint64(info.Size())
		if off > total {
			log.Printf("getFile session=%d offset=%d > size=%d", sessionID, off, total)
			return
		}
		if off == total {
			if send != nil {
				body, mErr := json.Marshal(map[string]any{
					"type":     "backFile",
					"fileName": fileName,
					"filePath": filePath,
					"offset":   off,
					"size":     total,
					"ok":       true,
				})
				if mErr == nil {
					send(sessionID, body)
				}
			}
			return
		}
		// webrpc SendFile has no offset — slice remaining bytes into a temp file.
		src, openErr := os.Open(local)
		if openErr != nil {
			log.Printf("getFile session=%d open 失败: %v", sessionID, openErr)
			return
		}
		if _, seekErr := src.Seek(int64(off), io.SeekStart); seekErr != nil {
			_ = src.Close()
			log.Printf("getFile session=%d seek 失败: %v", sessionID, seekErr)
			return
		}
		tf, tmpErr := os.CreateTemp("", "mywebdisk-get-*")
		if tmpErr != nil {
			_ = src.Close()
			log.Printf("getFile session=%d temp 失败: %v", sessionID, tmpErr)
			return
		}
		tmp = tf.Name()
		_, copyErr := io.Copy(tf, src)
		_ = src.Close()
		_ = tf.Close()
		if copyErr != nil {
			_ = os.Remove(tmp)
			log.Printf("getFile session=%d copy 失败: %v", sessionID, copyErr)
			return
		}
		sendPath = tmp
		defer os.Remove(tmp)
	}
	ret := sendFile(sessionID, sendPath)
	log.Printf("SendFile session=%d path=%s name=%s file=%s offset=%d ret=%d", sessionID, filePath, fileName, sendPath, off, ret)
	if ret != 1 {
		return
	}
	if send == nil {
		log.Printf("getFile session=%d 无 SendData 回调", sessionID)
		return
	}
	info, _ := os.Stat(local)
	var total uint64
	if info != nil {
		total = uint64(info.Size())
	}
	body, err := json.Marshal(map[string]any{
		"type":     "backFile",
		"fileName": fileName,
		"filePath": filePath,
		"offset":   off,
		"size":     total,
		"ok":       true,
	})
	if err != nil {
		log.Printf("序列化 backFile 失败: %v", err)
		return
	}
	ack := send(sessionID, body)
	log.Printf("backFile session=%d path=%s name=%s offset=%d size=%d send=%d", sessionID, filePath, fileName, off, total, ack)
}

func handleCreateFile(sessionID uint32, raw json.RawMessage, root string, send SendFunc) {
	name, dir := parseCreateData(raw)
	code := nas.Create(root, dir, name)
	ok := code == ""
	body, err := json.Marshal(map[string]any{
		"type":  "backCreateFile",
		"ok":    ok,
		"name":  name,
		"path":  dir,
		"error": code,
	})
	if err != nil {
		log.Printf("序列化 backCreateFile 失败: %v", err)
		return
	}
	if send == nil {
		log.Printf("createFile session=%d 无 SendData 回调", sessionID)
		return
	}
	ret := send(sessionID, body)
	log.Printf("backCreateFile session=%d path=%s name=%s ok=%v error=%s send=%d", sessionID, dir, name, ok, code, ret)
}

func handleMoveFile(sessionID uint32, env envelope, root string, send SendFunc) {
	name := strings.TrimSpace(env.FileName)
	fromDir := strings.TrimSpace(env.FilePath)
	toDir := strings.TrimSpace(env.TargetPath)
	if dataName, dataFrom, dataTo := parseMoveData(env.Data); true {
		if name == "" {
			name = dataName
		}
		if fromDir == "" {
			fromDir = dataFrom
		}
		if toDir == "" {
			toDir = dataTo
		}
	}
	if fromDir == "" {
		fromDir = "/"
	}
	if toDir == "" {
		toDir = "/"
	}
	code := nas.Move(root, fromDir, name, toDir)
	ok := code == ""
	body, err := json.Marshal(map[string]any{
		"type":       "backMoveFile",
		"ok":         ok,
		"name":       name,
		"path":       fromDir,
		"targetPath": toDir,
		"error":      code,
	})
	if err != nil {
		log.Printf("序列化 backMoveFile 失败: %v", err)
		return
	}
	if send == nil {
		log.Printf("moveFile session=%d 无 SendData 回调", sessionID)
		return
	}
	ret := send(sessionID, body)
	log.Printf("backMoveFile session=%d from=%s to=%s name=%s ok=%v error=%s send=%d", sessionID, fromDir, toDir, name, ok, code, ret)
}

func handleMoveFiles(sessionID uint32, env envelope, root string, send SendFunc) {
	names := parseMoveNames(env)
	fromDir := strings.TrimSpace(env.FilePath)
	toDir := strings.TrimSpace(env.TargetPath)
	if dataName, dataFrom, dataTo := parseMoveData(env.Data); true {
		if fromDir == "" {
			fromDir = dataFrom
		}
		if toDir == "" {
			toDir = dataTo
		}
		if dataName != "" {
			names = appendUniqueName(names, dataName)
		}
	}
	if fromDir == "" {
		fromDir = "/"
	}
	if toDir == "" {
		toDir = "/"
	}
	type moveResult struct {
		Name  string `json:"name"`
		Ok    bool   `json:"ok"`
		Error string `json:"error"`
	}
	results := make([]moveResult, 0, len(names))
	if len(names) == 0 {
		results = append(results, moveResult{Error: "invalid"})
	}
	allOk := len(names) > 0
	for _, name := range names {
		code := nas.Move(root, fromDir, name, toDir)
		ok := code == ""
		if !ok {
			allOk = false
		}
		results = append(results, moveResult{Name: name, Ok: ok, Error: code})
	}
	body, err := json.Marshal(map[string]any{
		"type":       "backMoveFiles",
		"ok":         allOk,
		"path":       fromDir,
		"targetPath": toDir,
		"results":    results,
	})
	if err != nil {
		log.Printf("序列化 backMoveFiles 失败: %v", err)
		return
	}
	if send == nil {
		log.Printf("moveFiles session=%d 无 SendData 回调", sessionID)
		return
	}
	ret := send(sessionID, body)
	log.Printf("backMoveFiles session=%d from=%s to=%s count=%d ok=%v send=%d", sessionID, fromDir, toDir, len(names), allOk, ret)
}

func handleDeleteFile(sessionID uint32, env envelope, root string, send SendFunc) {
	names := parseMoveNames(env)
	fromDir := strings.TrimSpace(env.FilePath)
	if dataName, dataFrom, _ := parseMoveData(env.Data); true {
		if fromDir == "" {
			fromDir = dataFrom
		}
		if dataName != "" {
			names = appendUniqueName(names, dataName)
		}
	}
	if fromDir == "" {
		fromDir = "/"
	}
	type deleteResult struct {
		Name  string `json:"name"`
		Ok    bool   `json:"ok"`
		Error string `json:"error"`
	}
	results := make([]deleteResult, 0, len(names))
	if len(names) == 0 {
		results = append(results, deleteResult{Error: "invalid"})
	}
	allOk := len(names) > 0
	for _, name := range names {
		code := nas.Delete(root, fromDir, name)
		ok := code == ""
		if !ok {
			allOk = false
		}
		results = append(results, deleteResult{Name: name, Ok: ok, Error: code})
	}
	body, err := json.Marshal(map[string]any{
		"type":    "backDeleteFile",
		"ok":      allOk,
		"path":    fromDir,
		"results": results,
	})
	if err != nil {
		log.Printf("序列化 backDeleteFile 失败: %v", err)
		return
	}
	if send == nil {
		log.Printf("deleteFile session=%d 无 SendData 回调", sessionID)
		return
	}
	ret := send(sessionID, body)
	log.Printf("backDeleteFile session=%d path=%s count=%d ok=%v send=%d", sessionID, fromDir, len(names), allOk, ret)
}

func handleRenameFile(sessionID uint32, env envelope, root string, send SendFunc) {
	oldName := strings.TrimSpace(env.FileName)
	newName := firstNonEmpty(env.NewFileName, env.NewName)
	fromDir := strings.TrimSpace(env.FilePath)
	if dataOld, dataNew, dataFrom := parseRenameData(env.Data); true {
		if oldName == "" {
			oldName = dataOld
		}
		if newName == "" {
			newName = dataNew
		}
		if fromDir == "" {
			fromDir = dataFrom
		}
	}
	if fromDir == "" {
		fromDir = "/"
	}
	code := nas.Rename(root, fromDir, oldName, newName)
	ok := code == ""
	body, err := json.Marshal(map[string]any{
		"type":        "backRenameFile",
		"ok":          ok,
		"path":        fromDir,
		"fileName":    oldName,
		"name":        oldName,
		"newFileName": newName,
		"error":       code,
	})
	if err != nil {
		log.Printf("序列化 backRenameFile 失败: %v", err)
		return
	}
	if send == nil {
		log.Printf("renameFile session=%d 无 SendData 回调", sessionID)
		return
	}
	ret := send(sessionID, body)
	log.Printf("backRenameFile session=%d path=%s from=%s to=%s ok=%v error=%s send=%d", sessionID, fromDir, oldName, newName, ok, code, ret)
}

func handleSearchFile(sessionID uint32, env envelope, root string, send SendFunc) {
	keyword := parseSearchKeyword(env)
	hits, truncated := nas.Search(root, keyword)
	body, err := json.Marshal(map[string]any{
		"type":      "backSearchFile",
		"ok":        true,
		"keyword":   keyword,
		"truncated": truncated,
		"results":   hits,
	})
	if err != nil {
		log.Printf("序列化 backSearchFile 失败: %v", err)
		return
	}
	if send == nil {
		log.Printf("searchFile session=%d 无 SendData 回调", sessionID)
		return
	}
	ret := send(sessionID, body)
	log.Printf("backSearchFile session=%d keyword=%s hits=%d truncated=%v send=%d", sessionID, keyword, len(hits), truncated, ret)
}

func handleZipFile(sessionID uint32, env envelope, root string, send SendFunc) {
	name := strings.TrimSpace(env.FileName)
	fromDir := strings.TrimSpace(env.FilePath)
	if dataName, _, dataFrom := parseRenameData(env.Data); true {
		if name == "" {
			name = dataName
		}
		if fromDir == "" {
			fromDir = dataFrom
		}
	}
	if fromDir == "" {
		fromDir = "/"
	}
	zipName := strings.TrimSpace(name) + ".zip"
	sendZip := func(ok, done bool, pct int, code string) {
		body, err := json.Marshal(map[string]any{
			"type":     "backZipFile",
			"ok":       ok,
			"done":     done,
			"progress": pct,
			"fileName": name,
			"filePath": fromDir,
			"zipName":  zipName,
			"error":    code,
		})
		if err != nil {
			log.Printf("序列化 backZipFile 失败: %v", err)
			return
		}
		if send == nil {
			log.Printf("zipFile session=%d 无 SendData 回调", sessionID)
			return
		}
		ret := send(sessionID, body)
		log.Printf("backZipFile session=%d path=%s name=%s zip=%s done=%v ok=%v progress=%d error=%s send=%d", sessionID, fromDir, name, zipName, done, ok, pct, code, ret)
	}
	sendZip(true, false, 0, "")
	code := nas.ZipFolder(root, fromDir, name, func(pct int) {
		if pct < 0 {
			pct = 0
		}
		if pct > 99 {
			pct = 99
		}
		sendZip(true, false, pct, "")
	})
	ok := code == ""
	pct := 0
	if ok {
		pct = 100
	}
	sendZip(ok, true, pct, code)
}

func parseSearchKeyword(env envelope) string {
	keyword := strings.TrimSpace(env.FileName)
	if keyword != "" {
		return keyword
	}
	if len(env.Data) == 0 {
		return ""
	}
	var s string
	if json.Unmarshal(env.Data, &s) == nil {
		return strings.TrimSpace(s)
	}
	var data struct {
		FileName string `json:"fileName"`
		Keyword  string `json:"keyword"`
		Name     string `json:"name"`
		Query    string `json:"query"`
	}
	if json.Unmarshal(env.Data, &data) != nil {
		return ""
	}
	return firstNonEmpty(data.FileName, data.Keyword, data.Name, data.Query)
}

func firstNonEmpty(values ...string) string {
	for _, v := range values {
		if strings.TrimSpace(v) != "" {
			return strings.TrimSpace(v)
		}
	}
	return ""
}

func parseRenameData(raw json.RawMessage) (oldName, newName, fromDir string) {
	if len(raw) == 0 {
		return "", "", ""
	}
	var data struct {
		FileName    string `json:"fileName"`
		Name        string `json:"name"`
		NewFileName string `json:"newFileName"`
		NewName     string `json:"newName"`
		FilePath    string `json:"filePath"`
		Path        string `json:"path"`
	}
	if json.Unmarshal(raw, &data) != nil {
		return "", "", ""
	}
	return firstNonEmpty(data.FileName, data.Name), firstNonEmpty(data.NewFileName, data.NewName), firstNonEmpty(data.FilePath, data.Path)
}

func appendUniqueName(names []string, raw string) []string {
	name := strings.TrimSpace(raw)
	if name == "" {
		return names
	}
	for _, item := range names {
		if item == name {
			return names
		}
	}
	return append(names, name)
}

func parseMoveNames(env envelope) []string {
	var names []string
	for _, n := range env.FileNames {
		names = appendUniqueName(names, n)
	}
	names = appendUniqueName(names, env.FileName)
	if len(env.Data) == 0 {
		return names
	}
	var data struct {
		FileNames []string `json:"fileNames"`
		Names     []string `json:"names"`
		Name      string   `json:"name"`
		FileName  string   `json:"fileName"`
	}
	if json.Unmarshal(env.Data, &data) != nil {
		return names
	}
	for _, n := range data.FileNames {
		names = appendUniqueName(names, n)
	}
	for _, n := range data.Names {
		names = appendUniqueName(names, n)
	}
	names = appendUniqueName(names, data.Name)
	names = appendUniqueName(names, data.FileName)
	return names
}

func parseMoveData(raw json.RawMessage) (name, fromDir, toDir string) {
	if len(raw) == 0 {
		return "", "", ""
	}
	var data struct {
		Name       string `json:"name"`
		Path       string `json:"path"`
		FilePath   string `json:"filePath"`
		TargetPath string `json:"targetPath"`
		Target     string `json:"target"`
	}
	if json.Unmarshal(raw, &data) != nil {
		return "", "", ""
	}
	name = strings.TrimSpace(data.Name)
	fromDir = strings.TrimSpace(data.Path)
	if fromDir == "" {
		fromDir = strings.TrimSpace(data.FilePath)
	}
	toDir = strings.TrimSpace(data.TargetPath)
	if toDir == "" {
		toDir = strings.TrimSpace(data.Target)
	}
	return name, fromDir, toDir
}

func parseCreateData(raw json.RawMessage) (name, dir string) {
	var data struct {
		Name string `json:"name"`
		Path string `json:"path"`
	}
	if err := json.Unmarshal(raw, &data); err != nil {
		return "", "/"
	}
	name = strings.TrimSpace(data.Name)
	dir = strings.TrimSpace(data.Path)
	if dir == "" {
		dir = "/"
	}
	return name, dir
}

func parsePathData(raw json.RawMessage) string {
	if len(raw) == 0 {
		return "/"
	}
	var s string
	if json.Unmarshal(raw, &s) == nil {
		s = strings.TrimSpace(s)
		if s == "" {
			return "/"
		}
		return s
	}
	return "/"
}

func parseType(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	var s string
	if json.Unmarshal(raw, &s) == nil {
		return s
	}
	var n json.Number
	if json.Unmarshal(raw, &n) == nil {
		return n.String()
	}
	return string(raw)
}

func readUint32(r io.Reader) (uint32, error) {
	var b [4]byte
	if _, err := io.ReadFull(r, b[:]); err != nil {
		return 0, err
	}
	return binary.BigEndian.Uint32(b[:]), nil
}
