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

	var requestCount uint64

	countRequests := func(next http.HandlerFunc) http.HandlerFunc {
		return func(w http.ResponseWriter, r *http.Request) {
			atomic.AddUint64(&requestCount, 1)
			next(w, r)
		}
	}

	// TPS printer
	go func() {
		ticker := time.NewTicker(10 * time.Second)
		for range ticker.C {
			currentCount := atomic.SwapUint64(&requestCount, 0)
			tps := float64(currentCount) / 10.0
			fmt.Printf("[%s] TPS: %.2f\n", time.Now().Format("15:04:05"), tps)
		}
	}()

	// ---- AUTH ----
	mux.HandleFunc("/v1/auth/login", countRequests(func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, `{
            "success": true,
            "token": "tok_123",
            "user_id": "user_123",
            "session_id": "sess_123"
        }`)
	}))

	mux.HandleFunc("/v1/auth/session/", countRequests(okJSON))
	mux.HandleFunc("/v1/auth/logout", countRequests(okJSON))

	// ---- USER ----
	mux.HandleFunc("/v1/users/", countRequests(func(w http.ResponseWriter, r *http.Request) {

		path := r.URL.Path

		switch {
		case contains(path, "addresses") && r.Method == "POST":
			writeJSON(w, `{"address_id":"addr_123"}`)
		case contains(path, "addresses"):
			writeJSON(w, `{"status":"ok"}`)
		case contains(path, "preferences"):
			writeJSON(w, `{"status":"ok"}`)
		case contains(path, "recently_viewed"):
			writeJSON(w, `{"status":"ok"}`)
		default:
			writeJSON(w, `{"status":"ok"}`)
		}
	}))

	// ---- CATALOG ----
	mux.HandleFunc("/v1/catalog/", countRequests(okJSON))
	mux.HandleFunc("/v1/products/", countRequests(okJSON))
	mux.HandleFunc("/v1/search", countRequests(okJSON))
	mux.HandleFunc("/v1/recommendations", countRequests(okJSON))
	mux.HandleFunc("/v1/inventory/", countRequests(okJSON))

	// ---- CART ----
	mux.HandleFunc("/v1/cart", countRequests(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == "POST" {
			writeJSON(w, `{"cart_id":"cart_123"}`)
			return
		}
		okJSON(w, r)
	}))

	mux.HandleFunc("/v1/cart/", countRequests(func(w http.ResponseWriter, r *http.Request) {
		path := r.URL.Path

		switch {
		case contains(path, "/items") && r.Method == "POST":
			writeJSON(w, `{"item_id":"item_123"}`)
		case contains(path, "apply_coupon"):
			writeJSON(w, `{"discount_id":"disc_123"}`)
		default:
			writeJSON(w, `{"status":"ok"}`)
		}
	}))

	mux.HandleFunc("/v1/shipping/quote", countRequests(okJSON))
	mux.HandleFunc("/v1/coupons/validate", countRequests(okJSON))

	// ---- ORDER ----
	mux.HandleFunc("/v1/orders/preview", countRequests(func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, `{"order_id":"order_123"}`)
	}))

	mux.HandleFunc("/v1/orders/", countRequests(okJSON))

	// ---- PAYMENT ----
	mux.HandleFunc("/v1/payments/init", countRequests(func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, `{"payment_id":"pay_123"}`)
	}))

	mux.HandleFunc("/v1/payments/authorize/", countRequests(okJSON))

	// ---- SUPPORT ----
	mux.HandleFunc("/v1/support/tickets", countRequests(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == "POST" {
			writeJSON(w, `{"ticket_id":"ticket_123"}`)
			return
		}
		okJSON(w, r)
	}))

	mux.HandleFunc("/v1/support/tickets/", countRequests(okJSON))

	// ---- NOTIFS ----
	mux.HandleFunc("/v1/notifications", countRequests(okJSON))
	mux.HandleFunc("/v1/notifications/mark_read", countRequests(okJSON))

	// Fallback
	mux.HandleFunc("/", countRequests(okJSON))

	fmt.Println("🚀 Mock server running on :8080")
	log.Fatal(http.ListenAndServe(":8080", mux))
}

// ---------- helpers ----------

func okJSON(w http.ResponseWriter, r *http.Request) {
	_, _ = io.Copy(io.Discard, r.Body)
	w.WriteHeader(200)
	w.Write([]byte(`{"status":"ok"}`))
}

func writeJSON(w http.ResponseWriter, body string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(200)
	w.Write([]byte(body))
}

func contains(s, sub string) bool {
	return len(s) >= len(sub) && (stringIndex(s, sub) >= 0)
}

func stringIndex(s, substr string) int {
	for i := 0; i+len(substr) <= len(s); i++ {
		if s[i:i+len(substr)] == substr {
			return i
		}
	}
	return -1
}
