package main

import (
	"flag"
	"fmt"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/xiaoming-software/mywebdisk/internal/callback"
	"github.com/xiaoming-software/mywebdisk/internal/webrpc"
)

func usage() {
	fmt.Fprintf(os.Stderr, `个人 NAS 服务端（webrpc P2P）

用法:
  mywebdisk-server --token TOKEN --passwd PASSWD --permission PERMISSION --path DIR

参数:
  --token        webrpc token（必填）
  --passwd       webrpc token 密码（必填）
  --permission   webrpc 口令，对端 OpenSession 时需提供（必填）
  --path         NAS 文件目录（必填，不存在则自动创建）
`)
}

func main() {
	log.SetFlags(log.LstdFlags | log.Lmsgprefix)
	log.SetPrefix("mywebdisk-server ")

	token := flag.String("token", "", "webrpc token")
	passwd := flag.String("passwd", "", "webrpc password")
	permission := flag.String("permission", "", "webrpc permission")
	path := flag.String("path", "", "NAS root directory")
	flag.Usage = usage
	flag.Parse()

	if *token == "" || *passwd == "" || *permission == "" || *path == "" {
		usage()
		os.Exit(2)
	}

	root, err := prepareNASDir(*path)
	if err != nil {
		log.Fatalf("NAS 目录: %v", err)
	}

	client, err := webrpc.Open(*token, *passwd, *permission)
	if err != nil {
		log.Fatalf("webrpc: %v", err)
	}
	defer client.Close()

	log.Printf("webrpc 客户端已创建，等待登录...")
	if err := client.WaitLogin(2*time.Second, 30*time.Second); err != nil {
		log.Fatal(err)
	}
	log.Printf("webrpc 登录成功 status=%d", client.LoginStatus())

	tcpPort := client.ReceivePort()
	if tcpPort <= 0 {
		log.Fatalf("无效的回调端口: %d", tcpPort)
	}
	go callback.Listen(tcpPort, root, func(sessionID uint32, data []byte) int {
		return client.SendData(sessionID, data, 5000)
	}, func(sessionID uint32, filePath string) int {
		return client.SendFile(sessionID, filePath)
	})

	log.Printf("NAS 目录: %s", root)
	log.Printf("回调端口: %d", tcpPort)
	log.Printf("服务已就绪，等待客户端通过 webrpc 建立会话")

	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	ticker := time.NewTicker(5 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-sig:
			log.Println("正在退出")
			return
		case <-ticker.C:
			log.Printf("当前会话数: %d", client.SessionSize())
		}
	}
}

func prepareNASDir(path string) (string, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	info, err := os.Stat(abs)
	if err != nil {
		if !os.IsNotExist(err) {
			return "", err
		}
		if err := os.MkdirAll(abs, 0o755); err != nil {
			return "", fmt.Errorf("创建目录失败: %w", err)
		}
		return abs, nil
	}
	if !info.IsDir() {
		return "", fmt.Errorf("不是目录: %s", abs)
	}
	return abs, nil
}
