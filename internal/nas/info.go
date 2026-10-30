package nas

import "encoding/json"

type Info struct {
	DiskSize   uint64 `json:"diskSize"`
	BanlenSize uint64 `json:"banlenSize"`
	FileNum    int64  `json:"fileNum"`
}

func Collect(root string) Info {
	total, free, err := DiskUsage(root)
	if err != nil {
		total, free = 0, 0
	}
	return Info{
		DiskSize:   total,
		BanlenSize: free,
		FileNum:    CountFiles(root),
	}
}

type BackNasInfo struct {
	Type       string   `json:"type"`
	Data       Info     `json:"data"`
	DriveProto int      `json:"driveProto"`
	Caps       []string `json:"caps"`
}

func MarshalBack(info Info) ([]byte, error) {
	return json.Marshal(BackNasInfo{
		Type:       "backNasInfo",
		Data:       info,
		DriveProto: 2,
		Caps:       []string{"driveResume", "driveRange"},
	})
}
