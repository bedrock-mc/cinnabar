// Cinnabar's small static website server. Forwardme owns TLS and domain routing.
package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

func handler(root, release string) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-cache")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		if r.Method != http.MethodGet && r.Method != http.MethodHead {
			w.Header().Set("Allow", "GET, HEAD")
			http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
			return
		}
		if r.URL.Path == "/healthz" {
			w.Header().Set("Content-Type", "text/plain; charset=utf-8")
			if r.Method != http.MethodHead {
				fmt.Fprintln(w, release)
			}
			return
		}
		name := r.URL.Path
		if name == "/" {
			name = "/index.html"
		}
		for _, component := range strings.Split(name, "/") {
			if strings.HasPrefix(component, ".") {
				http.NotFound(w, r)
				return
			}
		}
		base, err := filepath.EvalSymlinks(root)
		if err != nil {
			http.NotFound(w, r)
			return
		}
		target, err := filepath.EvalSymlinks(filepath.Join(base, filepath.FromSlash(name)))
		if err != nil || !strings.HasPrefix(target, base+string(os.PathSeparator)) {
			http.NotFound(w, r)
			return
		}
		file, err := os.Open(target)
		if err != nil {
			http.NotFound(w, r)
			return
		}
		defer file.Close()
		info, err := file.Stat()
		if err != nil || !info.Mode().IsRegular() {
			http.NotFound(w, r)
			return
		}
		http.ServeContent(w, r, info.Name(), info.ModTime(), file)
	})
}

func main() {
	root := os.Getenv("CINNABAR_ROOT")
	listen := os.Getenv("CINNABAR_LISTEN")
	release := os.Getenv("CINNABAR_RELEASE")
	if root == "" || listen == "" || release == "" {
		log.Fatal("CINNABAR_ROOT, CINNABAR_LISTEN and CINNABAR_RELEASE are required")
	}
	server := &http.Server{
		Addr: listen, Handler: handler(root, release),
		ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 15 * time.Second,
		WriteTimeout: 30 * time.Second, IdleTimeout: 60 * time.Second,
		MaxHeaderBytes: 32 * 1024,
	}
	signals, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	shutdown := make(chan struct{})
	go func() {
		defer close(shutdown)
		<-signals.Done()
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		if err := server.Shutdown(ctx); err != nil {
			log.Print(err)
		}
	}()
	log.Printf("Serving release %s on %s", release, listen)
	if err := server.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Fatal(err)
	}
	<-shutdown
}
