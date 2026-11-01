# AGENTS.md — Autonomous Coding Agent Guidelines

> Version: 1.0 · Status: Final · Date: 2026-11-01  
> Target Project: QrisStatic2Dynamic (qris-api)  
> Related documents: [RULES](./RULES.md) · [PRD](./PRD.md) · [ERP](./ERP.md) · [README](./README.md)

---

## ⚠️ Important Operating Disclaimer

> **DISCLAIMER: USE AT YOUR OWN RISK (RISIKO DITANGGUNG PENGGUNA)**  
> Autonomous coding agents operating in this repository must strictly adhere to the EMVCo TLV specifications and merchant-safe invariants defined in `RULES.md`. Under no circumstances may an agent alter merchant identifier tags, bypass anti-replay nonce verifications, disable SSRF safeguards, or expose financial credentials.

---

## 1. Source of Truth Priority

When analyzing or modifying code, consult documents in this strict order:

1. **`RULES.md`** — Normative rules. If code contradicts `RULES.md`, the code is buggy.
2. **`PRD.md`** — Objectives, requirements, scope, and non-goals.
3. **`ERP.md`** — Database DDL, JSON contracts, signature algorithms, and state machines.
4. **`README.md`** — Operational instructions and runtime configurations.
5. **`AGENTS.md`** — Agent workflow and quality control guidelines (this document).

If a prompt conflicts with `RULES.md`:
- Do **NOT** quietly change the rule.
- Explicitly explain the conflict, cite the affected Rule ID, and prompt for confirmation.

---

## 2. Inviolable Core Invariants

1. **Merchant-Safe QRIS**:
   - Only Tag `01`, `54`, and `63` may change.
   - Tags `26–51`, `52`, `53`, `58`, `59`, `60`, `61`, `62`, `64` must remain **100% byte-identical** and retain original relative ordering.
   - Tag `62` (Additional Data) must never be injected, modified, or stripped.
   - Tags `55/56/57` are strictly forbidden in both input and output.
2. **Currency & Precision**:
   - All monetary values are integer Rupiah (`i64`). Floating point (`f64`) values for money are strictly prohibited.
3. **Cryptographic Integrity**:
   - Master and encryption keys are loaded exclusively from environment variables.
   - API keys are hashed with SHA-256; webhook secrets are encrypted with AES-256-GCM.
   - Secrets, raw QRIS payloads, and bank notification text must never be logged.
4. **Network Safety**:
   - Outbound webhooks must validate callback URLs against private, loopback, and multicast IP ranges (SSRF protection).
   - Redirects must never be followed.

---

## 3. Toolchain & Quality Gates

Target: **Rust stable ≥ 1.85, edition 2024**

Required checks before committing any change:
```bash
# 1. Format check
cargo fmt --all -- --check

# 2. Strict linter check
cargo clippy --all-targets --all-features -- -D warnings

# 3. Test suite
cargo test --all

# 4. Real fixture validation (local only, do not commit raw fixtures)
cargo test --test qris_vectors -- --ignored
```

---

## 4. Architectural Boundaries

- `src/domain/`: Pure functional domain logic (TLV parsing, CRC16, amount calculations, image QR decoding, QR rendering). No database or network dependencies.
- `src/services/`: Business services (merchant service, transaction management, mutation parsers, matchers, outbound delivery worker, sweeper, rate limiter, audit logger).
- `src/auth/`: Cryptographic operations, principal extractors, constant-time comparisons, and HMAC signing.
- `src/db/`: SQLite connection pooling, pragmas, migrations, and online backup logic.
- `src/api/`: Axum routers, handlers, middleware (rate limiting, security headers, request ID, signed requests, body limits).
- `src/bin/`: Executables (`main.rs` daemon, `qris-cli.rs` operational tool).

---

## 5. Definition of Done (DoD)

A task or pull request is complete only when:
- [x] Conforms strictly to `RULES.md` and `ERP.md`.
- [x] All unit, integration, and property tests pass without warnings.
- [x] `cargo fmt` and `cargo clippy -D warnings` are completely clean.
- [x] No sensitive credentials or customer payloads are logged.
- [x] Any schema changes include backwards-compatible additive migrations.
- [x] Golden vectors and real QRIS fixtures maintain zero regressions.
