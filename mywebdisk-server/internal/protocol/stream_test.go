package protocol

import "testing"

func TestStreamFrameRoundTrip(t *testing.T) {
	payload := []byte("hello chunk")
	frame, err := EncodeStreamFrame("vs-abc", StreamFlagChunk, 1024, payload)
	if err != nil {
		t.Fatal(err)
	}
	id, flag, off, data, ok, err := DecodeStreamFrame(frame)
	if err != nil || !ok {
		t.Fatalf("decode: ok=%v err=%v", ok, err)
	}
	if id != "vs-abc" || flag != StreamFlagChunk || off != 1024 {
		t.Fatalf("meta mismatch id=%q flag=%d off=%d", id, flag, off)
	}
	if string(data) != string(payload) {
		t.Fatalf("data mismatch")
	}
}
