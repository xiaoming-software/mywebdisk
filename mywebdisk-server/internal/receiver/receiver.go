package receiver

import (
	"encoding/binary"
	"fmt"
	"io"
	"log"
	"net"

	"github.com/mywebdisk/mywebdisk-server/internal/protocol"
)

// Handler receives webrpc callback streams (server disk API or desktop relay).
type Handler interface {
	HandleData(sessionID uint32, payload []byte)
	HandleFile(sessionID uint32, fileName, fileData []byte)
}

// Run connects to the local webrpc callback TCP port and dispatches streams.
func Run(tcpPort int, handler Handler) {
	conn, err := net.Dial("tcp", fmt.Sprintf("127.0.0.1:%d", tcpPort))
	if err != nil {
		log.Printf("connect callback server: %v", err)
		return
	}
	defer conn.Close()

	log.Printf("connected to webrpc callback port %d", tcpPort)

	for {
		sessionID, dataType, ok := readHeader(conn)
		if !ok {
			return
		}

		switch dataType {
		case protocol.StreamTypeData:
			payload, ok := readDataStream(conn)
			if !ok {
				return
			}
			handler.HandleData(sessionID, payload)
		case protocol.StreamTypeFile:
			fileName, fileData, ok := readFileStream(conn)
			if !ok {
				return
			}
			handler.HandleFile(sessionID, fileName, fileData)
		default:
			log.Printf("session %d: unknown stream type %d", sessionID, dataType)
		}
	}
}

func readHeader(conn net.Conn) (sessionID uint32, dataType byte, ok bool) {
	var hdr [5]byte
	if _, err := io.ReadFull(conn, hdr[:]); err != nil {
		if err != io.EOF {
			log.Printf("read header: %v", err)
		} else {
			log.Println("callback connection closed")
		}
		return 0, 0, false
	}
	sessionID = binary.BigEndian.Uint32(hdr[:4])
	dataType = hdr[4]
	return sessionID, dataType, true
}

func readDataStream(conn net.Conn) ([]byte, bool) {
	var lenBuf [4]byte
	if _, err := io.ReadFull(conn, lenBuf[:]); err != nil {
		log.Printf("read data length: %v", err)
		return nil, false
	}
	n := binary.BigEndian.Uint32(lenBuf[:])
	if n == 0 {
		return []byte{}, true
	}
	data := make([]byte, n)
	if _, err := io.ReadFull(conn, data); err != nil {
		log.Printf("read data body: %v", err)
		return nil, false
	}
	return data, true
}

func readFileStream(conn net.Conn) (fileName, fileData []byte, ok bool) {
	var nameLenBuf [4]byte
	if _, err := io.ReadFull(conn, nameLenBuf[:]); err != nil {
		log.Printf("read file name length: %v", err)
		return nil, nil, false
	}
	nameLen := binary.BigEndian.Uint32(nameLenBuf[:])
	fileName = make([]byte, nameLen)
	if _, err := io.ReadFull(conn, fileName); err != nil {
		log.Printf("read file name: %v", err)
		return nil, nil, false
	}

	var dataLenBuf [4]byte
	if _, err := io.ReadFull(conn, dataLenBuf[:]); err != nil {
		log.Printf("read file data length: %v", err)
		return nil, nil, false
	}
	dataLen := binary.BigEndian.Uint32(dataLenBuf[:])
	fileData = make([]byte, dataLen)
	if _, err := io.ReadFull(conn, fileData); err != nil {
		log.Printf("read file data: %v", err)
		return nil, nil, false
	}
	return fileName, fileData, true
}
