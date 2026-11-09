//! Sandbox integration tests: a real local nearcore node (near-sandbox), real
//! native transfers, real asynchronous callbacks, and an agent that signs with
//! a real function-call access key restricted to `request_payment`.
//!
//! Requires the contract WASM to be built first:
//!   cargo near build non-reproducible-wasm
//! (or set MANDATE_VAULT_WASM=/path/to/mandate_vault.wasm).

use near_workspaces::network::Sandbox;
use near_workspaces::result::ExecutionFinalResult;
use near_workspaces::types::{AccessKey, AccessKeyPermission, Gas, KeyType, NearToken, SecretKey};
use near_workspaces::{Account, AccountId, Contract, Worker};
use serde_json::{json, Value};

const ONE_NEAR: u128 = 10u128.pow(24);
const MILLI: u128 = 10u128.pow(21);
const HASH: &str = "9a4f1c0e2b9d8c7f6e5d4c3b2a1908f7e6d5c4b3a291807f6e5d4c3b2a190807";
const PAY_GAS: Gas = Gas::from_tgas(50);

fn wasm() -> anyhow::Result<Vec<u8>> {
    let path = std::env::var("MANDATE_VAULT_WASM").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/target/near/mandate_vault.wasm"
        )
        .to_string()
    });
    std::fs::read(&path).map_err(|e| {
        anyhow::anyhow!(
            "cannot read {path}: {e}. Run `cargo near build non-reproducible-wasm` first."
        )
    })
}

struct Setup {
    worker: Worker<Sandbox>,
    vault: Contract,
    owner: Account,
    /// The agent account object signing with the *restricted* function-call key.
    agent: Account,
    merchant_a: Account,
    attacker: Account,
    ghost: AccountId,
}

/// Creates owner, agent, merchant, attacker and vault accounts, deploys and
/// initialises the vault, funds the vault, and replaces the agent signer with
/// a function-call key restricted to `vault.request_payment`.
async fn setup(second_recipient_ghost: bool, vault_funding: u128) -> anyhow::Result<Setup> {
    let worker = near_workspaces::sandbox().await?;
    let root = worker.root_account()?;
    let mk = |name: &'static str, bal: u128| {
        let root = root.clone();
        async move {
            root.create_subaccount(name)
                .initial_balance(NearToken::from_yoctonear(bal))
                .transact()
                .await?
                .into_result()
                .map_err(anyhow::Error::from)
        }
    };
    let owner = mk("owner", 50 * ONE_NEAR).await?;
    let agent_full = mk("agent", 5 * ONE_NEAR).await?;
    let merchant_a = mk("merchant-a", ONE_NEAR).await?;
    let merchant_b = mk("merchant-b", ONE_NEAR).await?;
    let attacker = mk("attacker", ONE_NEAR).await?;
    let vault_acc = mk("vault", 10 * ONE_NEAR).await?; // operational reserve + code storage
    let ghost: AccountId = format!("ghost-merchant.{}", root.id()).parse()?; // never created

    let vault = vault_acc.deploy(&wasm()?).await?.into_result()?;
    let recipients: Vec<AccountId> = if second_recipient_ghost {
        vec![merchant_a.id().clone(), ghost.clone()]
    } else {
        vec![merchant_a.id().clone(), merchant_b.id().clone()]
    };
    let now_ns = worker.view_block().await?.timestamp();
    vault
        .call("new")
        .args_json(json!({
            "owner_id": owner.id(),
            "agent_id": agent_full.id(),
            "allowed_recipients": recipients,
            "max_per_payment": (500 * MILLI).to_string(),
            "total_budget": (2 * ONE_NEAR).to_string(),
            "expires_at_ns": (now_ns + 72 * 3_600 * 1_000_000_000).to_string(),
        }))
        .transact()
        .await?
        .into_result()?;

    if vault_funding > 0 {
        owner
            .call(vault.id(), "fund_vault")
            .deposit(NearToken::from_yoctonear(vault_funding))
            .transact()
            .await?
            .into_result()?;
    }

    // Restricted key: receiver = vault, methods = [request_payment], gas allowance 1 NEAR.
    let fc_sk = SecretKey::from_random(KeyType::ED25519);
    agent_full
        .batch(agent_full.id())
        .add_key(
            fc_sk.public_key(),
            AccessKey::function_call_access(
                vault.id(),
                &["request_payment"],
                Some(NearToken::from_near(1)),
            ),
        )
        .transact()
        .await?
        .into_result()?;
    let agent = Account::from_secret_key(agent_full.id().clone(), fc_sk, &worker);

    Ok(Setup {
        worker,
        vault,
        owner,
        agent,
        merchant_a,
        attacker,
        ghost,
    })
}

async fn pay(
    s: &Setup,
    id: &str,
    to: &AccountId,
    amount: u128,
) -> anyhow::Result<ExecutionFinalResult> {
    Ok(s.agent
        .call(s.vault.id(), "request_payment")
        .args_json(json!({
            "request_id": id,
            "recipient_id": to,
            "amount": amount.to_string(),
            "context_hash": HASH,
        }))
        .gas(PAY_GAS)
        .transact()
        .await?)
}

async fn state(s: &Setup) -> anyhow::Result<Value> {
    Ok(s.vault.view("get_vault_state").await?.json()?)
}

async fn request(s: &Setup, id: &str) -> anyhow::Result<Value> {
    Ok(s.vault
        .view("get_request")
        .args_json(json!({ "request_id": id }))
        .await?
        .json()?)
}

async fn balance(w: &Worker<Sandbox>, id: &AccountId) -> anyhow::Result<u128> {
    Ok(w.view_account(id).await?.balance.as_yoctonear())
}

fn n(v: &Value, k: &str) -> u128 {
    v[k].as_str()
        .unwrap_or_else(|| panic!("{k} missing in {v}"))
        .parse()
        .unwrap()
}

fn assert_invariants(v: &Value) {
    let (a, p, s, sp, f) = (
        n(v, "vault_available"),
        n(v, "vault_pending"),
        n(v, "spent_successful"),
        n(v, "spent_pending"),
        n(v, "total_funded"),
    );
    assert_eq!(sp, p, "INV-02 spent_pending == vault_pending: {v}");
    assert_eq!(a + p + s, f, "INV-10 conservation: {v}");
    assert!(s + sp <= 2 * ONE_NEAR, "INV-01: {v}");
    // On-chain account must still hold the vault plus storage.
    assert!(
        n(v, "account_balance") >= n(v, "storage_staked") + a,
        "reserve: {v}"
    );
}

fn failure_text(r: ExecutionFinalResult) -> String {
    assert!(r.is_failure(), "expected failure, got success: {r:?}");
    format!("{:?}", r.into_result().unwrap_err())
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn init_only_by_contract_account_and_only_once() -> anyhow::Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let root = worker.root_account()?;
    let vault_acc = root
        .create_subaccount("vault")
        .initial_balance(NearToken::from_near(10))
        .transact()
        .await?
        .into_result()?;
    let attacker = root
        .create_subaccount("attacker")
        .transact()
        .await?
        .into_result()?;
    let vault = vault_acc.deploy(&wasm()?).await?.into_result()?;
    let now_ns = worker.view_block().await?.timestamp();
    let args = json!({
        "owner_id": attacker.id(),
        "agent_id": root.id(),
        "allowed_recipients": [attacker.id()],
        "max_per_payment": "1",
        "total_budget": "1",
        "expires_at_ns": (now_ns + 3_600_000_000_000).to_string(),
    });
    // Front-running init by a third party fails.
    let r = attacker
        .call(vault.id(), "new")
        .args_json(&args)
        .transact()
        .await?;
    assert!(failure_text(r).contains("Method new is private"));
    // Contract account itself can init once...
    vault
        .call("new")
        .args_json(&args)
        .transact()
        .await?
        .into_result()?;
    // ...but not twice.
    let r = vault.call("new").args_json(&args).transact().await?;
    assert!(failure_text(r).contains("already been initialized"));
    Ok(())
}

/// P0 core: authorized 0.2 NEAR payment succeeds (I01); 3 NEAR to an
/// unauthorized account is rejected with no payout (I02); per-payment cap
/// (I03); replay (I06); all via the restricted function-call key.
#[tokio::test]
async fn p0_authorized_succeeds_unauthorized_rejected_replay_blocked() -> anyhow::Result<()> {
    let s = setup(false, 2 * ONE_NEAR).await?;
    let before = state(&s).await?;
    assert_eq!(n(&before, "vault_available"), 2 * ONE_NEAR);
    assert_invariants(&before);

    // I01 authorized payment
    let merchant_before = balance(&s.worker, s.merchant_a.id()).await?;
    let r = pay(&s, "p0-ok-1", s.merchant_a.id(), 200 * MILLI).await?;
    assert!(r.is_success(), "{r:?}");
    assert!(r.receipt_failures().is_empty(), "no failed receipts: {r:?}");
    assert!(r.logs().iter().any(|l| l.contains("payment_reserved")));
    assert!(r
        .logs()
        .iter()
        .any(|l| l.contains("payment_settled") && l.contains("SUCCEEDED")));
    let status: String = r.json()?; // final value = callback return value
    assert_eq!(status, "SUCCEEDED");
    let merchant_after = balance(&s.worker, s.merchant_a.id()).await?;
    assert_eq!(
        merchant_after - merchant_before,
        200 * MILLI,
        "merchant receives exactly 0.2 NEAR"
    );
    let req = request(&s, "p0-ok-1").await?;
    assert_eq!(req["status"], "SUCCEEDED");
    assert_eq!(req["amount"], (200 * MILLI).to_string());
    let after_ok = state(&s).await?;
    assert_eq!(n(&after_ok, "vault_available"), 1_800 * MILLI);
    assert_eq!(n(&after_ok, "spent_successful"), 200 * MILLI);
    assert_eq!(n(&after_ok, "vault_pending"), 0);
    assert_invariants(&after_ok);

    // I02 3 NEAR to an unauthorized (existing) account
    let attacker_before = balance(&s.worker, s.attacker.id()).await?;
    let r = pay(&s, "p0-bad-1", s.attacker.id(), 3 * ONE_NEAR).await?;
    let err = failure_text(r);
    assert!(err.contains("UNAPPROVED_RECIPIENT"), "{err}");
    assert_eq!(
        balance(&s.worker, s.attacker.id()).await?,
        attacker_before,
        "no payout"
    );
    assert!(
        request(&s, "p0-bad-1").await?.is_null(),
        "rejected request leaves no record"
    );
    let after_bad = state(&s).await?;
    for k in [
        "vault_available",
        "vault_pending",
        "spent_successful",
        "spent_pending",
        "total_funded",
    ] {
        assert_eq!(
            after_bad[k], after_ok[k],
            "{k} unchanged by rejected request"
        );
    }

    // I03 one yocto over the per-payment cap to an approved merchant
    let r = pay(&s, "p0-bad-2", s.merchant_a.id(), 500 * MILLI + 1).await?;
    assert!(failure_text(r).contains("PER_PAYMENT_LIMIT"));
    let r = pay(&s, "p0-bad-3", s.merchant_a.id(), 3 * ONE_NEAR).await?;
    assert!(failure_text(r).contains("PER_PAYMENT_LIMIT"));

    // I06 replay of the successful request id
    let m_before = balance(&s.worker, s.merchant_a.id()).await?;
    let r = pay(&s, "p0-ok-1", s.merchant_a.id(), 200 * MILLI).await?;
    assert!(failure_text(r).contains("DUPLICATE_REQUEST"));
    assert_eq!(
        balance(&s.worker, s.merchant_a.id()).await?,
        m_before,
        "no double payout"
    );

    let end = state(&s).await?;
    for k in ["vault_available", "spent_successful", "total_funded"] {
        assert_eq!(end[k], after_ok[k]);
    }
    assert_invariants(&end);
    Ok(())
}

/// I05: allowlisted but non-existent recipient. The transfer receipt fails,
/// the callback sees the failure, marks FAILED and restores the vault; the
/// refund returns the NEAR to the contract account.
#[tokio::test]
async fn failed_transfer_callback_restores_vault() -> anyhow::Result<()> {
    let s = setup(true, 2 * ONE_NEAR).await?;
    let ghost = s.ghost.clone();
    assert!(
        s.worker.view_account(&ghost).await.is_err(),
        "ghost must not exist"
    );
    let before = state(&s).await?;

    let r = pay(&s, "to-ghost", &ghost, 300 * MILLI).await?;
    // The top-level result is the callback's return value; the transfer receipt failed.
    assert!(
        !r.receipt_failures().is_empty(),
        "transfer receipt should fail: {r:?}"
    );
    let status: String = r.clone().json()?;
    assert_eq!(status, "FAILED");
    assert!(r
        .logs()
        .iter()
        .any(|l| l.contains("payment_settled") && l.contains("FAILED")));

    let req = request(&s, "to-ghost").await?;
    assert_eq!(req["status"], "FAILED");
    let after = state(&s).await?;
    assert_eq!(
        n(&after, "vault_available"),
        n(&before, "vault_available"),
        "restored"
    );
    assert_eq!(n(&after, "spent_successful"), 0);
    assert_eq!(n(&after, "vault_pending"), 0);
    assert_eq!(after["pending_count"], 0);
    // Refund came back: contract balance did not drop by the 0.3 NEAR.
    assert!(
        n(&after, "account_balance") + 10 * MILLI >= n(&before, "account_balance"),
        "refund returned: before {} after {}",
        before["account_balance"],
        after["account_balance"]
    );
    assert!(
        s.worker.view_account(&ghost).await.is_err(),
        "ghost still does not exist"
    );
    assert_invariants(&after);

    // The consumed id cannot be retried; a new id is required.
    let r = pay(&s, "to-ghost", s.merchant_a.id(), 300 * MILLI).await?;
    assert!(failure_text(r).contains("DUPLICATE_REQUEST"));
    let r = pay(&s, "retry-1", s.merchant_a.id(), 300 * MILLI).await?;
    assert_eq!(r.json::<String>()?, "SUCCEEDED");
    assert_invariants(&state(&s).await?);
    Ok(())
}

/// I04 + T04: five concurrent 0.5 NEAR requests against a 2 NEAR budget (vault
/// funded with 3 NEAR, so the budget — not the funds — is the binding limit).
/// Reservation happens before dispatch, so at most four can pass.
#[tokio::test]
async fn split_and_concurrent_requests_cannot_exceed_budget() -> anyhow::Result<()> {
    let s = setup(false, 3 * ONE_NEAR).await?;
    let m_before = balance(&s.worker, s.merchant_a.id()).await?;

    let futs = (0..5).map(|i| {
        let agent = s.agent.clone();
        let vault_id = s.vault.id().clone();
        let to = s.merchant_a.id().clone();
        async move {
            agent
                .call(&vault_id, "request_payment")
                .args_json(json!({
                    "request_id": format!("split-{i}"),
                    "recipient_id": to,
                    "amount": (500 * MILLI).to_string(),
                    "context_hash": HASH,
                }))
                .gas(PAY_GAS)
                .transact()
                .await
        }
    });
    let results = futures::future::join_all(futs).await;
    let mut ok = 0;
    let mut cap = 0;
    for r in results {
        let r = r?;
        if r.is_success() {
            assert_eq!(r.json::<String>()?, "SUCCEEDED");
            ok += 1;
        } else {
            let e = failure_text(r);
            assert!(e.contains("CUMULATIVE_LIMIT"), "{e}");
            cap += 1;
        }
    }
    assert_eq!((ok, cap), (4, 1));
    assert_eq!(
        balance(&s.worker, s.merchant_a.id()).await? - m_before,
        2 * ONE_NEAR
    );
    let st = state(&s).await?;
    assert_eq!(n(&st, "spent_successful"), 2 * ONE_NEAR);
    assert_eq!(n(&st, "budget_remaining"), 0);
    assert_eq!(
        n(&st, "vault_available"),
        ONE_NEAR,
        "unspent funds stay in the vault"
    );
    assert_invariants(&st);

    // Even 1 yocto more is refused.
    let r = pay(&s, "split-extra", s.merchant_a.id(), 1).await?;
    assert!(failure_text(r).contains("CUMULATIVE_LIMIT"));
    Ok(())
}

/// I07: the agent's function-call key can only call `request_payment` on the
/// vault, with no deposit. Everything else is rejected by the protocol.
#[tokio::test]
async fn restricted_key_cannot_transfer_or_call_owner_methods() -> anyhow::Result<()> {
    let s = setup(false, ONE_NEAR).await?;
    let pk = s.agent.secret_key().public_key();

    // Inspect the key on chain.
    let ak = s.worker.view_access_key(s.agent.id(), &pk).await?;
    match ak.permission {
        AccessKeyPermission::FunctionCall(fc) => {
            assert_eq!(&fc.receiver_id, s.vault.id().as_str());
            assert_eq!(fc.method_names, vec!["request_payment".to_string()]);
            assert_eq!(fc.allowance, Some(NearToken::from_near(1)));
        }
        other => panic!("expected function-call key, got {other:?}"),
    }

    let a_before = balance(&s.worker, s.attacker.id()).await?;
    // Direct NEAR transfer from the agent account.
    // 1 yocto keeps the cost under the gas allowance, isolating the action-type check.
    let e = s
        .agent
        .transfer_near(s.attacker.id(), NearToken::from_yoctonear(1))
        .await;
    let e = format!("{e:?}");
    println!("direct transfer with function-call key -> {e}");
    assert!(e.contains("RequiresFullAccess"), "{e}");
    assert_eq!(balance(&s.worker, s.attacker.id()).await?, a_before);

    // Owner-only methods on the vault.
    for m in [
        "set_paused",
        "set_policy",
        "fund_vault",
        "on_payment_complete",
    ] {
        let e = s
            .agent
            .call(s.vault.id(), m)
            .args_json(json!({"paused": true}))
            .transact()
            .await;
        let e = format!("{e:?}");
        assert!(e.contains("MethodNameMismatch"), "{m}: {e}");
    }
    // A different receiver.
    let e = s
        .agent
        .call(s.merchant_a.id(), "request_payment")
        .transact()
        .await;
    assert!(format!("{e:?}").contains("ReceiverMismatch"));
    // Attaching a deposit is impossible with a function-call key.
    let e = s
        .agent
        .call(s.vault.id(), "request_payment")
        .args_json(json!({"request_id":"dep","recipient_id":s.merchant_a.id(),"amount":"1","context_hash":HASH}))
        .deposit(NearToken::from_yoctonear(1))
        .transact()
        .await;
    assert!(
        format!("{e:?}").contains("DepositWithFunctionCall"),
        "{e:?}"
    );
    // Adding a full-access key to itself.
    let e = s
        .agent
        .batch(s.agent.id())
        .add_key(
            SecretKey::from_random(KeyType::ED25519).public_key(),
            AccessKey::full_access(),
        )
        .transact()
        .await;
    assert!(format!("{e:?}").contains("RequiresFullAccess"), "{e:?}");

    // The one allowed call works.
    let r = pay(&s, "fc-ok", s.merchant_a.id(), 100 * MILLI).await?;
    assert_eq!(r.json::<String>()?, "SUCCEEDED");
    Ok(())
}

/// U13/T07: outsiders (and the agent/owner) cannot forge a settlement callback.
#[tokio::test]
async fn forged_callback_rejected() -> anyhow::Result<()> {
    let s = setup(false, ONE_NEAR).await?;
    // Make a real pending->settled request, then try to forge callbacks for it
    // and for an unknown id from several accounts with full-access keys.
    pay(&s, "real", s.merchant_a.id(), 100 * MILLI)
        .await?
        .into_result()?;
    let before = state(&s).await?;
    for acct in [&s.attacker, &s.owner, &s.merchant_a] {
        for id in ["real", "unknown"] {
            let r = acct
                .call(s.vault.id(), "on_payment_complete")
                .args_json(json!({ "request_id": id }))
                .transact()
                .await?;
            assert!(failure_text(r).contains("Method on_payment_complete is private"));
        }
    }
    let after = state(&s).await?;
    for k in [
        "vault_available",
        "vault_pending",
        "spent_successful",
        "spent_pending",
    ] {
        assert_eq!(after[k], before[k]);
    }
    assert_eq!(request(&s, "real").await?["status"], "SUCCEEDED");
    Ok(())
}

/// Owner controls & reserve separation: pause blocks the agent; non-owner can't
/// pause; plain transfers to the vault account are NOT credited to the vault;
/// payouts can't exceed `vault_available` even if the account holds more.
#[tokio::test]
async fn pause_and_operational_reserve_separation() -> anyhow::Result<()> {
    let s = setup(false, 300 * MILLI).await?;

    let r = s
        .attacker
        .call(s.vault.id(), "set_paused")
        .args_json(json!({"paused": true}))
        .transact()
        .await?;
    assert!(failure_text(r).contains("OWNER_ONLY"));
    let r = s
        .attacker
        .call(s.vault.id(), "fund_vault")
        .deposit(NearToken::from_millinear(100))
        .transact()
        .await?;
    assert!(failure_text(r).contains("OWNER_ONLY"));

    s.owner
        .call(s.vault.id(), "set_paused")
        .args_json(json!({"paused": true}))
        .transact()
        .await?
        .into_result()?;
    let r = pay(&s, "while-paused", s.merchant_a.id(), 100 * MILLI).await?;
    assert!(failure_text(r).contains("PAUSED"));
    s.owner
        .call(s.vault.id(), "set_paused")
        .args_json(json!({"paused": false}))
        .transact()
        .await?
        .into_result()?;

    // A plain transfer to the contract account increases the reserve, not the vault.
    let before = state(&s).await?;
    s.owner
        .transfer_near(s.vault.id(), NearToken::from_near(5))
        .await?
        .into_result()?;
    let after = state(&s).await?;
    assert_eq!(after["vault_available"], before["vault_available"]);
    assert!(
        n(&after, "operational_reserve")
            >= n(&before, "operational_reserve") + 5 * ONE_NEAR - MILLI
    );

    // Contract holds >15 NEAR but only 0.3 NEAR is in the vault.
    let r = pay(&s, "too-much", s.merchant_a.id(), 300 * MILLI + 1).await?;
    assert!(failure_text(r).contains("INSUFFICIENT_VAULT"));
    let r = pay(&s, "all-in", s.merchant_a.id(), 300 * MILLI).await?;
    assert_eq!(r.json::<String>()?, "SUCCEEDED");
    let r = pay(&s, "one-more", s.merchant_a.id(), 1).await?;
    assert!(failure_text(r).contains("INSUFFICIENT_VAULT"));
    assert_invariants(&state(&s).await?);
    Ok(())
}
