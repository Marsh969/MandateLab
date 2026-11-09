//! MandateLab `mandate-vault`: a single-vault NEAR contract that holds a small
//! native-NEAR treasury and enforces an owner-approved spending mandate for one
//! AI agent account.
//!
//! The agent is expected to sign with a *function-call* access key restricted to
//! this contract and the `request_payment` method. That key restriction limits
//! which method the agent can call; the payment policy itself (allowlist,
//! per-payment cap, cumulative budget, expiry, pause, replay protection) is
//! enforced here, on chain, independent of any model output.
//!
//! Accounting (all integer yoctoNEAR, checked arithmetic):
//! - `vault_available`  funds the agent may still draw on (subject to budget)
//! - `vault_pending`    funds reserved for in-flight transfers
//! - `spent_successful` cumulative payouts whose transfer finalised successfully
//! - `spent_pending`    cumulative budget reserved by in-flight transfers
//! - `total_funded`     sum of all `fund_vault` deposits
//!
//! Invariants (checked by `assert_invariants` in tests):
//! - `spent_successful + spent_pending <= total_budget`
//! - `spent_pending == vault_pending == sum(amount of PENDING requests)`
//! - `vault_available + vault_pending + spent_successful == total_funded`
//!
//! The contract account balance that is NOT tracked by `vault_available` /
//! `vault_pending` is the operational reserve (code + state storage staking and
//! a safety buffer). No method transfers it out.
//!
//! This is a testnet hackathon demonstration and has not been audited.

use near_sdk::json_types::{U128, U64};
use near_sdk::store::LookupMap;
use near_sdk::{
    env, near, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise, PromiseError,
};

/// P0 limits the allowlist to two merchants.
pub const MAX_RECIPIENTS: usize = 2;
/// Upper bound for a request id, e.g. `mandatelab:testnet:<vault-id>:<uuid-v4>`.
pub const MAX_REQUEST_ID_LEN: usize = 128;
/// Lifetime cap on stored request records (demo-only storage budget).
pub const MAX_REQUESTS: u32 = 500;
/// `context_hash` must be a lowercase hex SHA-256 digest (64 chars).
pub const CONTEXT_HASH_LEN: usize = 64;
/// Gas reserved for `on_payment_complete`. Unused gas from `request_payment`
/// is also forwarded to the callback (default unused-gas weight).
pub const CALLBACK_GAS: Gas = Gas::from_tgas(15);
/// Minimum balance (on top of storage staking and vault funds) that must stay
/// on the contract account after a payout is dispatched.
pub const MIN_OPERATING_BUFFER: NearToken = NearToken::from_millinear(50);
/// Event standard name used in `EVENT_JSON:` logs.
pub const EVENT_STANDARD: &str = "mandatelab";
pub const EVENT_VERSION: &str = "1.0.0";

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Requests,
}

/// Machine-readable policy decision codes (PRD section 5).
#[near(serializers = [json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PolicyError {
    NotAgent,
    OwnerOnly,
    Paused,
    Expired,
    ZeroAmount,
    PerPaymentLimit,
    UnapprovedRecipient,
    CumulativeLimit,
    InsufficientVault,
    DuplicateRequest,
    InvalidRequestId,
    InvalidContextHash,
    RequestLimit,
    StorageReserveLow,
    Overflow,
}

impl PolicyError {
    pub fn code(&self) -> &'static str {
        match self {
            PolicyError::NotAgent => "NOT_AGENT",
            PolicyError::OwnerOnly => "OWNER_ONLY",
            PolicyError::Paused => "PAUSED",
            PolicyError::Expired => "EXPIRED",
            PolicyError::ZeroAmount => "ZERO_AMOUNT",
            PolicyError::PerPaymentLimit => "PER_PAYMENT_LIMIT",
            PolicyError::UnapprovedRecipient => "UNAPPROVED_RECIPIENT",
            PolicyError::CumulativeLimit => "CUMULATIVE_LIMIT",
            PolicyError::InsufficientVault => "INSUFFICIENT_VAULT",
            PolicyError::DuplicateRequest => "DUPLICATE_REQUEST",
            PolicyError::InvalidRequestId => "INVALID_REQUEST_ID",
            PolicyError::InvalidContextHash => "INVALID_CONTEXT_HASH",
            PolicyError::RequestLimit => "REQUEST_LIMIT",
            PolicyError::StorageReserveLow => "STORAGE_RESERVE_LOW",
            PolicyError::Overflow => "OVERFLOW",
        }
    }

    fn message(&self) -> &'static str {
        match self {
            PolicyError::NotAgent => "caller is not the authorized agent account",
            PolicyError::OwnerOnly => "only the vault owner may call this method",
            PolicyError::Paused => "mandate is paused",
            PolicyError::Expired => "mandate has expired",
            PolicyError::ZeroAmount => "amount must be greater than zero",
            PolicyError::PerPaymentLimit => "amount exceeds the per-payment limit",
            PolicyError::UnapprovedRecipient => "recipient is not on the owner-approved allowlist",
            PolicyError::CumulativeLimit => "payment would exceed the cumulative mandate budget",
            PolicyError::InsufficientVault => "vault has insufficient available funds",
            PolicyError::DuplicateRequest => "request_id has already been used",
            PolicyError::InvalidRequestId => {
                "request_id is empty, too long or has invalid characters"
            }
            PolicyError::InvalidContextHash => "context_hash must be 64 lowercase hex characters",
            PolicyError::RequestLimit => "request record limit reached",
            PolicyError::StorageReserveLow => "contract operational/storage reserve is too low",
            PolicyError::Overflow => "arithmetic overflow",
        }
    }

    fn panic(self) -> ! {
        env::panic_str(&format!("{}: {}", self.code(), self.message()))
    }
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequestStatus {
    Pending,
    Succeeded,
    Failed,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentRequest {
    pub recipient_id: AccountId,
    pub amount: u128,
    pub context_hash: String,
    pub policy_version: u32,
    pub status: RequestStatus,
    pub created_at_ns: u64,
    pub settled_at_ns: Option<u64>,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentRequestView {
    pub request_id: String,
    pub recipient_id: AccountId,
    pub amount: U128,
    pub context_hash: String,
    pub policy_version: u32,
    pub status: RequestStatus,
    pub created_at_ns: U64,
    pub settled_at_ns: Option<U64>,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyView {
    pub owner_id: AccountId,
    pub agent_id: AccountId,
    pub allowed_recipients: Vec<AccountId>,
    pub max_per_payment: U128,
    pub total_budget: U128,
    pub expires_at_ns: U64,
    pub paused: bool,
    pub policy_version: u32,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultStateView {
    pub vault_available: U128,
    pub vault_pending: U128,
    pub spent_successful: U128,
    pub spent_pending: U128,
    pub total_funded: U128,
    /// `total_budget - spent_successful - spent_pending`
    pub budget_remaining: U128,
    pub pending_count: u32,
    pub request_count: u32,
    pub policy_version: u32,
    /// Raw contract account balance (vault + operational reserve).
    pub account_balance: U128,
    /// Balance locked for storage staking at current storage usage.
    pub storage_staked: U128,
    /// `account_balance - storage_staked - vault_available` (in-flight
    /// transfers have already left the account balance).
    pub operational_reserve: U128,
    pub block_timestamp_ns: U64,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentCheck {
    pub allowed: bool,
    pub code: Option<PolicyError>,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MandateVault {
    owner_id: AccountId,
    agent_id: AccountId,
    allowed_recipients: Vec<AccountId>,
    max_per_payment: u128,
    total_budget: u128,
    expires_at_ns: u64,
    paused: bool,
    policy_version: u32,

    vault_available: u128,
    vault_pending: u128,
    spent_successful: u128,
    spent_pending: u128,
    total_funded: u128,

    request_count: u32,
    pending_count: u32,
    requests: LookupMap<String, PaymentRequest>,
}

fn checked_add(a: u128, b: u128) -> u128 {
    a.checked_add(b)
        .unwrap_or_else(|| PolicyError::Overflow.panic())
}

fn checked_sub(a: u128, b: u128) -> u128 {
    a.checked_sub(b)
        .unwrap_or_else(|| PolicyError::Overflow.panic())
}

/// Accepts 1..=128 chars of `[A-Za-z0-9_.:-]`.
pub fn is_valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_REQUEST_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'))
}

pub fn is_valid_context_hash(h: &str) -> bool {
    h.len() == CONTEXT_HASH_LEN && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn emit_event(event: &str, data: near_sdk::serde_json::Value) {
    let payload = near_sdk::serde_json::json!({
        "standard": EVENT_STANDARD,
        "version": EVENT_VERSION,
        "event": event,
        "data": [data],
    });
    env::log_str(&format!("EVENT_JSON:{payload}"));
}

fn validate_policy_params(
    owner_id: &AccountId,
    agent_id: &AccountId,
    allowed_recipients: &[AccountId],
    max_per_payment: u128,
    total_budget: u128,
    expires_at_ns: u64,
) {
    let me = env::current_account_id();
    if allowed_recipients.is_empty() || allowed_recipients.len() > MAX_RECIPIENTS {
        env::panic_str("INVALID_POLICY: allowed_recipients must contain 1 or 2 accounts");
    }
    for (i, r) in allowed_recipients.iter().enumerate() {
        if allowed_recipients[..i].contains(r) {
            env::panic_str("INVALID_POLICY: duplicate recipient");
        }
        if r == agent_id || r == &me {
            env::panic_str("INVALID_POLICY: recipient may not be the agent or the vault itself");
        }
    }
    if owner_id == agent_id {
        env::panic_str("INVALID_POLICY: owner and agent must be different accounts");
    }
    if agent_id == &me || owner_id == &me {
        env::panic_str("INVALID_POLICY: owner/agent may not be the vault account");
    }
    if max_per_payment == 0 || total_budget == 0 {
        env::panic_str("INVALID_POLICY: limits must be greater than zero");
    }
    if max_per_payment > total_budget {
        env::panic_str("INVALID_POLICY: max_per_payment may not exceed total_budget");
    }
    if expires_at_ns <= env::block_timestamp() {
        env::panic_str("INVALID_POLICY: expires_at_ns must be in the future");
    }
}

#[near]
impl MandateVault {
    /// Initialise the single vault. `#[private]`: only the contract account
    /// itself (i.e. the deployer, typically in the same batch as the deploy)
    /// can initialise, preventing a third party from front-running init.
    /// `#[init]` refuses to run if state already exists.
    #[init]
    #[private]
    pub fn new(
        owner_id: AccountId,
        agent_id: AccountId,
        allowed_recipients: Vec<AccountId>,
        max_per_payment: U128,
        total_budget: U128,
        expires_at_ns: U64,
    ) -> Self {
        validate_policy_params(
            &owner_id,
            &agent_id,
            &allowed_recipients,
            max_per_payment.0,
            total_budget.0,
            expires_at_ns.0,
        );
        let this = Self {
            owner_id,
            agent_id,
            allowed_recipients,
            max_per_payment: max_per_payment.0,
            total_budget: total_budget.0,
            expires_at_ns: expires_at_ns.0,
            paused: false,
            policy_version: 1,
            vault_available: 0,
            vault_pending: 0,
            spent_successful: 0,
            spent_pending: 0,
            total_funded: 0,
            request_count: 0,
            pending_count: 0,
            requests: LookupMap::new(StorageKey::Requests),
        };
        emit_event(
            "policy_changed",
            near_sdk::serde_json::to_value(this.get_policy()).unwrap(),
        );
        this
    }

    // ------------------------------------------------------------------
    // Owner methods
    // ------------------------------------------------------------------

    /// Owner-only. Credits exactly the attached deposit to `vault_available`.
    /// Plain transfers to the contract account (not via this method) are NOT
    /// credited to the vault; they become operational reserve.
    #[payable]
    pub fn fund_vault(&mut self) -> U128 {
        self.assert_owner();
        let deposit = env::attached_deposit().as_yoctonear();
        if deposit == 0 {
            PolicyError::ZeroAmount.panic();
        }
        self.vault_available = checked_add(self.vault_available, deposit);
        self.total_funded = checked_add(self.total_funded, deposit);
        emit_event(
            "vault_funded",
            near_sdk::serde_json::json!({
                "amount_yocto": U128(deposit),
                "vault_available": U128(self.vault_available),
            }),
        );
        U128(self.vault_available)
    }

    /// Owner-only. Pausing blocks new payment requests immediately; in-flight
    /// transfers still settle through their callback.
    pub fn set_paused(&mut self, paused: bool) {
        self.assert_owner();
        self.paused = paused;
        emit_event(
            "policy_paused",
            near_sdk::serde_json::json!({ "paused": paused, "policy_version": self.policy_version }),
        );
    }

    /// Owner-only policy update. Owner and agent identities cannot be changed.
    /// Spending history, reservations and consumed request ids are preserved;
    /// `total_budget` may not be set below `spent_successful + spent_pending`.
    pub fn set_policy(
        &mut self,
        allowed_recipients: Vec<AccountId>,
        max_per_payment: U128,
        total_budget: U128,
        expires_at_ns: U64,
    ) -> PolicyView {
        self.assert_owner();
        validate_policy_params(
            &self.owner_id,
            &self.agent_id,
            &allowed_recipients,
            max_per_payment.0,
            total_budget.0,
            expires_at_ns.0,
        );
        let committed = checked_add(self.spent_successful, self.spent_pending);
        if total_budget.0 < committed {
            env::panic_str("INVALID_POLICY: total_budget below spent_successful + spent_pending");
        }
        self.allowed_recipients = allowed_recipients;
        self.max_per_payment = max_per_payment.0;
        self.total_budget = total_budget.0;
        self.expires_at_ns = expires_at_ns.0;
        self.policy_version = self
            .policy_version
            .checked_add(1)
            .unwrap_or_else(|| PolicyError::Overflow.panic());
        let view = self.get_policy();
        emit_event(
            "policy_changed",
            near_sdk::serde_json::to_value(&view).unwrap(),
        );
        view
    }

    // ------------------------------------------------------------------
    // Agent method
    // ------------------------------------------------------------------

    /// Agent-only. Validates the full mandate, reserves `amount` from the vault
    /// and the cumulative budget, records the request as PENDING, and
    /// dispatches a native transfer followed by a self-callback that settles
    /// the request. Not `#[payable]`: any attached deposit is rejected.
    ///
    /// A successful execution of this method only means the transfer was
    /// *scheduled*; the request is SUCCEEDED only after `on_payment_complete`.
    pub fn request_payment(
        &mut self,
        request_id: String,
        recipient_id: AccountId,
        amount: U128,
        context_hash: String,
    ) -> Promise {
        let caller = env::predecessor_account_id();
        if let Err(e) = self.evaluate(
            &caller,
            Some(&request_id),
            &recipient_id,
            amount.0,
            Some(&context_hash),
        ) {
            e.panic();
        }
        let amount = amount.0;

        // Reserve before dispatching the promise.
        self.vault_available = checked_sub(self.vault_available, amount);
        self.vault_pending = checked_add(self.vault_pending, amount);
        self.spent_pending = checked_add(self.spent_pending, amount);
        self.pending_count += 1;
        self.request_count += 1;
        let now = env::block_timestamp();
        self.requests.insert(
            request_id.clone(),
            PaymentRequest {
                recipient_id: recipient_id.clone(),
                amount,
                context_hash: context_hash.clone(),
                policy_version: self.policy_version,
                status: RequestStatus::Pending,
                created_at_ns: now,
                settled_at_ns: None,
            },
        );
        // Make the new record count towards storage usage before the reserve check.
        self.requests.flush();
        self.assert_operating_reserve(amount);

        emit_event(
            "payment_reserved",
            near_sdk::serde_json::json!({
                "request_id": request_id,
                "recipient": recipient_id,
                "amount_yocto": U128(amount),
                "context_hash": context_hash,
                "policy_version": self.policy_version,
                "status": RequestStatus::Pending,
            }),
        );

        Promise::new(recipient_id)
            .transfer(NearToken::from_yoctonear(amount))
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(CALLBACK_GAS)
                    .on_payment_complete(request_id),
            )
    }

    /// Self-only callback that settles a PENDING request from the actual
    /// transfer result.
    ///
    /// Note: `#[callback_result] r: Result<(), PromiseError>` is deliberately
    /// NOT used. near-sdk JSON-deserialises the promise result bytes, and a
    /// successful native transfer yields empty bytes, which fails to
    /// deserialise into `()` and would panic on every successful payment.
    #[private]
    pub fn on_payment_complete(&mut self, request_id: String) -> RequestStatus {
        if env::promise_results_count() != 1 {
            env::panic_str("CALLBACK_INVALID: expected exactly one promise result");
        }
        let transfer_ok = !matches!(env::promise_result_checked(0, 0), Err(PromiseError::Failed));

        let mut req = self
            .requests
            .get(&request_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("CALLBACK_INVALID: unknown request_id"));
        if req.status != RequestStatus::Pending {
            env::panic_str("CALLBACK_INVALID: request is not pending");
        }

        self.vault_pending = checked_sub(self.vault_pending, req.amount);
        self.spent_pending = checked_sub(self.spent_pending, req.amount);
        self.pending_count -= 1;
        if transfer_ok {
            self.spent_successful = checked_add(self.spent_successful, req.amount);
            req.status = RequestStatus::Succeeded;
        } else {
            self.vault_available = checked_add(self.vault_available, req.amount);
            req.status = RequestStatus::Failed;
        }
        req.settled_at_ns = Some(env::block_timestamp());
        let status = req.status;
        emit_event(
            "payment_settled",
            near_sdk::serde_json::json!({
                "request_id": request_id,
                "recipient": req.recipient_id,
                "amount_yocto": U128(req.amount),
                "policy_version": req.policy_version,
                "status": status,
            }),
        );
        self.requests.insert(request_id, req);
        status
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    pub fn get_policy(&self) -> PolicyView {
        PolicyView {
            owner_id: self.owner_id.clone(),
            agent_id: self.agent_id.clone(),
            allowed_recipients: self.allowed_recipients.clone(),
            max_per_payment: U128(self.max_per_payment),
            total_budget: U128(self.total_budget),
            expires_at_ns: U64(self.expires_at_ns),
            paused: self.paused,
            policy_version: self.policy_version,
        }
    }

    pub fn get_vault_state(&self) -> VaultStateView {
        let balance = env::account_balance().as_yoctonear();
        let staked = u128::from(env::storage_usage()) * env::storage_byte_cost().as_yoctonear();
        VaultStateView {
            vault_available: U128(self.vault_available),
            vault_pending: U128(self.vault_pending),
            spent_successful: U128(self.spent_successful),
            spent_pending: U128(self.spent_pending),
            total_funded: U128(self.total_funded),
            budget_remaining: U128(
                self.total_budget
                    .saturating_sub(self.spent_successful.saturating_add(self.spent_pending)),
            ),
            pending_count: self.pending_count,
            request_count: self.request_count,
            policy_version: self.policy_version,
            account_balance: U128(balance),
            storage_staked: U128(staked),
            operational_reserve: U128(
                balance
                    .saturating_sub(staked)
                    .saturating_sub(self.vault_available),
            ),
            block_timestamp_ns: U64(env::block_timestamp()),
        }
    }

    pub fn get_request(&self, request_id: String) -> Option<PaymentRequestView> {
        self.requests.get(&request_id).map(|r| PaymentRequestView {
            request_id: request_id.clone(),
            recipient_id: r.recipient_id.clone(),
            amount: U128(r.amount),
            context_hash: r.context_hash.clone(),
            policy_version: r.policy_version,
            status: r.status,
            created_at_ns: U64(r.created_at_ns),
            settled_at_ns: r.settled_at_ns.map(U64),
        })
    }

    /// Advisory, read-only prediction of whether the agent's request would be
    /// accepted right now. `request_payment` re-checks everything itself; this
    /// view is not a guarantee (state can change between view and call).
    pub fn check_payment(
        &self,
        recipient_id: AccountId,
        amount: U128,
        request_id: Option<String>,
    ) -> PaymentCheck {
        let agent = self.agent_id.clone();
        match self.evaluate(&agent, request_id.as_deref(), &recipient_id, amount.0, None) {
            Ok(()) => PaymentCheck {
                allowed: true,
                code: None,
            },
            Err(e) => PaymentCheck {
                allowed: false,
                code: Some(e),
            },
        }
    }

    // ------------------------------------------------------------------
    // Internal
    // ------------------------------------------------------------------

    fn assert_owner(&self) {
        if env::predecessor_account_id() != self.owner_id {
            PolicyError::OwnerOnly.panic();
        }
    }

    /// Single source of truth for the payment policy, shared by
    /// `request_payment` (authoritative) and `check_payment` (advisory).
    fn evaluate(
        &self,
        caller: &AccountId,
        request_id: Option<&str>,
        recipient_id: &AccountId,
        amount: u128,
        context_hash: Option<&str>,
    ) -> Result<(), PolicyError> {
        if caller != &self.agent_id {
            return Err(PolicyError::NotAgent);
        }
        if self.paused {
            return Err(PolicyError::Paused);
        }
        if env::block_timestamp() >= self.expires_at_ns {
            return Err(PolicyError::Expired);
        }
        if let Some(id) = request_id {
            if !is_valid_request_id(id) {
                return Err(PolicyError::InvalidRequestId);
            }
            if self.requests.contains_key(id) {
                return Err(PolicyError::DuplicateRequest);
            }
        }
        if let Some(h) = context_hash {
            if !is_valid_context_hash(h) {
                return Err(PolicyError::InvalidContextHash);
            }
        }
        if !self.allowed_recipients.contains(recipient_id) {
            return Err(PolicyError::UnapprovedRecipient);
        }
        if amount == 0 {
            return Err(PolicyError::ZeroAmount);
        }
        if amount > self.max_per_payment {
            return Err(PolicyError::PerPaymentLimit);
        }
        let committed = self
            .spent_successful
            .checked_add(self.spent_pending)
            .and_then(|c| c.checked_add(amount))
            .ok_or(PolicyError::Overflow)?;
        if committed > self.total_budget {
            return Err(PolicyError::CumulativeLimit);
        }
        if amount > self.vault_available {
            return Err(PolicyError::InsufficientVault);
        }
        if self.request_count >= MAX_REQUESTS {
            return Err(PolicyError::RequestLimit);
        }
        Ok(())
    }

    /// After the payout of `amount` leaves the account, the remaining balance
    /// must still cover storage staking + the remaining vault + a buffer, so a
    /// payout can never eat into the operational reserve.
    fn assert_operating_reserve(&self, amount: u128) {
        let balance = env::account_balance().as_yoctonear();
        let staked = u128::from(env::storage_usage()) * env::storage_byte_cost().as_yoctonear();
        let required = staked
            .checked_add(self.vault_available)
            .and_then(|v| v.checked_add(MIN_OPERATING_BUFFER.as_yoctonear()))
            .and_then(|v| v.checked_add(amount))
            .unwrap_or_else(|| PolicyError::Overflow.panic());
        if balance < required {
            PolicyError::StorageReserveLow.panic();
        }
    }

    #[cfg(test)]
    fn assert_invariants(&self) {
        assert!(
            self.spent_successful + self.spent_pending <= self.total_budget,
            "INV-01"
        );
        assert_eq!(self.spent_pending, self.vault_pending, "INV-02 SP == P");
        assert_eq!(
            self.vault_available + self.vault_pending + self.spent_successful,
            self.total_funded,
            "INV-10 conservation"
        );
    }
}

#[cfg(test)]
mod tests;
