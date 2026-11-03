# QRIS Static2Dynamic — Android E2E Tester & Notification Forwarder

Aplikasi Android native ringan (**~878 KB**, Pure Kotlin + Android SDK tanpa dependency berat) untuk menguji kesiapan ekosistem **QrisStatic2Dynamic (`qris-api`)** secara *end-to-end* langsung dari perangkat Android fisik menggunakan **USB Debugging (`adb reverse`)** maupun jaringan Wi-Fi/VPS.

---

## ✨ Fitur Utama

1. **1-Click Provisioning (Merchant + Android Source)**:
   - Otomatis memvalidasi QRIS statis (`POST /api/v1/qris/parse`), mendaftarkan Merchant (`POST /api/v1/merchants`), dan membuat kanal `ANDROID` Notification Source (`POST /api/v1/merchants/{id}/sources`).
   - Menyimpan `merchant_id`, `api_key` (`qsk_live_...`), `signing_secret` (`qss_live_...`), `source_id` (`src_...`), dan `source_secret` (`qws_live_...`) di penyimpanan lokal aplikasi.
2. **POS Kasir + QRIS Dinamis + Real-Time SSE**:
   - Membuat transaksi dinamis (`POST /api/v1/merchants/{id}/transactions`) lengkap dengan `Idempotency-Key` dan *Canonical HMAC Request Signing*.
   - Menampilkan rincian `base_amount`, `unique_code` (`1–999`), dan `final_amount`.
   - Merender gambar QRIS Dinamis (`GET /api/v1/transactions/{id}/qr.png`) di layar HP.
   - Membuka koneksi **Server-Sent Events (SSE)** (`GET /api/v1/transactions/{id}/stream`) yang otomatis mengubah status dari `PENDING` (Kuning) menjadi `PAID` (Hijau) secara real-time saat pembayaran masuk.
3. **Android NotificationListenerService (`QrisNotificationService`)**:
   - Mendengarkan *Push Notification* masuk di status bar Android yang memuat nominal Rupiah (`Rp` / `IDR`) dari GoPay Merchant, DANA Bisnis, m-Banking, maupun simulasi internal.
   - Otomatis menghitung signature HMAC-SHA256 kanonik (`timestamp + "\n" + event_id + "\n" + sha256(raw_body)`) dan mengirimkannya ke `POST /api/v1/webhooks/mutation/{source_id}`.
4. **Simulasi Pembayaran Instan & 1-Click Automated E2E Suite**:
   - **⚡ Trigger Push Notifikasi Bayar di HP (E2E OS)**: Memunculkan notifikasi Android asli di layar atas HP untuk menguji alur penuh: *OS Notification -> NotificationListenerService -> HMAC Webhook -> Atomic Matcher -> SSE `PAID`*.
   - **🚀 Kirim Webhook Mutasi HMAC Langsung**: Menguji endpoint mutasi secara langsung tanpa lewat OS notification.
   - **🧪 Jalankan Automated Full E2E Suite (1-Klik)**: Menjalankan 6 tahapan verifikasi kesiapan sistem dari awal hingga transaksi berstatus `PAID`.

---

## 🚀 Cara Menjalankan via USB Debugging

### 1. Jalankan Server Rust di PC
Di root repositori `QrisStatic2Dynamic`:
```bash
cp .env.example .env
cargo run --bin qris-api
```

### 2. Hubungkan HP via USB & Aktifkan Port Forwarding (`adb reverse`)
Pastikan **USB Debugging** aktif di HP Anda, lalu jalankan:
```bash
adb devices
adb reverse tcp:8080 tcp:8080
```
> Dengan `adb reverse`, akses ke `http://127.0.0.1:8080` dari dalam HP Android akan langsung diteruskan ke port `8080` di komputer melalui kabel USB.

### 3. Build & Install APK ke HP
```bash
cd android-tester
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

### 4. Berikan Izin Notification Listener & Buka Aplikasi
```bash
# Berikan izin akses baca notifikasi & post notifikasi via ADB
adb shell cmd notification allow_listener id.qris.tester/id.qris.tester.QrisNotificationService
adb shell pm grant id.qris.tester android.permission.POST_NOTIFICATIONS

# Buka aplikasi di layar HP
adb shell am start -n id.qris.tester/.MainActivity
```

Atau jalankan langsung **Automated E2E Diagnostic Suite** dari terminal:
```bash
adb logcat -c
adb shell am start -S -n id.qris.tester/.MainActivity --ez auto_test true
sleep 5
adb logcat -d -s QrisTester:I
```
