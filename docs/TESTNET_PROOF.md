# MandateLab — NEAR testnet P0 proof

All values below were obtained from real testnet RPC responses on **2026-11-09** and are
re-checkable by anyone, with no keys, using:

```bash
node scripts/check-testnet.ts --vault vault.mandatelab-p0-40db26.testnet
```

The full machine-readable capture (final transaction statuses, every receipt and log,
signer public keys, views, balances) is in
[`docs/evidence/testnet-p0-evidence.json`](evidence/testnet-p0-evidence.json), produced by
`scripts/check-testnet.ts` with `wait_until: FINAL`.

> Testnet demonstration only. Not audited. The deployer holds full-access keys to the
> vault account (code upgrade is possible) — see *Trust assumptions*.

## Summary

| # | Scenario | Signed with | Tx hash | Final outcome |
|---|---|---|---|---|
| 1 | Authorized **0.2 NEAR** to approved `merchant-a` | agent **function-call key** | [`3Pykat9paLowvkmmpiRFjNrWk3dzuioKDpxqDJRvvGXJ`](https://testnet.nearblocks.io/txns/3Pykat9paLowvkmmpiRFjNrWk3dzuioKDpxqDJRvvGXJ) | `SuccessValue "SUCCEEDED"`; merchant **+0.2 NEAR exactly** |
| 2 | Unauthorized **3 NEAR** to non-allowlisted `stranger` | same function-call key | [`4Wqgw5965CkhtJuzdGuMGvWDT7zt94Nj3739L6ABmZMA`](https://testnet.nearblocks.io/txns/4Wqgw5965CkhtJuzdGuMGvWDT7zt94Nj3739L6ABmZMA) | `Failure`: `Smart contract panicked: UNAPPROVED_RECIPIENT`; stranger **+0** |
| 3 | Replay of request #1's ID | same function-call key | [`SMHaNEnXCdp4dnYdahMTbRPoJjUx1yBFxdu6mu2aKU3`](https://testnet.nearblocks.io/txns/SMHaNEnXCdp4dnYdahMTbRPoJjUx1yBFxdu6mu2aKU3) | `Failure`: `DUPLICATE_REQUEST`; no second payout |
| 4 | **3 NEAR** to *approved* `merchant-a` (over 0.5 cap) | same function-call key | [`7Hx5WSpesshXmajBMTFZJaZQJugSsNoS9dwHfZpprEnY`](https://testnet.nearblocks.io/txns/7Hx5WSpesshXmajBMTFZJaZQJugSsNoS9dwHfZpprEnY) | `Failure`: `PER_PAYMENT_LIMIT` |
| 5 | Function-call key used for owner methods / transfer / deposit / other receiver | same function-call key | *none — rejected by the RPC node before inclusion* | `InvalidAccessKeyError` ×5 (see below) |

Rejected contract calls (rows 2–4) are real included transactions whose `request_payment`
receipt failed; the panic rolled back all state, so no request record exists for them
(`get_request` → `null`) and no transfer receipt was ever created.

## Required fields

```
Network: testnet  (RPC status: chain_id=testnet, nearcore 2.14.0-rc.5, protocol 88)
Contract account: vault.mandatelab-p0-40db26.testnet
Contract deployed WASM SHA-256: 6528507b6063cd3ebc33a406344a086a3c03199ad983729e09f07f7b8b81c276
  (base58 7osqEuCkFb4B1D1k2uhXtQAMkmYa26Xk3cGCjQXXC1wb == on-chain code_hash from view_account)
  Built with: cargo near build non-reproducible-wasm (cargo-near 0.23.0, rustc 1.97.0,
  near-sdk 5.30.1) from commit b384264 contracts/mandate-vault
Owner account: mandatelab-p0-40db26.testnet (created via the testnet faucet)
Agent account: agent.mandatelab-p0-40db26.testnet
Agent public key + allowed receiver/method:
  ed25519:6uFPrePvg1af1RQCALgkkWgk6wVJaNp3PUN9MATtGzV
  FunctionCall { receiver_id: vault.mandatelab-p0-40db26.testnet,
                 method_names: ["request_payment"], allowance: 0.25 NEAR (gas only) }
  (added in tx E2kkdbyutqPcmmgdz7wZEoTNtBo2yMiwjaH9KwFDTPfM)
Approved merchants: merchant-a.mandatelab-p0-40db26.testnet, merchant-b.mandatelab-p0-40db26.testnet
Unauthorized recipient used: stranger.mandatelab-p0-40db26.testnet (existing account)
Authorized request ID / tx hash / status:
  mandatelab:testnet:vault.mandatelab-p0-40db26.testnet:4539da8d-b6cf-48e1-9820-bf8649f9d2d5
  3Pykat9paLowvkmmpiRFjNrWk3dzuioKDpxqDJRvvGXJ / SUCCEEDED
  created 2026-11-09T20:56:18.650Z, settled 2026-11-09T20:56:19.763Z (on-chain timestamps)
Unauthorized request ID / failed tx hash / error:
  mandatelab:testnet:vault.mandatelab-p0-40db26.testnet:03d53a4f-1e45-45e5-a968-22f7170c99fe
  4Wqgw5965CkhtJuzdGuMGvWDT7zt94Nj3739L6ABmZMA /
  "Smart contract panicked: UNAPPROVED_RECIPIENT: recipient is not on the owner-approved allowlist"
Merchant balance delta: merchant-a 100000000000000000000000 -> 300000000000000000000000
  (+200000000000000000000000 yocto = exactly 0.2 NEAR)
Stranger balance delta: 100000000000000000000000 -> 100000000000000000000000 (0)
```

### Initialization and funding

| Step | Tx hash | Signer | Result |
|---|---|---|---|
| Deploy + `new` (single batch, `new` is `#[private]`) | [`5Y2oMUSkxsLAYT4SmAi4rUv6jnqyDWewb9BbeXaqEZa1`](https://testnet.nearblocks.io/txns/5Y2oMUSkxsLAYT4SmAi4rUv6jnqyDWewb9BbeXaqEZa1) | vault account | Success; `policy_changed` event |
| `fund_vault` 2 NEAR | [`HZBXSEx5VfaHcSjPeNfGNWsov5qoU7rEp54hB6H62dPp`](https://testnet.nearblocks.io/txns/HZBXSEx5VfaHcSjPeNfGNWsov5qoU7rEp54hB6H62dPp) | owner | Returned `"2000000000000000000000000"` |
| Add agent function-call key | [`E2kkdbyutqPcmmgdz7wZEoTNtBo2yMiwjaH9KwFDTPfM`](https://testnet.nearblocks.io/txns/E2kkdbyutqPcmmgdz7wZEoTNtBo2yMiwjaH9KwFDTPfM) | agent (full-access key) | Success |

The vault account was created with 3 NEAR (code storage + operational reserve) **separately**
from the 2 NEAR credited through `fund_vault`.

### Policy (`get_policy`)

```json
{
  "owner_id": "mandatelab-p0-40db26.testnet",
  "agent_id": "agent.mandatelab-p0-40db26.testnet",
  "allowed_recipients": ["merchant-a.mandatelab-p0-40db26.testnet", "merchant-b.mandatelab-p0-40db26.testnet"],
  "max_per_payment": "500000000000000000000000",
  "total_budget": "2000000000000000000000000",
  "expires_at_ns": "1795121703596652800",
  "paused": false,
  "policy_version": 1
}
```
`expires_at_ns` = 2026-11-19T20:55:03Z.

### Vault state before (read right after funding)

```json
{
  "vault_available": "2000000000000000000000000", "vault_pending": "0",
  "spent_successful": "0", "spent_pending": "0",
  "total_funded": "2000000000000000000000000", "budget_remaining": "2000000000000000000000000",
  "pending_count": 0, "request_count": 0, "policy_version": 1,
  "account_balance": "4998566048135888800000000",
  "storage_staked": "1696550000000000000000000",
  "operational_reserve": "1302016048135888800000000"
}
```

### Vault state after all scenarios (2026-11-09T21:10:35Z, block-final)

```json
{
  "vault_available": "1800000000000000000000000", "vault_pending": "0",
  "spent_successful": "200000000000000000000000", "spent_pending": "0",
  "total_funded": "2000000000000000000000000", "budget_remaining": "1800000000000000000000000",
  "pending_count": 0, "request_count": 1, "policy_version": 1,
  "account_balance": "4798566048135888800000000",
  "storage_staked": "1699390000000000000000000",
  "operational_reserve": "1299176048135888800000000"
}
```

Reconciliation:
- `vault_available + vault_pending + spent_successful = 1.8 + 0 + 0.2 = 2.0 = total_funded` ✔
- `spent_pending == vault_pending == 0` (no in-flight requests) ✔
- `spent_successful + spent_pending = 0.2 <= total_budget 2.0` ✔
- Contract `account_balance` dropped by exactly 0.2 NEAR (4.998566… → 4.798566…).
- Only one request record exists (`request_count: 1`); the three rejected calls left no record.
- Operational reserve fell by 0.00284 NEAR = storage staked for the one new request record.

### Authorized payment receipt flow (tx `3Pykat9p…`)

From `receipts_outcome` (see JSON evidence):
1. `vault` executes `request_payment` → `SuccessReceiptId` (reserve + `payment_reserved` event, status `PENDING`)
2. `merchant-a` executes the **Transfer** of 0.2 NEAR → `SuccessValue ""`
3. `vault` executes `on_payment_complete` → `SuccessValue "SUCCEEDED"` (`payment_settled` event)
4. gas refund receipts to `agent`

The transaction's final value is the callback's return value `"SUCCEEDED"` — the payment was
not called successful until the transfer result had been read by the callback.

### Restricted-key enforcement on testnet (I07)

`contracts/mandate-vault/examples/testnet_key_probe.rs` broadcasts forbidden transactions signed
with the agent's function-call key straight to a testnet RPC node
(`https://test.rpc.fastnear.com`; the official RPC rate-limited the burst). Captured output:

```
rpc=https://test.rpc.fastnear.com
agent=agent.mandatelab-p0-40db26.testnet signing_public_key=ed25519:6uFPrePvg1af1RQCALgkkWgk6wVJaNp3PUN9MATtGzV
owner method set_paused: REJECTED reason=MethodNameMismatch reported_by_rpc_node=true
owner method fund_vault: REJECTED reason=MethodNameMismatch reported_by_rpc_node=true
direct NEAR transfer (1 yocto): REJECTED reason=RequiresFullAccess reported_by_rpc_node=true
request_payment with attached deposit: REJECTED reason=DepositWithFunctionCall reported_by_rpc_node=true
call to a different receiver: REJECTED reason=ReceiverMismatch reported_by_rpc_node=true
ALL_PROBES_REJECTED_AS_EXPECTED=true
```

Each error text was the node's `handler error: … InvalidAccessKeyError(...)`. Such transactions
are invalid and are **never included in a block**, so there are no tx hashes to link — this is
expected protocol behaviour, not missing evidence. (An earlier attempt via `near` CLI was refused
client-side by the CLI's own pre-check; that is *not* counted as protocol evidence.)

## Local verification

```
$ cd contracts/mandate-vault && cargo test --lib
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo near build non-reproducible-wasm
SHA-256 checksum hex : 6528507b6063cd3ebc33a406344a086a3c03199ad983729e09f07f7b8b81c276

$ cargo test --test sandbox -- --test-threads=2      # nearcore sandbox 2.14.0
test failed_transfer_callback_restores_vault ... ok
test forged_callback_rejected ... ok
test init_only_by_contract_account_and_only_once ... ok
test p0_authorized_succeeds_unauthorized_rejected_replay_blocked ... ok
test pause_and_operational_reserve_separation ... ok
test split_and_concurrent_requests_cannot_exceed_budget ... ok
test restricted_key_cannot_transfer_or_call_owner_methods ... ok
test result: ok. 7 passed; 0 failed

$ cargo clippy --all-targets   # 0 warnings
```

The sandbox tests were run in a container whose hard open-file limit (20000) is below what
neard requires (65535); they used `scripts/sandbox/` (an `LD_PRELOAD` shim that clamps that one
`setrlimit` call) via `NEAR_SANDBOX_BIN_PATH`. It is unnecessary on a normal machine.

## Not yet proven on testnet (covered only in sandbox / unit tests)

- **I05** failed transfer → callback `FAILED` → vault restored (sandbox: `failed_transfer_callback_restores_vault`).
- **I04** 5×0.5 NEAR split against 2 NEAR budget incl. concurrent submissions (sandbox).
- **I08** pause while a transfer is in flight — deterministic only in unit tests
  (`pause_while_pending_still_reconciles`); the sandbox test covers pause/unpause, not the interleaving.
- Forged callback (`#[private]`) — sandbox `forged_callback_rejected`.

## Trust assumptions and limitations

- The deployer (this session) holds the vault account's full-access key: it could redeploy code.
  The deployed hash above lets anyone detect a change; this is **not** an immutable contract.
- The agent account also still has its original full-access key (recovery material). Payments in
  this proof were signed only by the function-call key (`signer_public_key` in the evidence JSON).
  A production setup should keep that full-access key offline or delete it.
- The owner/agent keys were generated in an ephemeral cloud container and are not stored in the
  repository. If the container is gone, owner actions (pause, set_policy, more funding) on this
  deployment are no longer possible; redeploy with your own accounts using `docs/DEPLOYMENT.md`.
- No withdrawal method exists yet (P2): vault funds can only leave via policy-checked payments.
- Not audited. Testnet only.
