# MandateLab — AI can propose. Your policy decides.

NEAR Hackathon 2026 · AI Security & Defense track.

An AI purchasing agent may only *propose* payments. A Rust NEAR contract holds a small
**testnet-only** treasury and enforces the owner's mandate on chain — caller, recipient
allowlist, per-payment cap, cumulative budget (including in-flight payments), expiry, pause and
replay protection. The agent signs with a **function-call access key** that can only call
`request_payment` on the vault, so a compromised or prompt-injected agent can at most spend
within the mandate.

> **Status: P0 proof complete on testnet. Not audited. Testnet only.** The AI planner and
> dashboard (P1) have not been started — see *What works today*.

## What works today

| Area | Status | Evidence |
|---|---|---|
| `mandate-vault` contract (Rust, near-sdk 5.30.1) | Done | `contracts/mandate-vault/src/lib.rs` |
| Unit tests | 32/32 pass | `cargo test --lib` |
| Sandbox integration tests (real nearcore 2.14.0, real transfers/callbacks, real FC key) | 7/7 pass | `cargo test --test sandbox` |
| Testnet: authorized 0.2 NEAR payment | **SUCCEEDED**, merchant +0.2 NEAR | [`3Pykat9p…`](https://testnet.nearblocks.io/txns/3Pykat9paLowvkmmpiRFjNrWk3dzuioKDpxqDJRvvGXJ) |
| Testnet: 3 NEAR to unauthorized account | **Rejected** `UNAPPROVED_RECIPIENT`, recipient +0 | [`4Wqgw596…`](https://testnet.nearblocks.io/txns/4Wqgw5965CkhtJuzdGuMGvWDT7zt94Nj3739L6ABmZMA) |
| Testnet: replay / over-cap | Rejected `DUPLICATE_REQUEST` / `PER_PAYMENT_LIMIT` | `docs/TESTNET_PROOF.md` |
| Testnet: FC key can't call owner methods, transfer, attach deposit, or call other contracts | Rejected by RPC node (`InvalidAccessKeyError`) | `docs/TESTNET_PROOF.md` |
| NEAR AI Cloud planner, web dashboard, Supabase | **Not started** | — |

Full proof with account IDs, WASM hash, policy/vault JSON before and after, and balance deltas:
**[`docs/TESTNET_PROOF.md`](docs/TESTNET_PROOF.md)**. Per-test status: [`docs/TEST_STATUS.csv`](docs/TEST_STATUS.csv).

Testnet deployment: `vault.mandatelab-p0-40db26.testnet` (WASM SHA-256
`6528507b6063cd3ebc33a406344a086a3c03199ad983729e09f07f7b8b81c276`).

## Verify it yourself (no keys needed)

```bash
node scripts/check-testnet.ts --vault vault.mandatelab-p0-40db26.testnet
```

Requires Node ≥ 22.18 (runs TypeScript directly). Read-only; refuses to run against a non-testnet RPC.

## Contract API

| Method | Caller | Purpose |
|---|---|---|
| `new(owner_id, agent_id, allowed_recipients, max_per_payment, total_budget, expires_at_ns)` | vault account (`#[private]`, once) | Initialise; ≤2 recipients |
| `fund_vault()` payable | owner | Credits exactly the attached deposit |
| `request_payment(request_id, recipient_id, amount, context_hash) -> Promise` | agent | Validate → reserve → transfer → self-callback |
| `on_payment_complete(request_id)` | self only (`#[private]`) | Settle `PENDING` → `SUCCEEDED` / `FAILED` from the real transfer result |
| `set_paused(paused)` | owner | Blocks new requests; in-flight payments still settle |
| `set_policy(allowed_recipients, max_per_payment, total_budget, expires_at_ns)` | owner | Bumps version; can't erase history or go below committed spend |
| `get_policy()`, `get_vault_state()`, `get_request(request_id)` | anyone | Views |
| `check_payment(recipient_id, amount, request_id?)` | anyone | Advisory only; `request_payment` re-checks |

Amounts are yoctoNEAR decimal strings (`U128`). Error codes: `NOT_AGENT`, `OWNER_ONLY`, `PAUSED`,
`EXPIRED`, `ZERO_AMOUNT`, `PER_PAYMENT_LIMIT`, `UNAPPROVED_RECIPIENT`, `CUMULATIVE_LIMIT`,
`INSUFFICIENT_VAULT`, `DUPLICATE_REQUEST`, `INVALID_REQUEST_ID`, `INVALID_CONTEXT_HASH`,
`REQUEST_LIMIT`, `STORAGE_RESERVE_LOW`, `OVERFLOW`.

Accounting invariants (asserted after every step in unit, randomized and sandbox tests):
`spent_successful + spent_pending ≤ total_budget`; `spent_pending == vault_pending`;
`vault_available + vault_pending + spent_successful == total_funded`. The rest of the contract
balance is the operational/storage reserve, which no method can pay out.

### Implementation notes worth knowing

- **Callback result handling.** `#[callback_result] r: Result<(), PromiseError>` is deliberately
  not used: near-sdk JSON-decodes the promise result, and a successful native transfer returns
  empty bytes, so the callback would panic on every *successful* payment and leave it stuck
  `PENDING`. The callback reads `env::promise_result_checked` directly.
- **near-sdk 5.30.1, not 5.30.0.** With 5.30.0, cargo resolves `near-sys 0.4.1`, which moved
  two host functions behind a new feature and breaks compilation. 5.30.1 is the upstream patch
  that bumps the sibling crates together. `Cargo.lock` is committed.
- Authorization uses `predecessor_account_id`, never `signer_account_id`.

## Build and test

Prerequisites, exact versions and deployment commands: [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md).

```bash
cd contracts/mandate-vault
cargo test --lib
cargo near build non-reproducible-wasm
cargo test --test sandbox -- --test-threads=2
```

## Repository layout

```
contracts/mandate-vault/     Rust contract, unit tests (src/tests.rs), sandbox tests (tests/),
                             testnet key probe (examples/testnet_key_probe.rs)
scripts/check-testnet.ts     read-only testnet evidence collector
scripts/sandbox/             fd-limit shim for constrained containers (not needed normally)
docs/TESTNET_PROOF.md        P0 on-chain proof
docs/evidence/               raw RPC evidence JSON
docs/DEPLOYMENT.md           exact deploy commands
docs/TEST_STATUS.csv         per-test status, command and evidence
docs/spec/                   PRD, threat model, test plan, original prompt (source of truth)
```

## Trust assumptions and limitations

- Testnet demonstration; **not audited**; no production security claim.
- The deployer holds the vault account's full-access key and could redeploy code. Compare the
  on-chain `code_hash` with the published hash.
- The agent's original full-access key still exists (recovery material, never used to sign
  payments). The function-call key's allowance limits gas spend only.
- The contract enforces *what* may be paid, not whether a merchant delivered or an invoice is
  genuine; an approved merchant that turns malicious can still be paid within the mandate.
- A compromised owner can change the policy.
- No withdrawal method yet (P2). Request records are capped at 500 for the demo storage budget.
- AI/prompt-injection behaviour is not yet tested — no model integration exists yet.
