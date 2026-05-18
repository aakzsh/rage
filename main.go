package main

import (
	"fmt"
	"io"
	"log"
	"net/http"
	"sync/atomic"
	"time"
)

func main() {
	mux := http.NewServeMux()

	// Atomic counter to track total processed requests
	var requestCount uint64

	// Middleware to count every incoming request dynamically
	countRequests := func(next http.HandlerFunc) http.HandlerFunc {
		return func(w http.ResponseWriter, r *http.Request) {
			atomic.AddUint64(&requestCount, 1)
			next(w, r)
		}
	}

	// Background ticker to calculate and print TPS every 10 seconds
	go func() {
		ticker := time.NewTicker(10 * time.Second)
		for range ticker.C {
			// Swap out the current count and reset it back to 0 atomically
			currentCount := atomic.SwapUint64(&requestCount, 0)
			tps := float64(currentCount) / 10.0
			fmt.Printf("[%s] Current TPS: %.2f\n", time.Now().Format("15:04:05"), tps)
		}
	}()

	// 1. LOGIN ENDPOINT
	mux.HandleFunc("/v1/login", countRequests(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(`{"success": true, "token": "eyM4ProEngineToken2026xyz"}`))
	}))

	// 2. CONTACT ENDPOINT
	mux.HandleFunc("/v1/contact", countRequests(func(w http.ResponseWriter, r *http.Request) {
		// Drain the body completely to keep TCP connections healthy/reusable
		_, _ = io.Copy(io.Discard, r.Body)
		r.Body.Close()

		w.WriteHeader(http.StatusOK)
		w.Write([]byte(`{"status":"contact_created"}`))
	}))

	// Fallback catch-all handler for any other routing matching test.yaml
	mux.HandleFunc("/", countRequests(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte("OK"))
	}))

	fmt.Println("🚀 Go mock server running on http://127.0.0.1:8080 (Muting verbose headers, tracking TPS...)")
	if err := http.ListenAndServe(":8080", mux); err != nil {
		log.Fatalf("Server failed to start: %v", err)
	}
}
