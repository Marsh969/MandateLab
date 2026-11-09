// Read-only NEAR testnet proof verifier for MandateLab.
//
// No dependencies, no keys. Uses only public JSON-RPC reads. Hard-fails if the
// RPC endpoint is not testnet.
//
// Usage (Node >= 22.18 runs .ts directly via type stripping):
//   node scripts/check-testnet.ts --vault <vault.testnet> \
//     [--agent <agent.testnet>] [--accounts a.testnet,b.testnet] \
//     [--request <id>]... [--tx <hash>:<signer>]... [--out evidence.json]
//
// Env: NEAR_RPC_URL (default https://rpc.testnet.near.org)

const RPC = process.env.NEAR_RPC_URL ?? "https://rpc.testnet.near.org";

type Json = any;

async function rpc(method: string, params: Json): Promise<Json> {
  for (let attempt = 0; attempt < 5; attempt++) {
    const res = await fetch(RPC, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: "mandatelab", method, params }),
    });
    if (res.status === 429 || res.status >= 500) {
      await new Promise((r) => setTimeout(r, 1000 * 2 ** attempt));
      continue;
    }
    const body = await res.json();
    if (body.error) throw new Error(`${method}: ${JSON.stringify(body.error)}`);
    return body.result;
  }
  throw new Error(`${method}: RPC unavailable after retries`);
}

async function assertTestnet(): Promise<Json> {
  const s = await rpc("status", []);
  if (s.chain_id !== "testnet") {
    throw new Error(`Refusing to run: RPC chain_id is ${s.chain_id}, expected testnet`);
  }
  return { chain_id: s.chain_id, version: s.version?.version, protocol_version: s.protocol_version,
    latest_block_height: s.sync_info.latest_block_height, latest_block_time: s.sync_info.latest_block_time };
}

async function view(contract: string, method: string, args: Json = {}): Promise<Json> {
  const r = await rpc("query", {
    request_type: "call_function",
    finality: "final",
    account_id: contract,
    method_name: method,
    args_base64: Buffer.from(JSON.stringify(args)).toString("base64"),
  });
  if (r.error || !r.result) throw new Error(`${contract}.${method}: ${r.error ?? "no result"}`);
  return { block_height: r.block_height, value: JSON.parse(Buffer.from(r.result).toString()) };
}

async function viewAccount(id: string): Promise<Json> {
  try {
    const r = await rpc("query", { request_type: "view_account", finality: "final", account_id: id });
    return { account_id: id, exists: true, amount: r.amount, code_hash: r.code_hash,
      storage_usage: r.storage_usage, block_height: r.block_height };
  } catch (e) {
    if (String(e).includes("UNKNOWN_ACCOUNT") || String(e).includes("does not exist")) {
      return { account_id: id, exists: false };
    }
    throw e;
  }
}

async function accessKeys(id: string): Promise<Json> {
  const r = await rpc("query", { request_type: "view_access_key_list", finality: "final", account_id: id });
  return r.keys.map((k: Json) => ({ public_key: k.public_key, permission: k.access_key.permission }));
}

function decodeStatus(st: Json): Json {
  if (st.SuccessValue !== undefined) {
    const raw = Buffer.from(st.SuccessValue, "base64").toString();
    let value: Json = raw;
    try { value = raw === "" ? "" : JSON.parse(raw); } catch { /* keep raw */ }
    return { outcome: "SuccessValue", value };
  }
  if (st.SuccessReceiptId !== undefined) return { outcome: "SuccessReceiptId", receipt_id: st.SuccessReceiptId };
  if (st.Failure !== undefined) return { outcome: "Failure", failure: st.Failure };
  return { outcome: "Unknown", raw: st };
}

async function txStatus(hash: string, signer: string): Promise<Json> {
  const r = await rpc("EXPERIMENTAL_tx_status", { tx_hash: hash, sender_account_id: signer, wait_until: "FINAL" });
  const actionsByReceipt: Record<string, Json> = {};
  for (const rc of r.receipts ?? []) {
    actionsByReceipt[rc.receipt_id] = { receiver_id: rc.receiver_id, predecessor_id: rc.predecessor_id,
      actions: rc.receipt?.Action?.actions ?? null };
  }
  return {
    tx_hash: hash,
    signer_id: r.transaction.signer_id,
    signer_public_key: r.transaction.public_key,
    receiver_id: r.transaction.receiver_id,
    actions: r.transaction.actions,
    final_execution_status: r.final_execution_status,
    final_status: decodeStatus(r.status),
    tx_outcome: { block_hash: r.transaction_outcome.block_hash, status: decodeStatus(r.transaction_outcome.outcome.status) },
    receipts: r.receipts_outcome.map((o: Json) => ({
      receipt_id: o.id,
      block_hash: o.block_hash,
      executor_id: o.outcome.executor_id,
      status: decodeStatus(o.outcome.status),
      logs: o.outcome.logs,
      ...(actionsByReceipt[o.id] ?? {}),
    })),
    explorer: `https://testnet.nearblocks.io/txns/${hash}`,
  };
}

function parseArgs(argv: string[]) {
  const out: { vault?: string; agent?: string; accounts: string[]; requests: string[]; txs: string[]; out?: string } =
    { accounts: [], requests: [], txs: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i], v = argv[i + 1];
    if (a === "--vault") { out.vault = v; i++; }
    else if (a === "--agent") { out.agent = v; i++; }
    else if (a === "--accounts") { out.accounts.push(...v.split(",").filter(Boolean)); i++; }
    else if (a === "--request") { out.requests.push(v); i++; }
    else if (a === "--tx") { out.txs.push(v); i++; }
    else if (a === "--out") { out.out = v; i++; }
    else throw new Error(`unknown argument ${a}`);
  }
  if (!out.vault) throw new Error("--vault is required");
  return out;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const network = await assertTestnet();
  const vault = args.vault!;
  const evidence: Json = {
    captured_at: new Date().toISOString(),
    rpc: RPC,
    network,
    vault: await viewAccount(vault),
    policy: await view(vault, "get_policy"),
    vault_state: await view(vault, "get_vault_state"),
    vault_access_keys: await accessKeys(vault),
  };
  const agent = args.agent ?? evidence.policy.value.agent_id;
  evidence.agent_access_keys = { account_id: agent, keys: await accessKeys(agent) };
  evidence.requests = {};
  for (const id of args.requests) evidence.requests[id] = await view(vault, "get_request", { request_id: id });
  evidence.accounts = [];
  for (const id of args.accounts) evidence.accounts.push(await viewAccount(id));
  evidence.transactions = [];
  for (const t of args.txs) {
    const [hash, signer] = t.split(":");
    evidence.transactions.push(await txStatus(hash, signer));
  }
  const text = JSON.stringify(evidence, null, 2);
  if (args.out) {
    const fs = await import("node:fs");
    fs.writeFileSync(args.out, text + "\n");
    console.error(`wrote ${args.out}`);
  }
  console.log(text);
}

main().catch((e) => { console.error(String(e?.stack ?? e)); process.exit(1); });
