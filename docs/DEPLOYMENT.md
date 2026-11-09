# Deploying the MandateLab vault to NEAR testnet

These are the exact commands used for the proof in `TESTNET_PROOF.md`, with the account name
replaced by `$O`. They use **near-cli-rs 0.31.1** and **cargo-near 0.23.0**. Testnet only.
Never paste private keys into chat, commit them, or put them in a web bundle.

## 0. Tooling

```bash
rustup toolchain install 1.97.0 --target wasm32-unknown-unknown   # pinned in rust-toolchain.toml
```

```bash
cargo install cargo-near --version 0.23.0 --locked
```

```bash
cargo install near-cli-rs --version 0.31.1 --locked
```

On Debian/Ubuntu the two installs need `libudev-dev` and `pkg-config` (Ledger support).

## 1. Build and test

```bash
cd contracts/mandate-vault
cargo test --lib
cargo near build non-reproducible-wasm      # -> target/near/mandate_vault.wasm (prints SHA-256)
cargo test --test sandbox -- --test-threads=2
```

If the sandbox fails with `couldn't set the file descriptor limit to (65535, 65535)`, your
environment's hard `ulimit -n` is too low; see `scripts/sandbox/README.md`.

## 2. Accounts

```bash
O=your-name.testnet
```

```bash
near account create-account sponsor-by-faucet-service $O autogenerate-new-keypair save-to-legacy-keychain network-config testnet create
```

Sub-accounts (vault gets code storage + operational reserve; it is **not** vault funding):

```bash
for spec in "vault:3 NEAR" "agent:0.5 NEAR" "merchant-a:0.1 NEAR" "merchant-b:0.1 NEAR" "stranger:0.1 NEAR"; do
  near account create-account fund-myself "${spec%%:*}.$O" "${spec#*:}" autogenerate-new-keypair save-to-legacy-keychain sign-as $O network-config testnet sign-with-legacy-keychain send
done
```

## 3. Deploy + initialise in one transaction

`new` is `#[private]`, so only the vault account can initialise it — batching it with the deploy
removes any front-running window.

```bash
EXP=$(python3 -c 'import time;print(int((time.time()+10*86400)*1e9))')
near contract deploy vault.$O use-file contracts/mandate-vault/target/near/mandate_vault.wasm \
  with-init-call new json-args "{\"owner_id\":\"$O\",\"agent_id\":\"agent.$O\",\"allowed_recipients\":[\"merchant-a.$O\",\"merchant-b.$O\"],\"max_per_payment\":\"500000000000000000000000\",\"total_budget\":\"2000000000000000000000000\",\"expires_at_ns\":\"$EXP\"}" \
  prepaid-gas '100.0 Tgas' attached-deposit '0 NEAR' network-config testnet sign-with-legacy-keychain send
```

## 4. Fund the vault (owner only)

```bash
near contract call-function as-transaction vault.$O fund_vault json-args '{}' prepaid-gas '30.0 Tgas' attached-deposit '2 NEAR' sign-as $O network-config testnet sign-with-legacy-keychain send
```

## 5. Restricted agent key

```bash
near account add-key agent.$O grant-function-call-access --allowance '0.25 NEAR' --contract-account-id vault.$O --function-names request_payment autogenerate-new-keypair save-to-legacy-keychain network-config testnet sign-with-legacy-keychain send
```

The allowance only pays **gas**; it is not a payment cap — the contract enforces payments.
Note the new public key printed by the command; its key file is
`~/.near-credentials/testnet/agent.$O/ed25519_<public-key>.json`.

## 6. Payments signed with the function-call key

```bash
FCKEY=~/.near-credentials/testnet/agent.$O/ed25519_<public-key>.json
RID="mandatelab:testnet:vault.$O:$(python3 -c 'import uuid;print(uuid.uuid4())')"
HASH=$(printf '%s' "<invoice text>" | sha256sum | cut -d' ' -f1)
near contract call-function as-transaction vault.$O request_payment \
  json-args "{\"request_id\":\"$RID\",\"recipient_id\":\"merchant-a.$O\",\"amount\":\"200000000000000000000000\",\"context_hash\":\"$HASH\"}" \
  prepaid-gas '50.0 Tgas' attached-deposit '0 NEAR' sign-as agent.$O network-config testnet sign-with-access-key-file $FCKEY send
```

Repeat with `recipient_id` = `stranger.$O` and `amount` = `3000000000000000000000000` for the
rejection case, and with the same `$RID` for the replay case.

## 7. Protocol-level key checks and evidence

```bash
cd contracts/mandate-vault
NEAR_RPC_URL=https://test.rpc.fastnear.com AGENT_ID=agent.$O AGENT_FC_KEY_FILE=$FCKEY VAULT_ID=vault.$O STRANGER_ID=stranger.$O cargo run --example testnet_key_probe
```

```bash
node scripts/check-testnet.ts --vault vault.$O --accounts merchant-a.$O,stranger.$O --request "$RID" --tx <hash>:agent.$O --out docs/evidence/my-run.json
```

`check-testnet.ts` is read-only, needs no keys and refuses to run if the RPC's `chain_id` is not
`testnet`.
