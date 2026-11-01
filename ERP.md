# ERP.md — Technical Architecture, DDL & API Contracts

> Version: 1.0 · Status: Final · Date: 2026-11-01  
> Related documents: [RULES](./RULES.md) · [PRD](./PRD.md) · [README](./README.md) · [AGENTS](./AGENTS.md)

---

## ⚠️ Risk & Disclaimer Notice

> **USE AT YOUR OWN RISK (RISIKO DITANGGUNG PENGGUNA)**  
> This specification defines database schemas, cryptographic mechanisms, and protocol contracts. Ensure safe environment variable management, reliable database backups, and strict compliance with local financial messaging standards when operating this system.

---

## 1. Database Schema (SQLite WAL)

All tables use `TEXT` ULID-prefixed identifiers (`mch_`, `txn_`, `src_`, `dlv_`, `aud_`, `key_`) and integer timestamps (epoch milliseconds).

```sql
-- 1. MERCHANTS
CREATE TABLE merchants (
    id                    TEXT PRIMARY KEY,
    name                  TEXT NOT NULL,
    static_payload        TEXT NOT NULL,
    static_payload_sha256 TEXT NOT NULL,
    merchant_name         TEXT NOT NULL,
    merchant_city         TEXT NOT NULL,
    postal_code           TEXT,
    mcc                   TEXT NOT NULL,
    nmid                  TEXT,
    default_fee_mode      TEXT NOT NULL DEFAULT 'NONE' CHECK (default_fee_mode IN ('NONE','FIXED','PERCENT')),
    default_fee_fixed     INTEGER NOT NULL DEFAULT 0 CHECK (default_fee_fixed >= 0),
    default_fee_percent_bps INTEGER NOT NULL DEFAULT 0 CHECK (default_fee_percent_bps >= 0),
    default_unique_code   INTEGER NOT NULL DEFAULT 1 CHECK (default_unique_code IN (0,1)),
    default_ttl_seconds   INTEGER NOT NULL DEFAULT 900 CHECK (default_ttl_seconds BETWEEN 60 AND 86400),
    callback_url          TEXT,
    status                TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','SUSPENDED','DELETED')),
    created_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL
);

-- 2. TRANSACTIONS
CREATE TABLE transactions (
    id                    TEXT PRIMARY KEY,
    merchant_id           TEXT NOT NULL REFERENCES merchants(id) ON DELETE RESTRICT,
    merchant_ref          TEXT,
    reference_label       TEXT,
    idempotency_key       TEXT,
    base_amount           INTEGER NOT NULL CHECK (base_amount > 0),
    fee_mode              TEXT NOT NULL CHECK (fee_mode IN ('NONE','FIXED','PERCENT')),
    fee_fixed             INTEGER NOT NULL DEFAULT 0,
    fee_percent_bps       INTEGER NOT NULL DEFAULT 0,
    fee_amount            INTEGER NOT NULL DEFAULT 0,
    unique_code           INTEGER NOT NULL DEFAULT 0 CHECK (unique_code BETWEEN 0 AND 999),
    final_amount          INTEGER NOT NULL CHECK (final_amount > 0),
    currency              TEXT NOT NULL DEFAULT 'IDR',
    dynamic_payload       TEXT NOT NULL,
    dynamic_payload_sha256 TEXT NOT NULL,
    crc                   TEXT NOT NULL,
    status                TEXT NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','PAID','EXPIRED','CANCELED','AMBIGUOUS')),
    payment_source        TEXT CHECK (payment_source IN ('ANDROID','EMAIL','AGGREGATOR','MANUAL')),
    paid_event_id         TEXT,
    expires_at            INTEGER NOT NULL,
    paid_at               INTEGER,
    canceled_at           INTEGER,
    cancel_reason         TEXT,
    created_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL
);

-- Partial Indexes for unique code collision prevention
CREATE UNIQUE INDEX ux_txn_active_amount
    ON transactions(merchant_id, final_amount)
    WHERE status = 'PENDING';

CREATE UNIQUE INDEX ux_txn_active_code
    ON transactions(merchant_id, unique_code)
    WHERE status = 'PENDING' AND unique_code > 0;

-- 3. AUDIT LOGS
CREATE TABLE audit_logs (
    id            TEXT PRIMARY KEY,
    merchant_id   TEXT REFERENCES merchants(id) ON DELETE SET NULL,
    actor_type    TEXT NOT NULL CHECK (actor_type IN ('MASTER','MERCHANT','SYSTEM')),
    actor_id      TEXT NOT NULL,
    action        TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id   TEXT NOT NULL,
    changes_json  TEXT CHECK (changes_json IS NULL OR json_valid(changes_json)),
    created_at    INTEGER NOT NULL
);

-- 4. OUTBOUND WEBHOOK DELIVERIES
CREATE TABLE webhook_deliveries (
    id               TEXT PRIMARY KEY,
    merchant_id      TEXT NOT NULL REFERENCES merchants(id) ON DELETE RESTRICT,
    transaction_id   TEXT REFERENCES transactions(id) ON DELETE SET NULL,
    event_type       TEXT NOT NULL CHECK (event_type IN ('transaction.paid','transaction.expired','transaction.canceled','merchant.test')),
    url              TEXT NOT NULL,
    payload_json     TEXT NOT NULL CHECK (json_valid(payload_json)),
    status           TEXT NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','DELIVERED','FAILED','DEAD')),
    attempts         INTEGER NOT NULL DEFAULT 0,
    next_attempt_at  INTEGER NOT NULL,
    created_at       INTEGER NOT NULL
);

-- 5. ANTI-REPLAY NONCES
CREATE TABLE request_nonces (
    credential_id TEXT NOT NULL,
    nonce         TEXT NOT NULL,
    seen_at       INTEGER NOT NULL,
    PRIMARY KEY (credential_id, nonce)
);
```

---

## 2. HMAC Request Signing Protocol

When `REQUIRE_SIGNED_REQUESTS=true`, clients must include the following headers on non-GET calls:

```http
X-Qris-Timestamp: 1767225600
X-Qris-Nonce: 01J8Z9V4N6T3...
X-Qris-Signature: v1=3f8a4e...
```

### Canonical String Algorithm
```text
canonical = timestamp + "\n"
          + nonce + "\n"
          + UPPERCASE_HTTP_METHOD + "\n"
          + request_path_and_query + "\n"
          + hex_lowercase(sha256(raw_body_bytes))
```

- Signature calculation: `HMAC-SHA256(secret, canonical)`.
- Master requests use `MASTER_API_KEY` as the HMAC key.
- Merchant requests use their decrypted `signing_secret` (`qss_live_...`).

---

## 3. Outbound Webhook Delivery Contract

Delivered to the merchant's HTTPS callback URL upon payment confirmation:

```http
POST /merchant/webhook/callback HTTP/1.1
Host: merchant.example.com
Content-Type: application/json
X-Qris-Delivery-Id: dlv_01J8Z...
X-Qris-Signature: v1=7a8b9c...

{
  "event": "transaction.paid",
  "timestamp": "2026-11-01T10:15:30Z",
  "data": {
    "id": "txn_01J8Z...",
    "merchant_id": "mch_01J8Z...",
    "merchant_ref": "INV-10023",
    "amount": 50142,
    "status": "PAID",
    "payment_source": "ANDROID",
    "paid_at": "2026-11-01T10:15:28Z"
  }
}
```

### Exponential Backoff Schedule
1. Immediate
2. +5 seconds
3. +30 seconds
4. +2 minutes
5. +10 minutes
6. +1 hour
7. +6 hours -> `DEAD`

---

## 4. Server-Sent Events (SSE) Contract

`GET /api/v1/transactions/{id}/stream`

```text
event: status
id: txn_01J8Z...
data: {"id":"txn_01J8Z...","status":"PENDING","amount":{"final":50142}}

: ping

event: status
id: txn_01J8Z...
data: {"id":"txn_01J8Z...","status":"PAID","paid_at":"2026-11-01T10:15:28Z","amount":{"final":50142}}
```

- Sends `: ping` keepalive comment every 15 seconds.
- Automatically closes the connection upon reaching terminal status (`PAID`, `EXPIRED`, `CANCELED`) or after a 5-minute timeout.
