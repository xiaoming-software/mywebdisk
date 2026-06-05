// webrpc-helper：桌面端与 libwebrpc 的 IPC 桥（TCP JSON 行协议）
package main

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net"
	"os"
	"sync"
	"time"

	"github.com/mywebdisk/mywebdisk-server/internal/receiver"
	"github.com/mywebdisk/mywebdisk-server/internal/webrpc"
)

type wireReq struct {
	ID         int    `json:"id"`
	Cmd        string `json:"cmd"`
	Token      string `json:"token,omitempty"`
	Passwd     string `json:"passwd,omitempty"`
	Permission string `json:"permission,omitempty"`
	ToToken    string `json:"to_token,omitempty"`
	SessionID  uint32 `json:"session_id,omitempty"`
	Body       string `json:"body,omitempty"`
}

type wireResp struct {
	ID        int             `json:"id,omitempty"`
	OK        bool            `json:"ok"`
	Error     string          `json:"error,omitempty"`
	Event     string          `json:"event,omitempty"`
	SessionID uint32          `json:"session_id,omitempty"`
	Port      int             `json:"port,omitempty"`
	Status    int             `json:"status,omitempty"`
	Data      json.RawMessage `json:"data,omitempty"`
	Payload   string          `json:"payload,omitempty"`
}

type stdoutRelay struct {
	enc *json.Encoder
	mu  sync.Mutex
}

func (r *stdoutRelay) emit(v wireResp) {
	r.mu.Lock()
	defer r.mu.Unlock()
	_ = r.enc.Encode(v)
}

func (r *stdoutRelay) HandleData(sessionID uint32, payload []byte) {
	r.emit(wireResp{
		Event:     "data",
		SessionID: sessionID,
		Payload:   string(payload),
	})
}

func (r *stdoutRelay) HandleFile(sessionID uint32, fileName, fileData []byte) {
	r.emit(wireResp{
		Event:     "file",
		SessionID: sessionID,
		Payload:   string(fileName),
	})
}

var (
	clientMu sync.Mutex
	client   *webrpc.Client
	relay    *stdoutRelay
)

func main() {
	log.SetOutput(os.Stderr)
	port := flag.Int("port", 0, "TCP listen port (required)")
	flag.Parse()
	if *port <= 0 {
		fmt.Fprintln(os.Stderr, "usage: webrpc-helper --port <port>")
		os.Exit(2)
	}

	ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", *port))
	if err != nil {
		log.Fatal(err)
	}
	defer ln.Close()

	conn, err := ln.Accept()
	if err != nil {
		log.Fatal(err)
	}
	defer conn.Close()

	relay = &stdoutRelay{enc: json.NewEncoder(conn)}
	_ = relay.enc.Encode(wireResp{Event: "ready", OK: true})

	sc := bufio.NewScanner(conn)
	for sc.Scan() {
		line := sc.Bytes()
		if len(line) == 0 {
			continue
		}
		var req wireReq
		if err := json.Unmarshal(line, &req); err != nil {
			writeResp(conn, wireResp{OK: false, Error: "invalid json"})
			continue
		}
		handleReq(conn, &req)
	}
}

func writeResp(_ net.Conn, v wireResp) {
	if relay != nil {
		relay.emit(v)
	}
}

func handleReq(conn net.Conn, req *wireReq) {
	switch req.Cmd {
	case "login":
		go doLogin(req)
	case "logout":
		doLogout(conn, req)
	case "open_session":
		doOpenSession(conn, req)
	case "close_session":
		doCloseSession(conn, req)
	case "send_json":
		doSendJSON(conn, req)
	case "session_size":
		doSessionSize(conn, req)
	case "ping":
		writeResp(conn, wireResp{ID: req.ID, OK: true, Event: "pong"})
	default:
		writeResp(conn, wireResp{ID: req.ID, OK: false, Error: "unknown cmd"})
	}
}

func doLogin(req *wireReq) {
	clientMu.Lock()
	if client != nil {
		client.Close()
		client = nil
	}
	clientMu.Unlock()

	// WebrpcClient_New：同步一步创建客户端
	c, err := webrpc.Open(req.Token, req.Passwd, req.Permission)
	if err != nil {
		writeResp(nil, wireResp{ID: req.ID, OK: false, Error: err.Error()})
		return
	}

	// 每秒轮询 WebrpcClient_LoginStatus，最多 10 秒
	const maxWait = 10
	var st int
	for i := 0; i < maxWait; i++ {
		st = c.LoginStatus()
		if st != 0 {
			break
		}
		time.Sleep(1 * time.Second)
	}

	if st == 0 {
		c.Close() // WebrpcClient_Free
		writeResp(nil, wireResp{ID: req.ID, OK: false, Error: "login timeout (10s)"})
		return
	}

	port := c.ReceivePort()
	go receiver.Run(port, relay)

	clientMu.Lock()
	client = c
	clientMu.Unlock()

	writeResp(nil, wireResp{
		ID: req.ID, OK: true, Status: st, Port: port,
	})
}

func doLogout(conn net.Conn, req *wireReq) {
	clientMu.Lock()
	defer clientMu.Unlock()
	if client != nil {
		client.Close()
		client = nil
	}
	writeResp(conn, wireResp{ID: req.ID, OK: true})
}

func doOpenSession(conn net.Conn, req *wireReq) {
	clientMu.Lock()
	c := client
	clientMu.Unlock()
	if c == nil {
		writeResp(conn, wireResp{ID: req.ID, OK: false, Error: "not logged in"})
		return
	}
	sid := c.OpenSession(req.ToToken, req.Permission)
	if sid == 0 {
		writeResp(conn, wireResp{ID: req.ID, OK: false, Error: "open session failed"})
		return
	}
	writeResp(conn, wireResp{ID: req.ID, OK: true, SessionID: sid})
}

func doCloseSession(conn net.Conn, req *wireReq) {
	clientMu.Lock()
	c := client
	clientMu.Unlock()
	if c == nil {
		writeResp(conn, wireResp{ID: req.ID, OK: false, Error: "not logged in"})
		return
	}
	ret := c.CloseSession(req.SessionID)
	if ret != 1 {
		writeResp(conn, wireResp{ID: req.ID, OK: false, Error: fmt.Sprintf("close session ret=%d", ret)})
		return
	}
	writeResp(conn, wireResp{ID: req.ID, OK: true})
}

func doSendJSON(conn net.Conn, req *wireReq) {
	clientMu.Lock()
	c := client
	clientMu.Unlock()
	if c == nil {
		writeResp(conn, wireResp{ID: req.ID, OK: false, Error: "not logged in"})
		return
	}
	body := []byte(req.Body)
	ret := c.SendData(req.SessionID, body, 30000)
	if ret == 0 {
		writeResp(conn, wireResp{ID: req.ID, OK: false, Error: fmt.Sprintf("send failed ret=%d", ret)})
		return
	}
	writeResp(conn, wireResp{ID: req.ID, OK: true})
}

func doSessionSize(conn net.Conn, req *wireReq) {
	clientMu.Lock()
	c := client
	clientMu.Unlock()
	if c == nil {
		writeResp(conn, wireResp{ID: req.ID, OK: false, Error: "not logged in"})
		return
	}
	n := c.SessionSize()
	data, _ := json.Marshal(map[string]int{"count": n})
	writeResp(conn, wireResp{ID: req.ID, OK: true, Data: data})
}
