# QrisStatic2Dynamic (qris-api)

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85%2B%20(Edition%202024)-orange.svg)](https://www.rust-lang.org/)
[![Framework](https://img.shields.io/badge/Axum-0.8-blueviolet.svg)](https://github.com/tokio-rs/axum)
[![Database](https://img.shields.io/badge/SQLite-WAL%20Mode-lightgrey.svg)](https://www.sqlite.org/)

**QrisStatic2Dynamic** is a high-performance, merchant-safe, production-grade REST API built in **Rust (Axum 0.8)** that converts static QRIS payloads into dynamic QRIS with precise transaction amount injection, unique verification codes, and multi-channel inbound payment auto-detection (Android notifications, emails, aggregator webhooks).

---

## ⚠️ Important Disclaimer & Assumption of Risk

> **USE AT YOUR OWN RISK (RISIKO DITANGGUNG PENGGUNA)**
> 
> THIS SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE, AND NONINFRINGEMENT.
>
> 1. **Financial & Regulatory Compliance**: QRIS (Quick Response Code Indonesian Standard) is regulated by Bank Indonesia (BI) and ASPI. Deploying automated settlement, payment detection, or QR mutation services must comply with all applicable local financial laws and payment gateway regulations.
> 2. **Merchant Safety & Liability**: While this system adheres strictly to the EMVCo TLV specification and preserves merchant identifiers byte-by-byte, the authors and contributors assume **NO LIABILITY** for financial loss, chargebacks, reversed transactions, account suspensions, bank fines, missed settlements, or incorrect payment matchings.
> 3. **Notification Channel Reliability**: Payment detection relies on asynchronous bank/wallet notifications (Android accessibility notifications, email parsing, webhooks). Notification delivery delays or bank layout changes can affect auto-matching. Always maintain manual settlement and reconciliation workflows.

---

## 💖 Support & Donations

If this project saves you time, helps your business, or empowers your payment infrastructure, please consider supporting the development!

<p align="center">
  <img src="assets/donation-qr.svg" alt="Support via QRIS" width="280"/>
  <br/>
  <b>Scan with any Indonesian e-wallet / mobile banking app (QRIS)</b><br/>
  <i>Merchant: Dio Project, Komputer (Kab. Lumajang)</i>
</p>

Your donation directly supports maintaining this open-source engine, tracking upstream bank notification updates, and improving automated payment tooling.

---

## 🚀 Key Features

- **Merchant-Safe Dynamic Transformation**:
  - Strictly mutates **only** Tag `01` (Point of Initiation), Tag `54` (Transaction Amount), and Tag `63` (CRC16/CCITT-FALSE).
  - All other tags (`26–51` merchant accounts, `52` MCC, `53` Currency `360`, `58` Country `ID`, `59` Merchant Name, `60` City, `61` Postal Code, `62` Additional Data) remain **100% byte-identical and preserve exact original ordering**.
  - Automatically extracts the canonical NMID from Tag `51` (`ID.CO.QRIS`).
- **Complete EMVCo TLV Engine**: Zero-copy TLV parser, recursive sub-TLV parser, and standard CCITT-FALSE CRC16 checksum computation.
- **Transaction & Unique Code Engine**:
  - Automatically appends unique code (`1–999`) to allow simultaneous transaction matching with identical base amounts without manual interaction.
  - Partial SQLite indexes prevent concurrent amount collisions.
- **Multi-Channel Payment Auto-Detection**:
  - **Android Notifications**: Parses push notifications from GoPay Merchant, DANA Bisnis, BCA Merchant, etc.
  - **Email Mutations**: Ingests automated settlement emails.
  - **Aggregator Webhooks**: Validates HMAC signatures from upstream payment providers.
- **Real-Time Client Streaming (SSE)**:
  - `GET /api/v1/transactions/{id}/stream` delivers real-time Server-Sent Events with keep-alive pings and automatic termination on final status (`PAID`, `EXPIRED`, `CANCELED`).
- **Outbound Webhook Delivery**:
  - Built-in asynchronous worker delivering signed HMAC (`v1=<hex>`) callbacks to merchant endpoints with 6-stage exponential backoff.
- **Production-Grade Security Hardening**:
  - **Anti-Replay**: Timestamp skew check and cryptographic single-use nonces stored in SQLite.
  - **AES-256-GCM Encryption**: All webhook secrets and merchant keys are encrypted at rest with record-bound AAD.
  - **SSRF Protection**: Strict IP filtering rejecting loopback, link-local, private networks, and HTTP redirects on callbacks.
  - **DoS & Image Bomb Protection**: Enforces magic bytes, dimensions max 2048×2048, and rejects animated WebP multi-frame attacks.
  - **Operational CLI (`qris-cli`)**: Online non-blocking zero-downtime backups via `VACUUM INTO` and automatic `PRAGMA integrity_check`.

---

## 🏗️ Architecture

```
                                      ┌────────────────────────────────────────────────────────┐
                                      │                     Client / Admin                     │
                                      └──────────────────────────┬─────────────────────────────┘
                                                                 │
                                                       HTTPS / REST / SSE
                                                                 │
┌────────────────────────────────────────────────────────────────▼────────────────────────────────────────────────────────────────┐
│                                                      QrisStatic2Dynamic (Axum)                                                  │
│                                                                                                                                 │
│  ┌────────────────────────┐   ┌─────────────────────────┐   ┌──────────────────────────┐   ┌─────────────────────────────────┐  │
│  │    Rate Limiter        │   │    Security Headers     │   │   HMAC Request Signing   │   │       Request ID Tracing        │  │
│  │ (Token Bucket per IP)  │   │  (nosniff, DENY, CORS)  │   │     (Anti-Replay Nonce)  │   │       (JSON Structured Log)     │  │
│  └────────────────────────┘   └─────────────────────────┘   └──────────────────────────┘   └─────────────────────────────────┘  │
│                                                                                                                                 │
│  ┌───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐  │
│  │                                                      Domain & Services                                                    │  │
│  │  • EMVCo TLV Parser & CRC-16 Engine                                                                                       │  │
│  │  • Image Decoder (rqrr + image) in tokio::task::spawn_blocking                                                            │  │
│  │  • Transaction Management (Unique Code Pool 1–999, Expiry Sweeper)                                                        │  │
│  │  • Notification Mutation Parser & Atomic Matcher                                                                          │  │
│  │  • Outbound Webhook Delivery Worker (HMAC v1, 6x Exponential Backoff, SSRF Guard)                                         │  │
│  └───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘  │
│                                                                │                                                                │
└────────────────────────────────────────────────────────────────┼────────────────────────────────────────────────────────────────┘
                                                                 │
                                                   SQLite (WAL Mode, PRAGMA FK)
                                                                 │
                                      ┌──────────────────────────▼─────────────────────────────┐
                                      │  merchants | keys | transactions | webhook_deliveries   │
                                      │  audit_logs | request_nonces | webhook_events          │
                                      └────────────────────────────────────────────────────────┘
```

---

## 🛠️ Getting Started

### 1. Prerequisites
- **Rust Toolchain**: Stable ≥ 1.85 (Edition 2024 recommended).
- **SQLite**: 3.45+ (bundled via SQLx).

### 2. Environment Setup
Copy the sample environment file:
```bash
cp .env.example .env
```

Generate secure secrets:
```bash
# Generate 48-character Master API Key
openssl rand -base64 48

# Generate 32-byte AES-256 Data Encryption Key (DEK)
openssl rand -base64 32
```

Update your `.env`:
```env
APP_ENV=development
APP_BIND=127.0.0.1:8080
DATABASE_URL=sqlite://./data/qris.db
MASTER_API_KEY=your_generated_master_key
DATA_ENCRYPTION_KEY=your_generated_base64_32_byte_key
REQUIRE_SIGNED_REQUESTS=false # Set to true in production
ALLOW_INSECURE_CALLBACKS=false
```

### 3. Build & Run
```bash
# Run automated database migrations & start HTTP daemon
cargo run --bin qris-api

# Or build optimized release binary
cargo build --release
./target/release/qris-api
```

---

## 📖 API Documentation Overview

Base path: `/api/v1`

### Authentication
- Pass API credentials via `Authorization: Bearer <key>`.
- Master key has full administrative access.
- Merchant API keys (`qsk_live_...`) are scoped exclusively to their own merchant resources.

### Key Endpoints

| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/healthz` | Public | Liveness probe |
| `GET` | `/readyz` | Public | Readiness probe (verifies database connectivity) |
| `POST` | `/api/v1/qris/parse` | Bearer | Decodes QR from image (multipart) or parses raw TLV payload |
| `POST` | `/api/v1/qris/dynamic` | Bearer | Stateless conversion of static QRIS to dynamic QRIS |
| `POST` | `/api/v1/merchants` | Master | Register new merchant with static QRIS payload |
| `GET` | `/api/v1/merchants` | Master | List registered merchants |
| `POST` | `/api/v1/merchants/{id}/transactions` | Merchant/Master | Create dynamic transaction with unique code |
| `GET` | `/api/v1/transactions/{id}` | Merchant/Master | Get full transaction details |
| `GET` | `/api/v1/transactions/{id}/status` | Merchant/Master | Fast polling endpoint for transaction status |
| `GET` | `/api/v1/transactions/{id}/stream` | Merchant/Master | Server-Sent Events (SSE) real-time status update |
| `GET` | `/api/v1/transactions/{id}/qr.png` | Merchant/Master | Render dynamic QR as raw PNG |
| `GET` | `/api/v1/transactions/{id}/qr.svg` | Merchant/Master | Render dynamic QR as vector SVG |
| `POST` | `/api/v1/transactions/{id}/cancel` | Merchant/Master | Cancel pending transaction |
| `POST` | `/api/v1/transactions/{id}/mark-paid` | Master | Admin manual mark as paid (requires 3–200 char reason) |
| `POST` | `/api/v1/webhooks/mutation/{source_id}` | HMAC Source | Ingest raw bank/wallet mutation notification |
| `POST` | `/api/v1/merchants/{id}/test-webhook` | Master | Dispatch `merchant.test` callback to verify delivery |

For full request/response schemas, DDL specifications, and HMAC signature algorithms, refer to:
- [RULES.md](./RULES.md) — Normative specifications & strict invariants.
- [ERP.md](./ERP.md) — DDL, JSON contracts, state machine, and HMAC canonical format.
- [PRD.md](./PRD.md) — Requirements & functional definitions.
- [AGENTS.md](./AGENTS.md) — Architecture guide for contributors & autonomous coding agents.

---

## 🧰 Operational CLI (`qris-cli`)

A dedicated operational utility is included for zero-downtime maintenance:

```bash
# Run pending database migrations
qris-cli migrate

# Create online consistent backup using VACUUM INTO + PRAGMA integrity_check
qris-cli backup --out /backup/qris-20261101.db

# Verify database file integrity
qris-cli verify

# Trigger on-demand maintenance (sweeps expired transactions, cleans nonces & 90-day retention)
qris-cli cleanup
```

---

## 🧪 Testing

The repository features comprehensive unit, integration, property, and golden vector test suites:

```bash
# Run all standard tests
cargo test --all

# Run strict linter gate
cargo clippy --all-targets --all-features -- -D warnings

# Verify formatting
cargo fmt --all -- --check
```

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
Feel free to use, modify, and distribute it in your personal or commercial projects.
