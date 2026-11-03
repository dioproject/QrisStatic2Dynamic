# QRIS Static2Dynamic — Android E2E Tester & Notification Forwarder

An ultra-lightweight (**~878 KB**), zero-bloat native Android application (Pure Kotlin + Android Platform SDK with zero heavy third-party frameworks) designed for end-to-end validation of the **QrisStatic2Dynamic (`qris-api`)** ecosystem directly on a physical Android device via **USB Debugging (`adb reverse`)** or over Wi-Fi / LAN / VPS.

---

## ✨ Key Features

1. **1-Click Provisioning (Merchant + Android Source)**:
   - Automatically validates static QRIS payloads (`POST /api/v1/qris/parse`), registers a Merchant (`POST /api/v1/merchants`), and provisions an `ANDROID` Notification Source (`POST /api/v1/merchants/{id}/sources`).
   - Persists `merchant_id`, `api_key` (`qsk_live_...`), `signing_secret` (`qss_live_...`), `source_id` (`src_...`), and `source_secret` (`qws_live_...`) in local `SharedPreferences`.
2. **POS Cashier + Dynamic QRIS Rendering + Real-Time SSE**:
   - Creates dynamic transactions (`POST /api/v1/merchants/{id}/transactions`) with `Idempotency-Key` and *Canonical HMAC Request Signing*.
   - Displays the exact breakdown of `base_amount`, `unique_code` (`1–999`), and `final_amount`.
   - Fetches and renders the dynamic QRIS PNG image (`GET /api/v1/transactions/{id}/qr.png`) directly on the phone screen.
   - Subscribes to **Server-Sent Events (SSE)** (`GET /api/v1/transactions/{id}/stream`) to transition the status badge from `PENDING` (Yellow) to `PAID` (Green) in real time upon payment detection.
3. **Android NotificationListenerService (`QrisNotificationService`)**:
   - Listens for incoming Android status bar push notifications containing Indonesian Rupiah amounts (`Rp` / `IDR`) from merchant/banking apps (GoPay Merchant, DANA Bisnis, BCA, BRI, Mandiri, OVO) as well as built-in simulated notifications.
   - Computes the canonical inbound webhook HMAC-SHA256 signature (`timestamp + "\n" + event_id + "\n" + sha256(raw_body)`) and dispatches it to `POST /api/v1/webhooks/mutation/{source_id}`.
4. **Instant Payment Simulation & 1-Click Automated E2E Suite**:
   - **⚡ Trigger OS Payment Notification (E2E OS)**: Posts a real Android system notification to the phone's status bar to test the complete pipeline: *OS Notification -> NotificationListenerService -> Signed HMAC Webhook -> Atomic Matcher -> Real-Time SSE `PAID`*.
   - **🚀 Send Direct HMAC Mutation Webhook**: Dispatches a signed mutation webhook directly to the server to test amount matching without going through the OS notification bar.
   - **🧪 Run Automated Full E2E Suite (1-Click)**: Executes a 6-stage automated readiness verification from health probes to `PAID` settlement.

---

## 🚀 Quick Start via USB Debugging

### 1. Start the Rust Server on Your PC
From the root of the `QrisStatic2Dynamic` repository:
```bash
cp .env.example .env
cargo run --bin qris-api
```

### 2. Connect Your Phone via USB & Enable Port Forwarding (`adb reverse`)
Ensure **USB Debugging** is enabled in your phone's Developer Options, then run:
```bash
adb devices
adb reverse tcp:8080 tcp:8080
```
> With `adb reverse`, any request to `http://127.0.0.1:8080` inside the Android phone is tunnelled directly to port `8080` on your PC over the USB cable—no shared Wi-Fi or firewall changes required.

### 3. Build & Install the Debug APK
```bash
cd android-tester
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

### 4. Grant Notification Listener Permissions & Launch the App
```bash
# Grant Notification Listener & Post Notification permissions via ADB
adb shell cmd notification allow_listener id.qris.tester/id.qris.tester.QrisNotificationService
adb shell pm grant id.qris.tester android.permission.POST_NOTIFICATIONS

# Launch the app on the phone screen
adb shell am start -n id.qris.tester/.MainActivity
```

Or run the **Automated 6-Stage E2E Diagnostic Suite** directly from your terminal and inspect the results via `logcat`:
```bash
adb logcat -c
adb shell am start -S -n id.qris.tester/.MainActivity --ez auto_test true
sleep 5
adb logcat -d -s QrisTester:I
```
