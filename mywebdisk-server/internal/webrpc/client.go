package webrpc

import (
	"sync"
	"time"
)

// Client wraps the webrpc SDK (API 与官方 C 头文件一致).
type Client struct {
	h handle
}

var (
	loadOnce sync.Once
	loadErr  error
)

func Open(token, passwd, permission string) (*Client, error) {
	loadOnce.Do(func() {
		loadErr = openEmbedded()
	})
	if loadErr != nil {
		return nil, loadErr
	}
	return &Client{h: cNew(token, passwd, permission)}, nil
}

func (c *Client) Close() {
	if c.h != 0 {
		cFree(c.h)
		c.h = 0
	}
}

func (c *Client) LoginStatus() int {
	return cLoginStatus(c.h)
}

func (c *Client) WaitLogin(poll time.Duration) {
	for c.LoginStatus() == 0 {
		time.Sleep(poll)
	}
}

func (c *Client) ReceivePort() int {
	return cReceivePort(c.h)
}

func (c *Client) OpenSession(toToken, permission string) uint32 {
	return cOpenSession(c.h, toToken, permission)
}

func (c *Client) CloseSession(sessionID uint32) int {
	return cCloseSession(c.h, sessionID)
}

func (c *Client) SessionSize() int {
	return cSessionSize(c.h)
}

func (c *Client) SendData(sessionID uint32, data []byte, timeoutMs int64) int {
	return cSendData(c.h, sessionID, data, timeoutMs)
}

func (c *Client) SendFile(sessionID uint32, filePath string) int {
	return cSendFile(c.h, sessionID, filePath)
}
