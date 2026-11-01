# Product Requirement Document (PRD) — QrisStatic2Dynamic

> Versi: 1.0 · Status: Final · Tanggal: 2026-11-01
> Jenis produk: REST API headless (tanpa UI) · Stack: Rust + Axum + SQLite
> Asal fitur: diekstrak dari `JuraganKos` (`src/js/qris.js`)
> Dokumen terkait: [README](./README.md) · [ERP](./ERP.md) · [RULES](./RULES.md) · [AGENTS](./AGENTS.md)

---

## 1. Ringkasan Produk

**QrisStatic2Dynamic** adalah microservice REST API berkecepatan tinggi untuk mengubah
**QRIS statis** milik merchant menjadi **QRIS dinamis** yang sudah bertanam nominal,
tanpa payment gateway, tanpa login ke akun bank, dan tanpa menyentuh identitas merchant
di dalam payload.

API ini mendukung dua mode:

1. **Stateless** — client mengirim payload/gambar QRIS + nominal, API langsung
   mengembalikan payload dinamis dan gambar QR. Tidak ada data merchant yang disimpan.
2. **Stateful** — merchant didaftarkan sekali (payload atau gambar QRIS disimpan),
   lalu setiap transaksi cukup mengirim nominal. API mengelola **kode unik 1–999**,
   status transaksi, dan kecocokan pembayaran otomatis.

Pembayaran dapat terdeteksi otomatis lewat tiga kanal yang semuanya bermuara pada
satu webhook penerima mutasi:

- **Android Notification Forwarder** (notifikasi aplikasi bank/e-wallet di HP merchant)
- **Email Notification Parser** (email mutasi bank yang diteruskan ke API)
- **Aggregator Mutasi** (Moota / CekMutasi / webhook kustom)

Begitu nominal cocok dengan transaksi `PENDING`, status berubah menjadi `PAID`,
API memancarkan event **SSE** ke client yang menunggu, dan mengirim **outbound webhook**
bertanda tangan HMAC ke sistem pemanggil.

> **Posisi produk:** alat bantu operasional, **bukan** payment gateway, bukan penyimpan
> dana, dan bukan penyelenggara jasa pembayaran. Dana tetap masuk langsung ke rekening
> QRIS merchant.

---

## 2. Latar Belakang

Di `JuraganKos`, fitur QRIS statis → dinamis sudah terbukti berjalan sepenuhnya offline:

- membaca payload dari gambar QR (`jsQR`),
- memvalidasi struktur EMVCo TLV dan checksum CRC16,
- mengubah tag `01` dari `11` menjadi `12`,
- menyisipkan nominal di tag `54`,
- menghitung ulang CRC16 di tag `63`,
- merender QR baru (`qrcode`).

Namun implementasi tersebut terikat pada aplikasi Tauri dan hanya bisa dipakai di dalam
aplikasi. Banyak sistem lain (kasir, bot WhatsApp, web invoice, aplikasi kos lain,
e-commerce kecil) membutuhkan fungsi yang sama sebagai layanan terpusat yang:

- bisa dipanggil dari bahasa/framework apa pun,
- tidak membocorkan payload QRIS merchant ke semua client,
- memiliki riwayat transaksi dan status pembayaran,
- bisa menyesuaikan nominal secara aman dan idempoten,
- memiliki kontrol keamanan yang layak untuk produksi.

Proyek ini menjawab kebutuhan tersebut sebagai **API mandiri**.

---

## 3. Masalah yang Diselesaikan

| # | Masalah | Dampak |
|---|---|---|
| 1 | QRIS statis memaksa pembeli mengetik nominal sendiri | Salah nominal, kurang bayar, rekonsiliasi manual |
| 2 | Integrasi payment gateway memotong biaya dan butuh onboarding | Tidak cocok untuk UMKM/kos kecil |
| 3 | Payload QRIS merchant tersebar di banyak client | Risiko penyalahgunaan dan duplikasi konfigurasi |
| 4 | Tidak ada cara aman memverifikasi pembayaran tanpa gateway | Merchant harus cek mutasi manual satu per satu |
| 5 | Banyak sistem tidak punya library EMVCo/CRC16 | Developer harus menulis ulang parser di tiap bahasa |
| 6 | Modifikasi payload sembarangan bisa merusak akun merchant | Pembayaran ditolak acquirer, akun ditandai anomali |

---

## 4. Tujuan, Non-Goal, dan Anti-Goal

### 4.1 Tujuan

1. Menyediakan konversi QRIS statis → dinamis yang **patuh EMVCo** dan **merchant-safe**.
2. Menyediakan mode stateless yang cepat, tanpa penyimpanan data merchant.
3. Menyediakan mode stateful dengan profil merchant, kode unik, dan riwayat transaksi.
4. Mendeteksi pembayaran otomatis dari tiga kanal tanpa mengakses akun bank secara ilegal.
5. Menjadi layanan yang aman untuk produksi: API key berlapis, signed request,
   rate limit, validasi upload, audit log, dan redaksi log.
6. Menjadi fondasi integrasi bagi `JuraganKos` dan aplikasi lain.

### 4.2 Non-Goal (v1.0)

- Bukan payment gateway; tidak membuat transaksi ke Tripay/Midtrans/Xendit.
- Tidak menyimpan dana, tidak melakukan settlement, refund, atau disbursement.
- Tidak menyediakan dashboard web atau aplikasi mobile.
- Tidak mendukung multi-currency (hanya IDR).
- Tidak mengubah/menyisipkan tag `55`, `56`, `57` (fee EMV) atau tag `62` (reference).
- Tidak melakukan scraping/login ke aplikasi bank.
- Tidak menjamin pembayaran terdeteksi jika notifikasi tidak pernah dikirim ke API.

### 4.3 Anti-Goal (kegagalan walau fitur "jalan")

- Payload yang dihasilkan merusak identitas merchant atau menyebabkan pembayaran ditolak.
- Dua transaksi aktif memiliki nominal akhir yang sama sehingga pembayaran salah cocok.
- API key, signing secret, atau isi notifikasi bocor ke log.
- Transaksi ditandai `PAID` hanya karena nominal dasar cocok, tanpa kode unik.
- Layanan bisa dipakai untuk SSRF, image bomb, atau spam webhook.

---

## 5. Pengguna dan Persona

| Persona | Kebutuhan utama | Cara memakai API |
|---|---|---|
| **Developer integrasi** | Konversi cepat, dokumentasi jelas, error terstruktur | Mode stateless `parse` + `dynamic` |
| **Pemilik kos / UMKM** | QR bernominal otomatis, tanpa potongan gateway | Merchant terdaftar + transaksi + kode unik |
| **Operator kasir / admin** | Tahu transaksi mana yang sudah dibayar | Status transaksi + SSE + callback |
| **Developer bot/WA** | Membuat QR dari chat, cek status | REST + webhook |
| **Tim keamanan/ops** | Kontrol akses, audit, rotasi kunci | Master key, key rotation, audit log |

---

## 6. Prinsip Produk

1. **Merchant-Safe Mode (wajib).** Hanya tag `01`, `54`, dan `63` yang boleh berubah.
   Seluruh tag identitas merchant dan tag `62` dipertahankan byte-per-byte.
2. **Stateless-first.** Fungsi inti tidak boleh bergantung pada database. Database hanya
   untuk profil merchant, transaksi, dan kecocokan pembayaran.
3. **Uang selalu integer rupiah.** Tidak ada `f64` untuk nominal.
4. **Exact-match saja.** Auto-detect hanya cocok jika nominal akhir (termasuk kode unik)
   sama persis. Tidak ada fallback ke nominal dasar.
5. **Secure by default.** Semua endpoint (kecuali health) butuh kredensial; signature
   aktif secara default di produksi; upload dibatasi; error tidak membocorkan internal.
6. **Idempoten.** Permintaan ulang dengan `Idempotency-Key` yang sama tidak membuat
   transaksi ganda.
7. **Bisa diaudit.** Setiap aksi admin dan perubahan status penting tercatat.

---

## 7. User Journey

### 7.1 Developer — konversi stateless

1. Developer mendapat payload QRIS statis dari merchant.
2. `POST /api/v1/qris/parse` untuk memastikan payload valid dan melihat nama merchant.
3. `POST /api/v1/qris/dynamic` dengan `amount` untuk mendapatkan payload + PNG.
4. Developer merender QR di aplikasinya; tidak ada data yang tersimpan di API.

### 7.2 Merchant — onboarding stateful

1. Admin membuat merchant dengan master key, mengirim payload atau gambar QRIS.
2. API memvalidasi, menyimpan profil, dan mengembalikan **API key + signing secret sekali**.
3. Merchant/klien membuat transaksi dengan nominal tagihan.
4. API mengalokasikan kode unik, membuat QRIS dinamis, dan mengembalikan QR + `expires_at`.
5. Merchant menampilkan QR ke pembeli.

### 7.3 Pembayaran — auto-detect

1. Pembeli scan dan membayar nominal akhir (mis. `Rp 500.142`).
2. Notifikasi masuk ke HP merchant / email / aggregator.
3. Forwarder mengirim event bertanda tangan ke `POST /api/v1/webhooks/mutation/{source_id}`.
4. Parser mengekstrak nominal, matcher mencocokkan transaksi `PENDING` yang belum kedaluwarsa.
5. Status menjadi `PAID`; SSE mengirim event; outbound webhook memanggil sistem merchant.

### 7.4 Rekonsiliasi manual

1. Jika notifikasi tidak terbaca, event disimpan sebagai `UNMATCHED` (tidak hilang).
2. Admin dapat menandai transaksi lunas lewat `mark-paid` dengan alasan.
3. Aksi tercatat di audit log.

### 7.5 Operasi keamanan

1. Admin merotasi API key/secret merchant tanpa downtime.
2. Key lama langsung `REVOKED`.
3. Admin dapat menangguhkan merchant (`SUSPENDED`) yang mencurigakan.

---

## 8. Functional Requirements

Kode requirement dipakai sebagai acuan di `RULES.md`, test, dan review.

### Epic 1 — Core QRIS Engine

| ID | Requirement | Acceptance criteria |
|---|---|---|
| `FR-CORE-01` | Parse payload EMVCo TLV | Payload valid menghasilkan daftar tag, panjang, nilai, dan hash; payload rusak ditolak dengan kode error spesifik |
| `FR-CORE-02` | Validasi CRC16-CCITT (0x1021) | Vektor `123456789` → `29B1`; CRC salah → `QRIS_INVALID_CRC` |
| `FR-CORE-03` | Decode QR dari gambar | PNG/JPEG/WebP ≤ 2 MB dan ≤ 2048×2048 px dapat dibaca; gambar tanpa QR → `QRIS_QR_NOT_FOUND` |
| `FR-CORE-04` | Konversi statis → dinamis | Tag `01` `11`→`12` (default), tag `54` disisipkan tepat setelah `53`, CRC dihitung ulang |
| `FR-CORE-05` | Render QR | Output PNG biner, SVG string, atau base64; QR dapat di-decode kembali dan payload identik |
| `FR-CORE-06` | Merchant-safe invariant | Semua tag selain `01`, `54`, `63` identik dan urutannya tidak berubah; input dengan tag `55/56/57` ditolak |
| `FR-CORE-07` | Mode stateless | `parse` dan `dynamic` bekerja tanpa menulis ke database |

### Epic 2 — Merchant Configuration

| ID | Requirement | Acceptance criteria |
|---|---|---|
| `FR-MCH-01` | Registrasi merchant | Menerima payload string atau upload gambar; menyimpan payload tervalidasi + profil hasil ekstraksi |
| `FR-MCH-02` | Kredensial sekali tampil | Respons registrasi memuat API key + signing secret; setelah itu hanya hash/prefix yang tersimpan |
| `FR-MCH-03` | Manajemen profil | Master dapat membaca, mengubah, menangguhkan, dan soft-delete merchant |
| `FR-MCH-04` | Rotasi key | Key baru dibuat, key lama `REVOKED`; key baru hanya tampil sekali |
| `FR-MCH-05` | Sumber notifikasi | Merchant dapat mendaftarkan source Android/Email/Aggregator/Custom dan mendapat secret source sekali |
| `FR-MCH-06` | Duplikasi dicegah | NMID atau hash payload yang sama tidak bisa didaftarkan dua kali (409) |

### Epic 3 — Transactions

| ID | Requirement | Acceptance criteria |
|---|---|---|
| `FR-TXN-01` | Buat transaksi dinamis | Nominal dasar, fee opsional, kode unik, TTL, `reference_label`; QR langsung tersedia |
| `FR-TXN-02` | Mode fee | `NONE`, `FIXED`, `PERCENT` (basis point); fee digabung ke tag `54`, bukan tag `55–57` |
| `FR-TXN-03` | Kode unik | Range `1–999`; unik di antara transaksi `PENDING` merchant; habis → 409 |
| `FR-TXN-04` | TTL & expiry | Transaksi `PENDING` otomatis `EXPIRED` setelah `expires_at` |
| `FR-TXN-05` | Status & cancel | `GET` status, `GET` detail, cancel hanya untuk `PENDING` |
| `FR-TXN-06` | Idempotency | `Idempotency-Key` sama + body sama → transaksi yang sama; body beda → 409 |
| `FR-TXN-07` | Mark paid manual | Master dapat menandai lunas dengan alasan; audit log wajib |
| `FR-TXN-08` | Akses QR | PNG/SVG dapat diambil via endpoint ber-autentikasi; tidak ada enumerasi tanpa kredensial |

### Epic 4 — Auto-Detect Pembayaran

| ID | Requirement | Acceptance criteria |
|---|---|---|
| `FR-AUTO-01` | Android webhook | Menerima paket, judul, teks notifikasi, waktu; source secret diverifikasi |
| `FR-AUTO-02` | Email webhook | Menerima from/subject/text/message id; parser mengekstrak nominal |
| `FR-AUTO-03` | Aggregator webhook | Menerima nominal terstruktur + referensi + arah `CR` |
| `FR-AUTO-04` | Parser nominal Indonesia | Mendukung `Rp 500.142`, `Rp500.142,00`, `IDR 500,142.00`; menolak arah debit dan teks ambigu |
| `FR-AUTO-05` | Exact matcher | Hanya cocok dengan `final_amount` transaksi `PENDING` yang belum expired; tidak ada fuzzy match |
| `FR-AUTO-06` | Unmatched queue | Event tak cocok disimpan `UNMATCHED` beserta alasan; tidak dibuang |
| `FR-AUTO-07` | SSE | `GET /transactions/{id}/stream` mengirim status final + heartbeat |
| `FR-AUTO-08` | Outbound webhook | Event `transaction.paid` dikirim dengan HMAC, retry eksponensial, dan deduplikasi |
| `FR-AUTO-09` | Idempoten | `(source_id, event_id)` unik; event duplikat → `DUPLICATE` |

### Epic 5 — Keamanan & Operasi

| ID | Requirement | Acceptance criteria |
|---|---|---|
| `FR-SEC-01` | Master key | Minimal 32 karakter; dibandingkan constant-time; bisa rotasi dengan key sebelumnya |
| `FR-SEC-02` | Merchant key | Format `qsk_live_...`; disimpan sebagai SHA-256; prefix untuk lookup; bisa revoke/expire |
| `FR-SEC-03` | Signed request | HMAC-SHA256 atas timestamp + nonce + method + path + hash body; skew ≤ 300 detik; nonce anti-replay |
| `FR-SEC-04` | Rate limit | Per IP dan per kredensial; 429 + `Retry-After`; limit gambar lebih ketat |
| `FR-SEC-05` | Upload safety | Cek magic bytes, batas byte, batas piksel, decode di `spawn_blocking`, tolak animasi |
| `FR-SEC-06` | Audit log | Aksi admin, mark-paid, rotasi key, dan perubahan status tercatat |
| `FR-SEC-07` | Health | `/healthz` liveness, `/readyz` cek database |
| `FR-SEC-08` | Log aman | Tidak ada API key, secret, signature, payload QRIS, atau teks notifikasi mentah di log |
| `FR-SEC-09` | Error aman | Error produksi tidak memuat stack trace, SQL, path internal, atau isi secret |
| `FR-SEC-10` | CORS & header | Origin allowlist; `nosniff`, `DENY`, `no-store` pada data transaksi |

---

## 9. API Surface (Ringkasan)

Detail kontrak ada di [ERP.md](./ERP.md); contoh `curl` ada di [README.md](./README.md).

| Method | Path | Akses | Fungsi |
|---|---|---|---|
| `GET` | `/healthz` | publik | Liveness |
| `GET` | `/readyz` | publik | Readiness + DB |
| `POST` | `/api/v1/qris/parse` | merchant/master | Parse payload atau gambar |
| `POST` | `/api/v1/qris/dynamic` | merchant/master | Konversi stateless |
| `POST` | `/api/v1/merchants` | master | Daftarkan merchant |
| `GET` | `/api/v1/merchants` | master | List merchant |
| `GET` | `/api/v1/merchants/{id}` | master/owner | Detail merchant |
| `PATCH` | `/api/v1/merchants/{id}` | master | Ubah profil/status |
| `DELETE` | `/api/v1/merchants/{id}` | master | Soft delete + revoke |
| `POST` | `/api/v1/merchants/{id}/keys` | master | Rotasi key + secret |
| `POST` | `/api/v1/merchants/{id}/sources` | master | Daftarkan sumber notifikasi |
| `GET` | `/api/v1/merchants/{id}/sources` | master | List source |
| `DELETE` | `/api/v1/sources/{id}` | master | Revoke source |
| `POST` | `/api/v1/merchants/{id}/transactions` | merchant/master | Buat transaksi |
| `GET` | `/api/v1/transactions` | master/merchant | List transaksi (scoped) |
| `GET` | `/api/v1/transactions/{id}` | merchant/master | Detail transaksi |
| `GET` | `/api/v1/transactions/{id}/status` | merchant/master | Status ringkas |
| `GET` | `/api/v1/transactions/{id}/qr.png` | merchant/master | QR PNG |
| `GET` | `/api/v1/transactions/{id}/qr.svg` | merchant/master | QR SVG |
| `GET` | `/api/v1/transactions/{id}/stream` | merchant/master | SSE status |
| `POST` | `/api/v1/transactions/{id}/cancel` | merchant/master | Cancel |
| `POST` | `/api/v1/transactions/{id}/mark-paid` | master | Mark paid manual |
| `POST` | `/api/v1/webhooks/mutation/{source_id}` | HMAC source | Terima mutasi |
| `POST` | `/api/v1/merchants/{id}/test-webhook` | master | Tes callback |

Catatan: path parameter memakai sintaks **Axum 0.8** (`{id}`), bukan `:id`.

---

## 10. Non-Functional Requirements

| Aspek | Target v1.0 |
|---|---|
| **Latensi teks** | p50 ≤ 2 ms, p99 ≤ 25 ms untuk payload < 512 byte (tanpa render) |
| **Latensi gambar** | p99 ≤ 250 ms untuk gambar 2 MP; termasuk decode |
| **Throughput** | ≥ 500 req/s sustained pada 2 vCPU untuk operasi teks; SQLite write ≥ 100 txn/s |
| **Memori** | ≤ 100 MB RSS idle; tidak ada alokasi tak terbatas dari input |
| **Ukuran binary** | ≤ 30 MB (release, stripped, musl) |
| **Ketersediaan** | 99.5% single node; health check + restart otomatis |
| **Durabilitas** | SQLite WAL, `foreign_keys=ON`, backup harian `VACUUM INTO` |
| **Keamanan** | TLS 1.2+, secret tidak plaintext, signature + nonce, rate limit, upload limit |
| **Observability** | JSON log terstruktur, request id, audit log; metrics opsional v1.1 |
| **Kompatibilitas** | Rust stable ≥ 1.85, Axum 0.8, SQLite ≥ 3.45, Docker amd64/arm64 |
| **Zona waktu** | DB epoch ms UTC; API RFC3339 UTC |
| **Batas data** | Payload QRIS ≤ 4096 byte; JSON ≤ 256 KB; gambar ≤ 2 MB; 2048×2048 px |

---

## 11. Asumsi & Constraint

1. Merchant memiliki QRIS statis yang valid dari acquirer/PTEN.
2. Acquirer mengizinkan QRIS statis dipakai dengan nominal (tag `54`) dan flag `12`.
   Ini praktik umum, tetapi kebijakan tiap acquirer dapat berubah.
3. Notifikasi pembayaran tersedia di HP/email/aggregator merchant.
4. Server berjalan di belakang TLS reverse proxy.
5. Satu instance API per database SQLite (v1.0). Multi-instance membutuhkan Postgres
   dan store nonce terpusat (roadmap).
6. Waktu server disinkronkan NTP; skew signature maksimal 300 detik.
7. Merchant bertanggung jawab atas kepatuhan pajak, PJP, dan regulasi QRIS.

---

## 12. Risiko & Mitigasi

| Risiko | Dampak | Mitigasi |
|---|---|---|
| Acquirer menolak payload yang diubah | Pembayaran gagal | Merchant-safe mode; hanya ubah `01/54/63`; uji pilot per acquirer; opsi `point_of_initiation=preserve` |
| Notifikasi bank berubah format | Auto-detect gagal | Parser berbasis template + regex; simpan `UNMATCHED`; mark-paid manual; update template tanpa deploy? (v1.1) |
| Nominal sama antar transaksi | Salah cocok | Kode unik + partial unique index `(merchant_id, final_amount) WHERE PENDING` |
| Notifikasi palsu / replay | Transaksi ditandai lunas | HMAC source, timestamp, event id unik, rate limit, audit |
| Callback URL ke jaringan internal | SSRF | Wajib HTTPS, tolak IP privat/loopback/link-local, disable redirect |
| SQLite lock | 5xx saat ramai | WAL, transaksi pendek, `busy_timeout`, single node; roadmap Postgres |
| Secret bocor di log | Kompromi penuh | Redaksi, `secrecy`, audit review, hash/encrypt at rest |
| Image bomb | OOM | Batas byte/piksel, `image::Limits`, `spawn_blocking`, timeout |
| Merchant hilang notifikasi | Pembayaran tidak terdeteksi | Polling status, unmatched queue, manual mark-paid |
| Regulasi berubah | Produk tidak patuh | Disclaimer, konsultasi PJP/acquirer, fitur tidak menyentuh dana |

---

## 13. Metrik Keberhasilan

### Teknis

- 100% golden vector di `RULES.md` §15 lulus di CI.
- 0 pelanggaran invariant merchant-safe pada property test.
- p99 konversi teks ≤ 25 ms; p99 decode gambar ≤ 250 ms.
- 0 secret/payload di log (diverifikasi test redaksi).
- Auto-match ≥ 99.9% untuk template notifikasi yang didukung; sisanya `UNMATCHED`.
- 0 transaksi `PAID` ganda untuk `(source_id, event_id)` yang sama.

### Produk

- Minimal 3 merchant pilot berhasil onboarding.
- Minimal 1.000 transaksi/bulan di pilot.
- Waktu pembuatan QR < 1 detik dari sisi client.
- Penurunan rekonsiliasi manual ≥ 80% pada merchant pilot.

---

## 14. Rencana Rilis

| Milestone | Isi | Definisi selesai |
|---|---|---|
| **M0 — Docs** | PRD, ERP, RULES, AGENTS, README | Dokumen disetujui, konsisten |
| **M1 — Core + Stateless** | Parser TLV, CRC, convert, decode, render, parse/dynamic | Golden vector + integration test lulus |
| **M2 — Stateful** | Merchant, key, source, transaksi, fee, unique code, TTL | CRUD + idempotency + expiry test lulus |
| **M3 — Auto-detect** | 3 kanal webhook, parser, matcher, SSE, outbound callback | End-to-end test notifikasi → PAID lulus |
| **M4 — Hardening** | Auth ketat, signature, rate limit, audit, Docker, backup | Security checklist + load test dasar lulus |
| **v1.1** | Dashboard, unmatched queue UI, Postgres, metrics | Di luar scope v1.0 |

---

## 15. Di Luar Scope v1.0

- Payment gateway dan settlement.
- Dashboard web / mobile app.
- Multi-currency dan QRIS lintas negara.
- Tag `55/56/57` dan modifikasi tag `62`.
- Recurring payment, refund, partial payment.
- Multi-outlet per merchant (satu merchant = satu NMID/payload).
- Redis, Postgres, multi-region, horizontal scaling.
- Notifikasi WhatsApp keluar (hanya webhook/SSE).
- Enkripsi database penuh (SQLCipher).

---

## 16. Glosarium

| Istilah | Arti |
|---|---|
| **QRIS** | Quick Response Code Indonesian Standard, QR pembayaran berbasis EMVCo |
| **EMVCo TLV** | Format Tag-Length-Value untuk data QR pembayaran |
| **CRC16-CCITT** | Checksum poly `0x1021`, init `0xFFFF` pada tag `63` |
| **NMID** | National Merchant ID, identitas merchant QRIS |
| **Acquirer/PTEN** | Penerbit QRIS dan penyedia infrastruktur transaksi |
| **Merchant-Safe Mode** | Aturan hanya mengubah tag `01/54/63`, sisanya byte-identik |
| **Kode unik** | Angka 1–999 yang ditambahkan ke nominal untuk membedakan transaksi |
| **Stateless** | Tidak menyimpan data merchant/transaksi di API |
| **Stateful** | Menyimpan profil merchant, transaksi, dan status |
| **Source** | Kanal pengirim notifikasi mutasi (Android/Email/Aggregator/Custom) |
| **Matcher** | Komponen yang mencocokkan nominal mutasi dengan transaksi `PENDING` |
| **SSE** | Server-Sent Events untuk update status real-time |
| **Outbound webhook** | Callback bertanda tangan ke sistem merchant saat status berubah |
