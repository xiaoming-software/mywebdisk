package disk

// Sender sends replies over an active webrpc session.
type Sender interface {
	SendData(sessionID uint32, data []byte, timeoutMs int64) int
	SendFile(sessionID uint32, filePath string) int
}
