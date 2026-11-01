-- 0001_init.sql
-- PRAGMAS WAJIB
PRAGMA foreign_keys = ON;

-- 1. MERCHANTS
CREATE TABLE merchants (
    id                          TEXT PRIMARY KEY,
    name                        TEXT NOT NULL,
    status                      TEXT NOT NULL DEFAULT 'ACTIVE'
                                CHECK (status IN ('ACTIVE','SUSPENDED','DELETED')),
    static_payload              TEXT NOT NULL,
    static_payload_sha256       TEXT NOT NULL,
    merchant_name               TEXT NOT NULL,
    merchant_city               TEXT NOT NULL,
    postal_code                 TEXT,
    mcc                         TEXT NOT NULL,
    nmid                        TEXT,
    acquirer_guid               TEXT,
    merchant_criteria           TEXT,
    default_fee_mode            TEXT NOT NULL DEFAULT 'NONE'
                                CHECK (default_fee_mode IN ('NONE','FIXED','PERCENT')),
    default_fee_fixed           INTEGER NOT NULL DEFAULT 0 CHECK (default_fee_fixed >= 0),
    default_fee_percent_bps     INTEGER NOT NULL DEFAULT 0 CHECK (default_fee_percent_bps BETWEEN 0 AND 10000),
    default_unique_code         INTEGER NOT NULL DEFAULT 1 CHECK (default_unique_code IN (0,1)),
    default_ttl_seconds         INTEGER NOT NULL DEFAULT 900 CHECK (default_ttl_seconds BETWEEN 60 AND 86400),
    callback_url                TEXT,
    callback_events             TEXT NOT NULL DEFAULT '["transaction.paid"]',
    created_at                  INTEGER NOT NULL,
    updated_at                  INTEGER NOT NULL,
    deleted_at                  INTEGER
);

CREATE UNIQUE INDEX ux_merchants_nmid
    ON merchants(nmid)
    WHERE nmid IS NOT NULL AND deleted_at IS NULL;

CREATE UNIQUE INDEX ux_merchants_payload
    ON merchants(static_payload_sha256)
    WHERE deleted_at IS NULL;

CREATE INDEX ix_merchants_status
    ON merchants(status, deleted_at);

-- 2. MERCHANT KEYS
CREATE TABLE merchant_keys (
    id            TEXT PRIMARY KEY,
    merchant_id   TEXT NOT NULL REFERENCES merchants(id) ON DELETE CASCADE,
    key_prefix    TEXT NOT NULL UNIQUE,
    key_hash      TEXT NOT NULL CHECK (length(key_hash) = 64),
    label         TEXT,
    status        TEXT NOT NULL DEFAULT 'ACTIVE'
                  CHECK (status IN ('ACTIVE','REVOKED')),
    created_at    INTEGER NOT NULL,
    last_used_at  INTEGER,
    revoked_at    INTEGER
);

CREATE INDEX ix_merchant_keys_lookup
    ON merchant_keys(key_prefix, status);

CREATE INDEX ix_merchant_keys_merchant
    ON merchant_keys(merchant_id, status);

-- 3. SIGNING SECRETS
CREATE TABLE signing_secrets (
    id            TEXT PRIMARY KEY,
    merchant_id   TEXT NOT NULL REFERENCES merchants(id) ON DELETE CASCADE,
    secret_prefix TEXT NOT NULL,
    secret_enc    TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'ACTIVE'
                  CHECK (status IN ('ACTIVE','REVOKED')),
    created_at    INTEGER NOT NULL,
    revoked_at    INTEGER
);

CREATE UNIQUE INDEX ux_signing_secret_active
    ON signing_secrets(merchant_id)
    WHERE status = 'ACTIVE';

CREATE INDEX ix_signing_secrets_merchant
    ON signing_secrets(merchant_id, status);

-- 4. NOTIFICATION SOURCES
CREATE TABLE notification_sources (
    id            TEXT PRIMARY KEY,
    merchant_id   TEXT NOT NULL REFERENCES merchants(id) ON DELETE CASCADE,
    channel       TEXT NOT NULL
                  CHECK (channel IN ('ANDROID','EMAIL','AGGREGATOR','CUSTOM')),
    label         TEXT NOT NULL,
    secret_prefix TEXT NOT NULL,
    secret_enc    TEXT NOT NULL,
    config_json   TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(config_json)),
    status        TEXT NOT NULL DEFAULT 'ACTIVE'
                  CHECK (status IN ('ACTIVE','REVOKED')),
    created_at    INTEGER NOT NULL,
    last_seen_at  INTEGER,
    revoked_at    INTEGER
);

CREATE INDEX ix_sources_merchant
    ON notification_sources(merchant_id, status);

-- 5. TRANSACTIONS
CREATE TABLE transactions (
    id                         TEXT PRIMARY KEY,
    merchant_id                TEXT NOT NULL REFERENCES merchants(id) ON DELETE RESTRICT,
    merchant_ref               TEXT,
    reference_label            TEXT,
    idempotency_key            TEXT,
    base_amount                INTEGER NOT NULL CHECK (base_amount > 0),
    fee_mode                   TEXT NOT NULL
                               CHECK (fee_mode IN ('NONE','FIXED','PERCENT')),
    fee_fixed                  INTEGER NOT NULL DEFAULT 0 CHECK (fee_fixed >= 0),
    fee_percent_bps            INTEGER NOT NULL DEFAULT 0 CHECK (fee_percent_bps BETWEEN 0 AND 10000),
    fee_amount                 INTEGER NOT NULL DEFAULT 0 CHECK (fee_amount >= 0),
    unique_code                INTEGER NOT NULL DEFAULT 0 CHECK (unique_code BETWEEN 0 AND 999),
    final_amount               INTEGER NOT NULL,
    currency                   TEXT NOT NULL DEFAULT 'IDR' CHECK (currency = 'IDR'),
    dynamic_payload            TEXT NOT NULL,
    dynamic_payload_sha256     TEXT NOT NULL,
    crc                        TEXT NOT NULL CHECK (length(crc) = 4),
    status                     TEXT NOT NULL DEFAULT 'PENDING'
                               CHECK (status IN ('PENDING','PAID','EXPIRED','CANCELED','AMBIGUOUS')),
    payment_source             TEXT
                               CHECK (payment_source IN ('ANDROID','EMAIL','AGGREGATOR','CUSTOM','MANUAL')),
    paid_event_id              TEXT,
    expires_at                 INTEGER NOT NULL,
    paid_at                    INTEGER,
    canceled_at                INTEGER,
    cancel_reason              TEXT,
    created_at                 INTEGER NOT NULL,
    updated_at                 INTEGER NOT NULL,
    CHECK (final_amount = base_amount + fee_amount + unique_code),
    CHECK (final_amount <= 9999999999999)
);

CREATE UNIQUE INDEX ux_txn_idempotency
    ON transactions(merchant_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE UNIQUE INDEX ux_txn_active_amount
    ON transactions(merchant_id, final_amount)
    WHERE status = 'PENDING';

CREATE UNIQUE INDEX ux_txn_active_code
    ON transactions(merchant_id, unique_code)
    WHERE status = 'PENDING' AND unique_code > 0;

CREATE INDEX ix_txn_merchant_created
    ON transactions(merchant_id, created_at DESC);

CREATE INDEX ix_txn_status_expires
    ON transactions(status, expires_at);

-- 6. WEBHOOK EVENTS
CREATE TABLE webhook_events (
    id                TEXT PRIMARY KEY,
    source_id         TEXT NOT NULL REFERENCES notification_sources(id) ON DELETE RESTRICT,
    merchant_id       TEXT NOT NULL REFERENCES merchants(id) ON DELETE RESTRICT,
    channel           TEXT NOT NULL,
    event_key         TEXT NOT NULL,
    raw_payload_sha256 TEXT NOT NULL,
    raw_text_masked   TEXT,
    parsed_amount     INTEGER CHECK (parsed_amount IS NULL OR parsed_amount > 0),
    parsed_direction  TEXT CHECK (parsed_direction IN ('CR','DR','UNKNOWN')),
    parser_version    TEXT,
    match_reason      TEXT,
    status            TEXT NOT NULL DEFAULT 'RECEIVED'
                      CHECK (status IN ('RECEIVED','MATCHED','UNMATCHED','DUPLICATE','IGNORED','ERROR')),
    transaction_id    TEXT REFERENCES transactions(id) ON DELETE SET NULL,
    received_at       INTEGER NOT NULL,
    processed_at      INTEGER
);

CREATE UNIQUE INDEX ux_webhook_event_key
    ON webhook_events(source_id, event_key);

CREATE INDEX ix_webhook_events_status
    ON webhook_events(merchant_id, status, received_at DESC);

CREATE INDEX ix_webhook_events_txn
    ON webhook_events(transaction_id);

-- 7. WEBHOOK DELIVERIES
CREATE TABLE webhook_deliveries (
    id               TEXT PRIMARY KEY,
    merchant_id      TEXT NOT NULL REFERENCES merchants(id) ON DELETE RESTRICT,
    transaction_id   TEXT REFERENCES transactions(id) ON DELETE SET NULL,
    event_type       TEXT NOT NULL
                     CHECK (event_type IN ('transaction.paid','transaction.expired','transaction.canceled','merchant.test')),
    url              TEXT NOT NULL,
    payload_json     TEXT NOT NULL CHECK (json_valid(payload_json)),
    status           TEXT NOT NULL DEFAULT 'PENDING'
                     CHECK (status IN ('PENDING','DELIVERED','FAILED','DEAD')),
    attempts         INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at  INTEGER NOT NULL,
    last_status_code INTEGER,
    last_error       TEXT,
    created_at       INTEGER NOT NULL,
    delivered_at     INTEGER
);

CREATE UNIQUE INDEX ux_delivery_event
    ON webhook_deliveries(transaction_id, event_type)
    WHERE transaction_id IS NOT NULL;

CREATE INDEX ix_deliveries_pending
    ON webhook_deliveries(status, next_attempt_at)
    WHERE status IN ('PENDING','FAILED');

-- 8. AUDIT LOGS
CREATE TABLE audit_logs (
    id            TEXT PRIMARY KEY,
    merchant_id   TEXT REFERENCES merchants(id) ON DELETE SET NULL,
    actor_type    TEXT NOT NULL CHECK (actor_type IN ('MASTER','MERCHANT','SYSTEM')),
    actor_id      TEXT NOT NULL,
    action        TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id   TEXT NOT NULL,
    ip_address    TEXT,
    user_agent    TEXT,
    changes_json  TEXT CHECK (changes_json IS NULL OR json_valid(changes_json)),
    created_at    INTEGER NOT NULL
);

CREATE INDEX ix_audit_merchant
    ON audit_logs(merchant_id, created_at DESC);

CREATE INDEX ix_audit_actor
    ON audit_logs(actor_type, actor_id, created_at DESC);

CREATE INDEX ix_audit_action
    ON audit_logs(action, created_at DESC);

-- 9. REQUEST NONCES
CREATE TABLE request_nonces (
    credential_id TEXT NOT NULL,
    nonce         TEXT NOT NULL,
    seen_at       INTEGER NOT NULL,
    PRIMARY KEY (credential_id, nonce)
);

CREATE INDEX ix_nonces_seen ON request_nonces(seen_at);
