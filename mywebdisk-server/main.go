package main

import (
	"flag"
	"fmt"
	"log"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/mywebdisk/mywebdisk-server/internal/disk"
	"github.com/mywebdisk/mywebdisk-server/internal/receiver"
	"github.com/mywebdisk/mywebdisk-server/internal/webrpc"
	"github.com/mywebdisk/mywebdisk-server/internal/workspace"
)

func main() {
	path := flag.String("path", "", "workspace root (required)")
	token := flag.String("token", "", "webrpc token (required)")
	passwd := flag.String("passwd", "", "webrpc password (required)")
	permission := flag.String("permission", "", "webrpc permission (required)")
	flag.Parse()

	if *path == "" || *token == "" || *passwd == "" || *permission == "" {
		fmt.Fprintln(os.Stderr, "usage: mywebdisk-server --path=/home/tem --token=B --passwd=123456 --permission=123456abcd")
		os.Exit(2)
	}

	ws, err := workspace.Open(*path)
	if err != nil {
		log.Fatalf("open workspace: %v", err)
	}

	client, err := webrpc.Open(*token, *passwd, *permission)
	if err != nil {
		log.Fatalf("webrpc: %v", err)
	}
	defer client.Close()

	log.Printf("webrpc client created, waiting for login...")
	client.WaitLogin(2 * time.Second)
	log.Printf("webrpc login ok, status=%d", client.LoginStatus())

	tcpPort := client.ReceivePort()
	log.Printf("callback TCP port: %d", tcpPort)

	handler := disk.NewHandler(ws, client)
	go receiver.Run(tcpPort, handler)

	log.Printf("workspace: %s", ws.Root)
	log.Printf("mywebdisk-server ready; remote clients use webrpc P2P to read/write files")

	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)

	ticker := time.NewTicker(5 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-sig:
			log.Println("shutting down")
			return
		case <-ticker.C:
			log.Printf("active sessions: %d", client.SessionSize())
		}
	}
}
