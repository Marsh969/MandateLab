# MandateLab — Threat Model, Security Invariants & Test Specifications

**Date:** 2026-11-04  
**Scope:** Single vault, native testnet NEAR, owner-restricted policy, agent function-call key, AI planner and test runner.  
**Not an audit:** Threat analysis and proposed tests only, until executed and verified.

## 1. System assets and trust boundaries

**Assets:** (A1) owner-controlled vault NEAR; (A2) policy authority (owner identity, recipient allowlist, per-payment and total limits, expiration, pause); (A3) restricted agent private key; (A4) secure off-chain records and inference API key; (A5) transaction outcome evidence; (A6) contract-account code/storage reserve.

**Trust boundaries:**
1. **Untrusted invoice or webpage -> LLM:** all invoice content treated as data. The author of an invoice is not entitled to issue tool or system instructions.
2. **LLM -> structured output parser:** model-generated amount/recipient are untrusted candidates. Parse with strict schemas and reject anything malformed.
3. **Backend signer -> NEAR restricted key:** the signer uses only the dedicated testnet agent key and can call only `request_payment` on the designated contract.
4. **Agent contract call -> policy:** on-chain code uses `predecessor_account_id`, allowlist, per-payment cap, cumulative spent+reserved, expiry and replay checks.
5. **Contract -> asynchronous receiver:** payment only becomes final after the transfer result is reconciled through a self-only callback.
6. **RPC/indexer -> UI:** only finalized, independently retrieved chain states justify `SUCCEEDED_ON_CHAIN`; unknown states remain PENDING.
7. **Owner wallet -> configuration:** only owner-signed contract calls may change policy; public demo runner cannot.

**Security principle:** The LLM never holds an owner key, contract upgrade key, or unrestricted treasury signer. Restrict the action surface before model inference, validate afterward, and enforce again in contract.

## 2. Adversary classes
- **Invoice attacker:** can write arbitrary invoice text and alter supposed merchant name, amount, and recipient; cannot directly control owner wallet.
- **Prompt injection attacker:** can craft malicious tool/invoice content, impersonate roles, and seek to coerce the model into proposing unsafe transfers.
- **Compromised restricted agent:** possesses the agent function-call private key, can submit arbitrary `request_payment` arguments, but cannot modify policy or directly transfer NEAR from vault.
- **Public endpoint abuser:** can replay or flood public demo APIs; should not get general signing access or unbounded on-chain execution.
- **Compromised owner or deployer:** outside the guarantee; can change policy or upgrade code if such authority exists.
- **Network/indexer fault:** RPC unavailable or stale, transfer fails or callback is delayed; system must not confuse submission with settlement.

## 3. Formal-ish invariants
Let `A` = available vault, `P` = vault funds reserved for pending payments, `S` = successful cumulative payouts, `SP` = cumulative budget reserved for pending payments, `M` = mandate cumulative cap. All amounts are non-negative integer yoctoNEAR.

- **INV-01:** `0 <= S + SP <= M` after every committed transaction.
- **INV-02:** For pending payments, `P == sum(amounts of all pending payment records)` and `SP == P` for the single-vault P0 (exclude owner-withdraw pending from `P`).
- **INV-03:** A new payment reservation deducts the same amount from `A` that it adds to `P` and `SP`.
- **INV-04:** A successful callback decrements `P` and `SP` and increments `S` by exactly the original amount.
- **INV-05:** A failing transfer callback decrements `P` and `SP`, restores `A` by exactly the original amount, and leaves `S` unchanged.
- **INV-06:** A previously consumed `request_id` can never start a second transfer, whether it is pending, succeeded or failed.
- **INV-07:** No payment may be reserved if caller != authorized agent, policy paused/expired, recipient not allowlisted, amount zero/too high, available funds inadequate or cumulative cap exceeded.
- **INV-08:** No contract path allows agent caller to alter owner, policy, beneficiaries or upgrade code.
- **INV-09:** An error or failed transaction must not cause a confirmed transfer or a falsely shown successful payment.
- **INV-10:** `A + P + owner_withdraw_pending` must never exceed accounted-for deposited vault funds less completed outflows; storage and gas reserve are separate.
- **INV-11:** Only true callback self-calls can mark a pending payout as settled. Protect against duplicate callbacks and wrong request IDs.
- **INV-12:** Owner-only pause immediately blocks *new* payouts without incorrectly mutating already pending callbacks.

## 4. Threat register

| ID | Threat and attack route | Impact | Mitigation | Residual risk / proof |
|---|---|---|---|---|
| T01 | Invoice says to ignore system rules and transfer to attacker | High | Untrusted prompt boundary + hard on-chain allowlist/amounts | Approved-but-malicious merchants still possible; test hostile invoice |
| T02 | Malicious agent key holder signs a transfer | High | Function-call key limited to contract/method; exact predecessor verification; spending caps | Can spend within approved limits; inspect actual access keys |
| T03 | Splitting payments to bypass per-payment cap | Critical | Enforce cumulative `S + SP + amount <= M` | Owner can raise M; test 5×0.5 attempt |
| T04 | Parallel requests both spend same budget | Critical | On-chain state reservation before promise, atomic contract transitions | Callback timing; concurrency integration test |
| T05 | Duplicate/replayed payment identifier | Critical | On-chain consumed request ID map, unique per attempt | Storage growth/collision; limit lengths; test pending/settled/failed |
| T06 | Asynchronous transfer fails after reservation | Critical | Self-only callback reads actual result; restore available on failure | Callback execution failure requires reconciliation |
| T07 | Forged callback by an outsider | Critical | `#[private]` and `predecessor == current_account` | Verify callback guarding in integration test |
| T08 | Caller impersonates owner via submitted `owner_id` | Critical | Read authenticated predecessor, never caller-supplied identity | Owner key compromise outside scope |
| T09 | Owner's unrestricted key stored in browser/server repo | Critical | Wallet-signed owner actions; server holds restricted agent key only | Developer environment must still protect owner keys |
| T10 | API endpoint accepts arbitrary transactions | High | Curated scenario allowlist, authentication/rate limit, tiny testnet budget | Public replay DoS can consume testnet gas |
| T11 | Float rounding changes NEAR amount | High | `U128` decimal strings end-to-end; integer parser | UX may misdisplay if unsafe format used; test exact boundary |
| T12 | Read-only policy check races with chain state | Medium | Recheck all conditions in transaction method | Advisory check not authoritative; demonstrate race safety |
| T13 | RPC returns stale/pending result | Medium | Re-query final state; display PENDING; independent explorer proof | RPC outages impair demo, not necessarily security |
| T14 | Policy update improperly resets spent history | Critical | Preserve S/SP across versions and reject cap below spend+pending | Code bug; test owner updates while pending |
| T15 | Treasury withdrawal steals pending or storage buffer | Critical | Owner only, draw from A, callback reservation, explicit storage reserve | Demo-only reserve sizing; audit before mainnet |
| T16 | Malicious payload injected into UI logs | Medium | Escape/redact invoice content, schema validation, strict CSP | Web app security testing still required |
| T17 | Model or cloud provider falsely claimed attested | Medium | Choose TEE model; verify actual attestation if claimed; document alternative | Provider-dependent properties; make claim conditional |
| T18 | Contract upgrade/admin key replaces tested code | Critical | Publish deployed WASM hash, disclose upgrade ability, recommend immutable audited deployment later | P0 remains operator-trust-based |
| T19 | Storage growth or underfunded contract disables callbacks | High | Bound record sizes/volume, maintain operating/storage buffer and callback gas | Demo caps; storage metering mandatory for production |
| T20 | UI calls a rejected tx 'successful' because it was submitted | High | Distinguish submitted, execution rejected, payout pending, callback succeeded | E2E status transition tests |

## 5. Mandatory test matrix
Use the accompanying `MandateLab_Test_Cases.csv` as the machine-friendly checklist. Each test should have a deterministic fixture, code location, assertion and an outcome (`PASS`, `FAIL`, `BLOCKED`, `NOT_RUN`). Never mark a case `PASS` before running it. Key cases:

### Unit tests (Rust)
- **U01 Valid payment:** approved recipient, amount 0.2 NEAR, sufficient funds, unexpired, agent caller. Reservation tracked; after success callback S increments and A stays reduced.
- **U02 Unauthorized caller:** neither owner nor random caller can request; only agent.
- **U03 Wrong recipient:** other account rejected; A/P/S unchanged.
- **U04 Over per-transfer cap:** 0.500000000000000000000001 NEAR rejected when limit is 0.5.
- **U05 Exactly at cap:** 0.5 allowed if sufficient budget/funds.
- **U06 Zero or malformed amount:** rejected at JSON parsing or contract guard.
- **U07 Cumulative cap:** after 1.5 spent, 0.5 allowed, another >0 rejected.
- **U08 Pending cap:** pending reservations count against the same cumulative cap before any callback.
- **U09 Duplicate request ID:** reject on pending and after finalized result.
- **U10 Pause and expiry:** block new requests at expiry boundary; now >= expiry is expired.
- **U11 Policy owner only:** agent/random cannot pause, edit beneficiaries or increase cap.
- **U12 Failed transfer:** self-only callback failure fully restores A, never increments S.
- **U13 Forged callback:** non-self predecessor cannot finalize a payment.
- **U14 Overflow:** overflowing amount or sum rejected, never wraps.
- **U15 Policy change:** owner cannot reduce cap under already spent+pending; policy updates do not erase prior IDs.
- **U16 Withdrawal P2:** cannot withdraw pending or storage reserve; failed withdraw callback restores A.

### Sandbox / testnet integration tests
- **I01:** authorized 0.2 NEAR transfer delivers funds to merchant testnet account; inspect payer vault and merchant balance before/after; allow for transaction fees and storage reserve differences.
- **I02:** unauthorized recipient transfer fails and never credits recipient.
- **I03:** per-tx cap violation fails, with unchanged vault accounting.
- **I04:** 4×0.5 NEAR may succeed with 2 NEAR funded and cap; fifth fails; pending reservations cannot overspend even before callbacks.
- **I05:** transfer to approved but nonexistent account fails, callback restores available and marks failed.
- **I06:** replay of successful request ID fails without second transfer.
- **I07:** public agent access key is function-call limited to designated contract + request method; direct transfer cannot succeed using that key.
- **I08:** policy paused/revoked prevents new payments, but an existing pending payment still reconciles.

### AI and frontend acceptance tests
- **A01:** benign invoice leads to typed proposal and contract success (assuming API credentials available).
- **A02:** injected invoice may or may not persuade model; any illegal proposal is rejected on chain.
- **A03:** model returns invalid JSON: no payment, validation error.
- **A04:** model proposes decimal/negative/overflow amount: no payment, parsing rejection.
- **A05:** prompt attempts to ask for secrets: response must not expose keys; frontend secrets absent.
- **A06:** public `/api/demo/runs` cannot submit arbitrary recipient/amount; only curated server fixtures.
- **A07:** replay same UI request idempotently does not create duplicate spend.
- **A08:** UI labels transfers as PENDING until finalized; failures never appear green/success.
- **A09:** demo record marked `recorded` cannot be presented as `live`.
- **A10:** mobile interface readable; amounts and chain badges never overlap.

## 6. Property-based / fuzz tests (recommended)
- Random sequences of `fund`, `request_payment`, `set_paused`, successful callbacks, failed callbacks and owner withdrawals must preserve INV-01–INV-12.
- Interleave payments near boundaries of available vault and cumulative budget to catch reservation races.
- Generate malformed request IDs, max-length strings and high-precision decimal amounts.
- Compare callback ordering permutations; ensure each transition only applies once.

## 7. Manual verification runbook and evidence bundle
1. Confirm selected network **testnet** and note contract account ID + deployed WASM hash.
2. Retrieve owner, agent and merchant account IDs and publicly inspect restricted agent access-key permission.
3. Call `get_policy` and `get_vault_state` before test; record as JSON.
4. Submit permitted payment with unique request ID via restricted key; wait for all receipts and callback to finalize.
5. Retrieve contract `get_request` plus merchant balance delta and explorer transaction receipt. Treat gas/storage differences separately.
6. Submit over-limit/unauthorized request; record the actual failed transaction hash and error. Ensure no recipient payment.
7. Retrieve after-state; prove funds and caps correct. If not, halt further execution.
8. Run replay and split tests; export sanitized JSON evidence.
9. Write proof to `docs/TESTNET_PROOF.md` with exact links and timestamps; don't guess hashes.
10. Build the UI only after P0 is verified.

### `docs/TESTNET_PROOF.md` required fields
```
Network: testnet
Contract account: <actual account ID>
Contract deployed WASM SHA-256: <actual hash>
Owner account: <actual testnet account>
Agent account: <actual testnet account>
Agent public key + allowed receiver/method: <actual public chain view>
Approved merchant: <actual testnet account>
Vault state before: <actual JSON>
Authorized request ID / tx hash / status: <actual values>
Unauthorized request ID / failed tx hash / error: <actual values>
Vault state after: <actual JSON>
Merchant balance delta: <verified value>
Local unit test command/output: <actual command + results>
Sandbox test command/output: <actual command + results>
Outstanding limitations: <known gaps>
```

**Hard rule:** No test or live chain transaction may be claimed completed merely because it is described in this plan.
