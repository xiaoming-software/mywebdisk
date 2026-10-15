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
	fileName string
	filePath string
	dest     string
	at       time.Time
}

const putFileTTL = 2 * time.Minute

var (
	fileMu    sync.Mutex
	fileRecvs = map[string]*fileRecv{}
	putMu     sync.Mutex
	puts      = map[uint32]*pendingPut{}
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
	MaxEdge     int             `json:"maxEdge"`
}

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
			if err := writeFileChunk(sessionID, string(name), conn, dataLen, root); err != nil {
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
		go handleGetFile(sessionID, env.FileName, env.FilePath, root, send, sendFile)
	case "getThumb":
		go handleGetThumb(sessionID, env.FileName, env.FilePath, env.MaxEdge, root, send)
	case "putFile":
		handlePutFile(sessionID, env.FileName, env.FilePath, env.Size, root)
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

func handlePutFile(sessionID uint32, fileName, filePath string, size *uint64, root string) {
	dest, err := nas.ResolveWriteTarget(root, filePath, fileName)
	if err != nil {
		log.Printf("putFile 非法 session=%d path=%s name=%s: %v", sessionID, filePath, fileName, err)
		return
	}
	if size != nil && *size == 0 {
		if err := os.WriteFile(dest, []byte{}, 0o644); err != nil {
			log.Printf("putFile 清空失败 session=%d dest=%s: %v", sessionID, dest, err)
			return
		}
		putMu.Lock()
		delete(puts, sessionID)
		putMu.Unlock()
		log.Printf("putFile 空文件 session=%d path=%s name=%s dest=%s", sessionID, filePath, fileName, dest)
		return
	}
	putMu.Lock()
	puts[sessionID] = &pendingPut{
		fileName: fileName,
		filePath: filePath,
		dest:     dest,
		at:       time.Now(),
	}
	putMu.Unlock()
	log.Printf("putFile session=%d path=%s name=%s dest=%s", sessionID, filePath, fileName, dest)
}

func resolveIncomingDest(sessionID uint32, rawName, root string) (string, error) {
	putMu.Lock()
	st := puts[sessionID]
	if st != nil && time.Since(st.at) > putFileTTL {
		delete(puts, sessionID)
		st = nil
	}
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
	if st := puts[sessionID]; st != nil && st.dest == dest {
		delete(puts, sessionID)
	}
	putMu.Unlock()
}

func writeFileChunk(sessionID uint32, rawName string, r io.Reader, n uint32, root string) error {
	dest, err := resolveIncomingDest(sessionID, rawName, root)
	if err != nil {
		log.Printf("文件流路径非法 session=%d name=%s: %v", sessionID, rawName, err)
		_, copyErr := io.CopyN(io.Discard, r, int64(n))
		return copyErr
	}
	key := fmt.Sprintf("%d:%s", sessionID, dest)
	fileMu.Lock()
	st := fileRecvs[key]
	if st == nil || time.Since(st.at) > 3*time.Second {
		if st != nil && st.file != nil {
			st.file.Close()
		}
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
	_ = f.Sync()

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

func handleGetFile(sessionID uint32, fileName, filePath, root string, send SendFunc, sendFile SendFileFunc) {
	local, err := nas.ResolveFile(root, filePath, fileName)
	if err != nil {
		log.Printf("getFile session=%d path=%s name=%s 失败: %v", sessionID, filePath, fileName, err)
		return
	}
	if sendFile == nil {
		log.Printf("getFile session=%d 无 SendFile 回调", sessionID)
		return
	}
	ret := sendFile(sessionID, local)
	log.Printf("SendFile session=%d path=%s name=%s file=%s ret=%d", sessionID, filePath, fileName, local, ret)
	if ret != 1 {
		return
	}
	if send == nil {
		log.Printf("getFile session=%d 无 SendData 回调", sessionID)
		return
	}
	body, err := json.Marshal(map[string]any{
		"type":     "backFile",
		"fileName": fileName,
		"filePath": filePath,
	})
	if err != nil {
		log.Printf("序列化 backFile 失败: %v", err)
		return
	}
	ack := send(sessionID, body)
	log.Printf("backFile session=%d path=%s name=%s send=%d", sessionID, filePath, fileName, ack)
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
