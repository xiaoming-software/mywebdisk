package callback

import "testing"

func TestPutChunkOutcome(t *testing.T) {
	const chunk = fileChunkSize
	cases := []struct {
		name     string
		written  uint64
		expected uint64
		chunkLen uint32
		done     bool
		failed   bool
	}{
		{"partial full chunk", 10 << 20, 30 << 20, chunk, false, false},
		{"exact on full chunk", 20 << 20, 20 << 20, chunk, true, false},
		{"exact on short chunk", 100, 100, 100, true, false},
		{"short chunk missing bytes", 50, 100, 50, true, true},
		{"wrote past announced size", 101, 100, 101, true, true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			done, failed := putChunkOutcome(tc.written, tc.expected, tc.chunkLen)
			if done != tc.done || failed != tc.failed {
				t.Fatalf("got done=%v failed=%v", done, failed)
			}
		})
	}
}
