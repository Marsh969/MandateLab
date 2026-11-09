//! Testnet probe (I07): broadcasts forbidden transactions signed with the
//! agent's *function-call* key directly to the testnet RPC, with no client-side
//! permission pre-checks, so any rejection comes from the NEAR protocol.
//!
//! Each probe is expected to be rejected at transaction validation
//! (`InvalidTxError::InvalidAccessKeyError`), which means the transaction is
//! never included in a block and therefore has no on-chain hash.
//!
//! Usage (testnet only; the key file stays local and is never printed):
//!   AGENT_FC_KEY_FILE=~/.near-credentials/testnet/<agent>/<fc-key>.json \
//!   AGENT_ID=<agent.testnet> VAULT_ID=<vault.testnet> STRANGER_ID=<stranger.testnet> \
//!   [NEAR_RPC_URL=https://test.rpc.fastnear.com] cargo run --example testnet_key_probe

use near_workspaces::types::{NearToken, SecretKey};
use near_workspaces::{Account, AccountId};
use serde_json::json;

fn summarize(label: &str, res: anyhow::Result<String>) -> bool {
    match res {
        Ok(s) => {
            println!("{label}: UNEXPECTED SUCCESS {s}");
            false
        }
        Err(e) => {
            let text = format!("{e:?}");
            // Keep only the protocol error variant for the report.
            let reason = ["MethodNameMismatch", "RequiresFullAccess", "DepositWithFunctionCall", "ReceiverMismatch"]
                .into_iter()
                .find(|k| text.contains(k))
                .unwrap_or("OTHER");
            // "handler error" = the RPC node's own transaction-validation response.
            let from_node = text.contains("handler error") || text.contains("HandlerError");
            println!("{label}: REJECTED reason={reason} reported_by_rpc_node={from_node}");
            println!("  detail: {}", text.chars().take(600).collect::<String>());
            reason != "OTHER"
        }
    }
}

/// The public testnet RPC rate-limits bursts; space the probes out.
async fn pace() {
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let key_file = std::env::var("AGENT_FC_KEY_FILE")?;
    let agent_id: AccountId = std::env::var("AGENT_ID")?.parse()?;
    let vault: AccountId = std::env::var("VAULT_ID")?.parse()?;
    let stranger: AccountId = std::env::var("STRANGER_ID")?.parse()?;

    let rpc = std::env::var("NEAR_RPC_URL").unwrap_or_else(|_| "https://rpc.testnet.near.org".into());
    let worker = near_workspaces::testnet().rpc_addr(&rpc).await?;
    println!("rpc={rpc}");
    let status = worker.status().await?;
    anyhow::ensure!(status.chain_id == "testnet", "refusing: chain_id {}", status.chain_id);
    // near-cli-rs per-key files hold `public_key` + `private_key` (no account_id).
    let kf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&key_file)?)?;
    let sk: SecretKey = kf["private_key"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("key file has no private_key"))?
        .parse()?;
    let agent = Account::from_secret_key(agent_id, sk, &worker);
    println!("agent={} signing_public_key={}", agent.id(), agent.secret_key().public_key());

    let mut all_ok = true;

    pace().await;
    let r = agent
        .call(&vault, "set_paused")
        .args_json(json!({ "paused": true }))
        .transact()
        .await
        .map(|o| format!("{o:?}"))
        .map_err(anyhow::Error::from);
    all_ok &= summarize("owner method set_paused", r);

    pace().await;
    let r = agent
        .call(&vault, "fund_vault")
        .transact()
        .await
        .map(|o| format!("{o:?}"))
        .map_err(anyhow::Error::from);
    all_ok &= summarize("owner method fund_vault", r);

    pace().await;
    let r = agent
        .transfer_near(&stranger, NearToken::from_yoctonear(1))
        .await
        .map(|o| format!("{o:?}"))
        .map_err(anyhow::Error::from);
    all_ok &= summarize("direct NEAR transfer (1 yocto)", r);

    pace().await;
    let r = agent
        .call(&vault, "request_payment")
        .args_json(json!({
            "request_id": "probe-deposit",
            "recipient_id": stranger,
            "amount": "1",
            "context_hash": "0".repeat(64),
        }))
        .deposit(NearToken::from_yoctonear(1))
        .transact()
        .await
        .map(|o| format!("{o:?}"))
        .map_err(anyhow::Error::from);
    all_ok &= summarize("request_payment with attached deposit", r);

    pace().await;
    let r = agent
        .call(&stranger, "request_payment")
        .transact()
        .await
        .map(|o| format!("{o:?}"))
        .map_err(anyhow::Error::from);
    all_ok &= summarize("call to a different receiver", r);

    println!("ALL_PROBES_REJECTED_AS_EXPECTED={all_ok}");
    anyhow::ensure!(all_ok, "a probe was not rejected for the expected reason");
    Ok(())
}
