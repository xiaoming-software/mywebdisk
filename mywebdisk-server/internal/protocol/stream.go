package protocol

import (
	"encoding/binary"
	"errors"
)

// Binary stream frames (MWDK) are sent on StreamTypeData alongside JSON RPC responses.
const (
	StreamMagic      = "MWDK"
	StreamVersion    = 1
	StreamFlagChunk  = 0
	StreamFlagDone   = 1
	StreamFlagError  = 2
	StreamChunkSize  = 512 * 1024
	MaxVideoStreamBytes = 200 * 1024 * 1024
)

var (
	errStreamFrameShort = errors.New("stream frame too short")
	errStreamBadMagic   = errors.New("stream frame bad magic")
	errStreamBadVersion = errors.New("stream frame bad version")
)

// EncodeStreamFrame builds a MWDK v1 frame for chunked download / video stream.
func EncodeStreamFrame(reqID string, flag byte, offset int64, payload []byte) ([]byte, error) {
	id := []byte(reqID)
	if len(id) == 0 || len(id) > 65535 {
		return nil, errors.New("stream frame: invalid request id length")
	}
	if len(payload) > 0xfffffff {
		return nil, errors.New("stream frame: chunk too large")
	}
	out := make([]byte, 4+1+1+2+len(id)+8+4+len(payload))
	copy(out[0:4], StreamMagic)
	out[4] = StreamVersion
	out[5] = flag
	binary.BigEndian.PutUint16(out[6:8], uint16(len(id)))
	copy(out[8:8+len(id)], id)
	off := 8 + len(id)
	binary.BigEndian.PutUint64(out[off:off+8], uint64(offset))
	off += 8
	binary.BigEndian.PutUint32(out[off:off+4], uint32(len(payload)))
	off += 4
	copy(out[off:], payload)
	return out, nil
}

// DecodeStreamFrame parses a MWDK v1 frame. Returns ok=false if payload is not a stream frame.
func DecodeStreamFrame(payload []byte) (reqID string, flag byte, offset int64, data []byte, ok bool, err error) {
	if len(payload) < 4 || string(payload[0:4]) != StreamMagic {
		return "", 0, 0, nil, false, nil
	}
	if len(payload) < 20 {
		return "", 0, 0, nil, true, errStreamFrameShort
	}
	if payload[4] != StreamVersion {
		return "", 0, 0, nil, true, errStreamBadVersion
	}
	flag = payload[5]
	idLen := int(binary.BigEndian.Uint16(payload[6:8]))
	if len(payload) < 8+idLen+8+4 {
		return "", 0, 0, nil, true, errStreamFrameShort
	}
	reqID = string(payload[8 : 8+idLen])
	off := 8 + idLen
	offset = int64(binary.BigEndian.Uint64(payload[off : off+8]))
	off += 8
	dataLen := int(binary.BigEndian.Uint32(payload[off : off+4]))
	off += 4
	if len(payload) < off+dataLen {
		return "", 0, 0, nil, true, errStreamFrameShort
	}
	data = payload[off : off+dataLen]
	return reqID, flag, offset, data, true, nil
}
