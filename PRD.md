# PRD.md — Product Requirements Document

> Version: 1.0 · Status: Final · Date: 2026-11-01  
> Related documents: [RULES](./RULES.md) · [ERP](./ERP.md) · [README](./README.md) · [AGENTS](./AGENTS.md)

---

## ⚠️ Disclaimer of Liability

> **USE AT YOUR OWN RISK (RISIKO DITANGGUNG PENGGUNA)**  
> This software is an independent open-source technical implementation of EMVCo TLV manipulation and notification matching. It is NOT affiliated with, endorsed by, or operated by Bank Indonesia, ASPI, or any financial institution. Users bear all risks and legal liabilities regarding payment integration, financial discrepancies, regulatory compliance, tax obligations, and bank account actions.

---

## 1. Executive Summary

Indonesian merchants frequently utilize **static QRIS** printouts provided by acquiring banks or digital wallets (e.g. GoPay, DANA, BCA, OVO). While inexpensive and easy to deploy, static QR codes require customers to manually input transaction amounts, resulting in frequent underpayments, overpayments, fraud, and cumbersome manual reconciliation.

Official **dynamic QRIS** APIs from banks or payment gateways often involve lengthy application processes, restrictive settlement terms, and high transaction MDR fees.

**QrisStatic2Dynamic** bridges this gap:
- Transforms static QRIS into merchant-safe dynamic QRIS on the fly.
- Encodes the exact payable amount along with a small unique verification code (1–999).
- Ingests inbound settlement notifications across 3 channels (Android notifications, emails, aggregator webhooks).
- Automatically marks matched transactions as `PAID` in real-time and dispatches signed webhooks to merchant applications.

---

## 2. Product Objectives

1. **Merchant-Safe Transformation**:
   - Guarantee that dynamic QRIS codes route payments directly into the merchant's original bank account without acquirer rejection.
2. **Deterministic Settlement Matching**:
   - Achieve 100% exact amount matching without guesswork by utilizing ephemeral unique codes (`1–999`).
3. **Multi-Channel Notification Ingestion**:
   - Normalize notifications from disparate sources (Android push notifications, email, custom webhooks) into a standardized transaction state machine.
4. **Developer-First Infrastructure**:
   - Deliver sub-10ms response times for core API endpoints, native Server-Sent Events (SSE) for checkout UI updates, and zero-downtime database maintenance.

---

## 3. Scope & Feature Requirements

### 3.1 QRIS Engine
- **Stateless Parse & Decode**:
  - Accept raw QRIS string or image uploads (PNG, JPEG, static WebP).
  - Extract Merchant Name, City, Postal Code, MCC, and canonical NMID (from Tag `51`).
  - Calculate CRC-16/CCITT-FALSE checksums.
- **Dynamic Mutation**:
  - Invalidate static Point of Initiation (`Tag 01 = 11 -> 12`).
  - Inject whole integer rupiah amount (`Tag 54`).
  - Recalculate checksum (`Tag 63`) while keeping all other tags byte-identical.
  - Render dynamic QR as raw PNG, vector SVG, or base64.

### 3.2 Merchant & Transaction Management
- **Merchant Profiling**:
  - Store encrypted static QRIS payloads and configure default fee rules (`NONE`, `FIXED`, `PERCENT`).
  - Assign API keys (`qsk_live_...`) and signing secrets (`qss_live_...`).
- **Dynamic Transactions**:
  - Issue short-lived transaction records with customizable TTL (default 15 minutes).
  - Assign unique code `1–999` to guarantee active amount exclusivity per merchant.
  - Automatically release expired or canceled unique codes back to the available pool.

### 3.3 Mutation Matching & Inbound Webhooks
- **Kanal Android Notification**: Ingest push notifications forwarded by dedicated merchant mobile agents.
- **Kanal Email**: Ingest automated payment credit alert emails.
- **Kanal Aggregator**: Ingest upstream merchant aggregator webhook payloads.
- **Atomic Matcher**:
  - Atomically matches incoming credit amounts against `PENDING` transactions within `expires_at + grace_period`.
  - Dispatches signed outbound callbacks to the merchant's configured `callback_url`.

### 3.4 Operational & Admin Tooling
- **Real-Time Stream**: SSE stream (`/api/v1/transactions/{id}/stream`) for real-time frontend status updates.
- **Admin Mark-Paid**: Manual administrative resolution for out-of-band settlements with mandatory audit reason.
- **CLI Utility (`qris-cli`)**: Database migrations, online consistent SQLite backups (`VACUUM INTO`), integrity verification, and periodic retention purges.

---

## 4. Non-Functional Requirements (NFR)

- **Performance**: p99 latency < 25 ms for text transformations; p99 < 250 ms for image decoding (2 MP).
- **Security**: AES-256-GCM encryption at rest, HMAC-SHA256 request signing with anti-replay nonces, public HTTPS SSRF protection.
- **Reliability**: SQLite WAL mode with immediate transactions to avoid race conditions during unique code allocations.
- **Observability**: Structured JSON logging, request correlation headers (`X-Request-Id`), and comprehensive audit trail (`audit_logs`).
