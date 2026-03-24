#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    Address, Env, Map, String, Vec,
};

#[cfg(test)]
extern crate std;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TipWithMessage {
    pub sender: Address,
    pub creator: Address,
    pub amount: i128,
    pub message: String,
    pub metadata: Map<String, String>,
    pub timestamp: u64,
}

/// Storage layout for persistent contract data.
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    /// Token contract address used for all tips.
    Token,
    /// Creator's currently withdrawable balance held by this contract.
    CreatorBalance(Address),
    /// Historical total tips ever received by creator.
    CreatorTotal(Address),
    /// Emergency pause state (bool).
    Paused,
    /// Contract administrator (Address).
    Admin,
    /// Messages appended for a creator.
    CreatorMessages(Address),
    /// Platform fee in basis points (10_000 == 100%). Max 1_000 (10%).
    PlatformFee,
    /// Address that receives collected platform fees.
    TreasuryAddress,
    /// Cumulative fees collected by the platform.
    TotalFeesCollected,
    /// Auto-incrementing counter for subscription IDs.
    SubCounter,
    /// Subscription record by ID.
    Subscription(u64),
    /// List of subscription IDs created by a subscriber.
    SubscriberSubs(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TipJarError {
    AlreadyInitialized = 1,
    TokenNotInitialized = 2,
    InvalidAmount = 3,
    NothingToWithdraw = 4,
    MessageTooLong = 5,
    InvalidSplitTotal = 6,
    FeeTooHigh = 7,
    NotDueYet = 8,
    SubscriptionNotActive = 9,
    SubscriptionNotFound = 10,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TipSplit {
    pub creator: Address,
    /// Basis points: 10_000 == 100.00%
    pub percentage: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subscription {
    pub subscriber: Address,
    pub creator: Address,
    pub amount: i128,
    pub interval: u64,
    pub next_payment: u64,
    pub active: bool,
}

#[contract]
pub struct TipJarContract;

#[contractimpl]
impl TipJarContract {
    /// One-time setup to choose the token contract and administrator for the TipJar.
    pub fn init(env: Env, token: Address, admin: Address) {
        if env.storage().instance().has(&DataKey::Token) {
            panic_with_error!(&env, TipJarError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
    }

    /// Moves `amount` tokens from `sender` into contract escrow for `creator`.
    ///
    /// If a platform fee is configured, the fee is sent directly to the treasury
    /// and only the remainder is escrowed for the creator.
    pub fn tip(env: Env, sender: Address, creator: Address, amount: i128) {
        Self::require_not_paused(&env);
        if amount <= 0 {
            panic_with_error!(&env, TipJarError::InvalidAmount);
        }

        sender.require_auth();

        let token_id = Self::read_token(&env);
        let token_client = token::Client::new(&env, &token_id);
        let contract_address = env.current_contract_address();

        let fee_bps: u32 = env.storage().instance().get(&DataKey::PlatformFee).unwrap_or(0);
        let fee = Self::calc_fee(amount, fee_bps);
        let creator_amount = amount - fee;

        // Fee goes directly to treasury; creator portion goes into escrow.
        if fee > 0 {
            let treasury: Address = env.storage().instance().get(&DataKey::TreasuryAddress).unwrap();
            token_client.transfer(&sender, &treasury, &fee);

            let total_fees: i128 = env.storage().instance().get(&DataKey::TotalFeesCollected).unwrap_or(0);
            env.storage().instance().set(&DataKey::TotalFeesCollected, &(total_fees + fee));

            env.events()
                .publish((symbol_short!("fee_coll"), treasury), (sender.clone(), fee));
        }

        token_client.transfer(&sender, &contract_address, &creator_amount);

        let creator_balance_key = DataKey::CreatorBalance(creator.clone());
        let creator_total_key = DataKey::CreatorTotal(creator.clone());

        let next_balance: i128 = env.storage().persistent().get(&creator_balance_key).unwrap_or(0) + creator_amount;
        let next_total: i128 = env.storage().persistent().get(&creator_total_key).unwrap_or(0) + creator_amount;

        env.storage().persistent().set(&creator_balance_key, &next_balance);
        env.storage().persistent().set(&creator_total_key, &next_total);

        env.events()
            .publish((symbol_short!("tip"), creator), (sender, creator_amount));
    }

    /// Allows supporters to attach a note and metadata to a tip.
    pub fn tip_with_message(
        env: Env,
        sender: Address,
        creator: Address,
        amount: i128,
        message: String,
        metadata: Map<String, String>,
    ) {
        Self::require_not_paused(&env);
        if amount <= 0 {
            panic_with_error!(&env, TipJarError::InvalidAmount);
        }
        if message.len() > 280 {
            panic_with_error!(&env, TipJarError::MessageTooLong);
        }

        sender.require_auth();

        let token_id = Self::read_token(&env);
        let token_client = token::Client::new(&env, &token_id);
        let contract_address = env.current_contract_address();

        // Transfer tokens into contract escrow first so creators can withdraw later.
        token_client.transfer(&sender, &contract_address, &amount);

        let creator_balance_key = DataKey::CreatorBalance(creator.clone());
        let creator_total_key = DataKey::CreatorTotal(creator.clone());
        let creator_msgs_key = DataKey::CreatorMessages(creator.clone());

        let current_balance: i128 = env
            .storage()
            .persistent()
            .get(&creator_balance_key)
            .unwrap_or(0);
        let current_total: i128 = env
            .storage()
            .persistent()
            .get(&creator_total_key)
            .unwrap_or(0);

        let next_balance = current_balance + amount;
        let next_total = current_total + amount;

        env.storage()
            .persistent()
            .set(&creator_balance_key, &next_balance);
        env.storage()
            .persistent()
            .set(&creator_total_key, &next_total);

        // Store message
        let timestamp = env.ledger().timestamp();
        let payload = TipWithMessage {
            sender: sender.clone(),
            creator: creator.clone(),
            amount,
            message: message.clone(),
            metadata: metadata.clone(),
            timestamp,
        };
        let mut messages: Vec<TipWithMessage> = env
            .storage()
            .persistent()
            .get(&creator_msgs_key)
            .unwrap_or_else(|| Vec::new(&env));
        messages.push_back(payload);
        env.storage().persistent().set(&creator_msgs_key, &messages);

        // Emit message payload
        env.events().publish(
            (symbol_short!("tip_msg"), creator),
            (sender, amount, message, metadata),
        );
    }

    /// Returns total historical tips for a creator.
    pub fn get_total_tips(env: Env, creator: Address) -> i128 {
        let key = DataKey::CreatorTotal(creator);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Returns stored messages for a creator.
    pub fn get_messages(env: Env, creator: Address) -> Vec<TipWithMessage> {
        let key = DataKey::CreatorMessages(creator);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns currently withdrawable escrowed tips for a creator.
    pub fn get_withdrawable_balance(env: Env, creator: Address) -> i128 {
        let key = DataKey::CreatorBalance(creator);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Allows creator to withdraw their accumulated escrowed tips.
    pub fn withdraw(env: Env, creator: Address) {
        Self::require_not_paused(&env);
        creator.require_auth();

        let key = DataKey::CreatorBalance(creator.clone());
        let amount: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount <= 0 {
            panic_with_error!(&env, TipJarError::NothingToWithdraw);
        }

        let token_id = Self::read_token(&env);
        let token_client = token::Client::new(&env, &token_id);
        let contract_address = env.current_contract_address();

        token_client.transfer(&contract_address, &creator, &amount);
        env.storage().persistent().set(&key, &0i128);

        env.events()
            .publish((symbol_short!("withdraw"), creator), amount);
    }

    /// Distributes `total_amount` tokens from `sender` across multiple creators
    /// according to basis-point percentages (10_000 == 100%). Any rounding dust
    /// is added to the last creator.
    pub fn tip_split(env: Env, sender: Address, splits: Vec<TipSplit>, total_amount: i128) {
        Self::require_not_paused(&env);

        if splits.is_empty() || total_amount <= 0 {
            panic_with_error!(&env, TipJarError::InvalidAmount);
        }

        // Validate percentages sum to exactly 10_000.
        let mut pct_sum: u32 = 0;
        for i in 0..splits.len() {
            pct_sum += splits.get(i).unwrap().percentage;
        }
        if pct_sum != 10_000 {
            panic_with_error!(&env, TipJarError::InvalidSplitTotal);
        }

        sender.require_auth();

        let token_id = Self::read_token(&env);
        let token_client = token::Client::new(&env, &token_id);
        let contract_address = env.current_contract_address();

        // Single transfer of the full amount into escrow.
        token_client.transfer(&sender, &contract_address, &total_amount);

        let last_idx = splits.len() - 1;
        let mut distributed: i128 = 0;

        for i in 0..splits.len() {
            let split = splits.get(i).unwrap();
            let amount = if i == last_idx {
                // Assign remaining dust to last creator.
                total_amount - distributed
            } else {
                total_amount * (split.percentage as i128) / 10_000
            };
            distributed += amount;

            let balance_key = DataKey::CreatorBalance(split.creator.clone());
            let total_key = DataKey::CreatorTotal(split.creator.clone());

            let new_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0) + amount;
            let new_total: i128 = env.storage().persistent().get(&total_key).unwrap_or(0) + amount;

            env.storage().persistent().set(&balance_key, &new_balance);
            env.storage().persistent().set(&total_key, &new_total);

            env.events()
                .publish((symbol_short!("tip"), split.creator), (sender.clone(), amount));
        }
    }

    /// Creates a recurring subscription from `subscriber` to `creator`.
    /// Returns the new subscription ID.
    pub fn create_subscription(
        env: Env,
        subscriber: Address,
        creator: Address,
        amount: i128,
        interval: u64,
    ) -> u64 {
        Self::require_not_paused(&env);
        if amount <= 0 {
            panic_with_error!(&env, TipJarError::InvalidAmount);
        }
        subscriber.require_auth();

        let sub_id: u64 = env.storage().instance().get(&DataKey::SubCounter).unwrap_or(0);
        let next_id = sub_id + 1;
        env.storage().instance().set(&DataKey::SubCounter, &next_id);

        let sub = Subscription {
            subscriber: subscriber.clone(),
            creator,
            amount,
            interval,
            next_payment: env.ledger().timestamp() + interval,
            active: true,
        };
        env.storage().persistent().set(&DataKey::Subscription(next_id), &sub);

        // Track this subscription ID under the subscriber's index.
        let idx_key = DataKey::SubscriberSubs(subscriber);
        let mut ids: Vec<u64> = env.storage().persistent().get(&idx_key).unwrap_or_else(|| Vec::new(&env));
        ids.push_back(next_id);
        env.storage().persistent().set(&idx_key, &ids);

        next_id
    }

    /// Processes a due subscription payment. Can be called by anyone; the subscriber
    /// must authorize the transaction (pull-payment model).
    /// `next_payment` advances by `interval` from the scheduled time, not from now,
    /// preventing schedule drift on late processing.
    pub fn process_subscription_payment(env: Env, sub_id: u64) {
        Self::require_not_paused(&env);

        let mut sub: Subscription = env
            .storage()
            .persistent()
            .get(&DataKey::Subscription(sub_id))
            .unwrap_or_else(|| panic_with_error!(&env, TipJarError::SubscriptionNotFound));

        if !sub.active {
            panic_with_error!(&env, TipJarError::SubscriptionNotActive);
        }
        if env.ledger().timestamp() < sub.next_payment {
            panic_with_error!(&env, TipJarError::NotDueYet);
        }

        sub.subscriber.require_auth();

        let token_client = token::Client::new(&env, &Self::read_token(&env));
        let contract_address = env.current_contract_address();

        token_client.transfer(&sub.subscriber, &contract_address, &sub.amount);

        let balance_key = DataKey::CreatorBalance(sub.creator.clone());
        let total_key = DataKey::CreatorTotal(sub.creator.clone());
        let new_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0) + sub.amount;
        let new_total: i128 = env.storage().persistent().get(&total_key).unwrap_or(0) + sub.amount;
        env.storage().persistent().set(&balance_key, &new_balance);
        env.storage().persistent().set(&total_key, &new_total);

        env.events().publish(
            (symbol_short!("tip"), sub.creator.clone()),
            (sub.subscriber.clone(), sub.amount),
        );

        // Advance schedule from intended time, not from now, to prevent drift.
        sub.next_payment += sub.interval;
        env.storage().persistent().set(&DataKey::Subscription(sub_id), &sub);
    }

    /// Cancels a subscription. Either the subscriber or the creator may cancel.
    pub fn cancel_subscription(env: Env, sub_id: u64, caller: Address) {
        let mut sub: Subscription = env
            .storage()
            .persistent()
            .get(&DataKey::Subscription(sub_id))
            .unwrap_or_else(|| panic_with_error!(&env, TipJarError::SubscriptionNotFound));

        if !sub.active {
            panic_with_error!(&env, TipJarError::SubscriptionNotActive);
        }

        // Caller must be subscriber or creator, and must authorize.
        if caller != sub.subscriber && caller != sub.creator {
            panic!("Unauthorized");
        }
        caller.require_auth();

        sub.active = false;
        env.storage().persistent().set(&DataKey::Subscription(sub_id), &sub);
    }

    /// Returns a subscription record by ID.
    pub fn get_subscription(env: Env, sub_id: u64) -> Subscription {
        env.storage()
            .persistent()
            .get(&DataKey::Subscription(sub_id))
            .unwrap_or_else(|| panic_with_error!(&env, TipJarError::SubscriptionNotFound))
    }

    /// Sets the platform fee (in basis points) and treasury address. Admin only.
    pub fn set_platform_fee(env: Env, admin: Address, fee_bps: u32, treasury: Address) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Unauthorized");
        }
        if fee_bps > 1_000 {
            panic_with_error!(&env, TipJarError::FeeTooHigh);
        }
        env.storage().instance().set(&DataKey::PlatformFee, &fee_bps);
        env.storage().instance().set(&DataKey::TreasuryAddress, &treasury);
    }

    /// Returns the total fees collected by the platform.
    pub fn get_total_fees_collected(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::TotalFeesCollected).unwrap_or(0)
    }

    /// Calculates the platform fee for a given amount and fee in basis points.
    fn calc_fee(amount: i128, fee_bps: u32) -> i128 {
        amount * (fee_bps as i128) / 10_000
    }

    fn read_token(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Token)
            .unwrap_or_else(|| panic_with_error!(env, TipJarError::TokenNotInitialized))
    }

    /// Emergency pause to stop all state-changing activities (Admin only).
    pub fn pause(env: Env, admin: Address) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Unauthorized");
        }
        env.storage().instance().set(&DataKey::Paused, &true);
    }

    /// Resume contract activities after an emergency pause (Admin only).
    pub fn unpause(env: Env, admin: Address) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Unauthorized");
        }
        env.storage().instance().set(&DataKey::Paused, &false);
    }

    /// Internal helper to check if the contract is paused.
    fn require_not_paused(env: &Env) {
        let is_paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if is_paused {
            panic!("Contract is paused");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, token, Address, Env};

    fn setup() -> (Env, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let token_admin = Address::generate(&env);
        let token_id = env
            .register_stellar_asset_contract_v2(token_admin.clone())
            .address();

        let admin = Address::generate(&env);
        let contract_id = env.register(TipJarContract, ());
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        tipjar_client.init(&token_id, &admin);

        (env, contract_id, token_id, admin)
    }

    #[test]
    fn test_tipping_functionality() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_client = token::Client::new(&env, &token_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender = Address::generate(&env);
        let creator = Address::generate(&env);

        token_admin_client.mint(&sender, &1_000);
        tipjar_client.tip(&sender, &creator, &250);

        assert_eq!(token_client.balance(&sender), 750);
        assert_eq!(token_client.balance(&contract_id), 250);
        assert_eq!(tipjar_client.get_total_tips(&creator), 250);
    }

    #[test]
    fn test_tipping_with_message_functionality() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_client = token::Client::new(&env, &token_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender = Address::generate(&env);
        let creator = Address::generate(&env);

        let message = soroban_sdk::String::from_str(&env, "Great job!");
        let metadata = soroban_sdk::Map::new(&env);

        token_admin_client.mint(&sender, &1_000);
        tipjar_client.tip_with_message(&sender, &creator, &250, &message, &metadata);

        assert_eq!(token_client.balance(&sender), 750);
        assert_eq!(token_client.balance(&contract_id), 250);
        assert_eq!(tipjar_client.get_total_tips(&creator), 250);

        let msgs = tipjar_client.get_messages(&creator);
        assert_eq!(msgs.len(), 1);
        let msg = msgs.get(0).unwrap();
        assert_eq!(msg.message, message);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_tipping_message_too_long() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender = Address::generate(&env);
        let creator = Address::generate(&env);

        let long_str = "x".repeat(281);
        let message = soroban_sdk::String::from_str(&env, &long_str);
        let metadata = soroban_sdk::Map::new(&env);

        token_admin_client.mint(&sender, &1_000);
        tipjar_client.tip_with_message(&sender, &creator, &250, &message, &metadata);
    }

    #[test]
    fn test_balance_tracking_and_withdraw() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_client = token::Client::new(&env, &token_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender_a = Address::generate(&env);
        let sender_b = Address::generate(&env);
        let creator = Address::generate(&env);

        token_admin_client.mint(&sender_a, &1_000);
        token_admin_client.mint(&sender_b, &1_000);

        tipjar_client.tip(&sender_a, &creator, &100);
        tipjar_client.tip(&sender_b, &creator, &300);

        assert_eq!(tipjar_client.get_total_tips(&creator), 400);
        assert_eq!(tipjar_client.get_withdrawable_balance(&creator), 400);
        assert_eq!(token_client.balance(&contract_id), 400);

        tipjar_client.withdraw(&creator);

        assert_eq!(tipjar_client.get_withdrawable_balance(&creator), 0);
        assert_eq!(token_client.balance(&creator), 400);
        assert_eq!(token_client.balance(&contract_id), 0);
    }

    #[test]
    #[should_panic]
    fn test_invalid_tip_amount() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender = Address::generate(&env);
        let creator = Address::generate(&env);

        token_admin_client.mint(&sender, &100);

        // Zero tips are rejected to prevent accidental or abusive calls.
        tipjar_client.tip(&sender, &creator, &0);
    }

    #[test]
    fn test_pause_unpause() {
        let (env, contract_id, _token_id, admin) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);

        tipjar_client.pause(&admin);

        let sender = Address::generate(&env);
        let creator = Address::generate(&env);

        // This should fail
        let result = tipjar_client.try_tip(&sender, &creator, &100);
        assert!(result.is_err());

        // Unpause
        tipjar_client.unpause(&admin);

        // This should now succeed (once we mint tokens)
        let token_admin_client = token::StellarAssetClient::new(&env, &_token_id);
        token_admin_client.mint(&sender, &100);
        tipjar_client.tip(&sender, &creator, &100);
        assert_eq!(tipjar_client.get_total_tips(&creator), 100);
    }

    #[test]
    #[should_panic(expected = "Unauthorized")]
    fn test_pause_admin_only() {
        let (env, contract_id, _, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let non_admin = Address::generate(&env);

        tipjar_client.pause(&non_admin);
    }

    #[test]
    fn test_withdraw_blocked_when_paused() {
        let (env, contract_id, token_id, admin) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender = Address::generate(&env);
        let creator = Address::generate(&env);

        token_admin_client.mint(&sender, &100);
        tipjar_client.tip(&sender, &creator, &100);

        tipjar_client.pause(&admin);

        let result = tipjar_client.try_withdraw(&creator);
        assert!(result.is_err());
    }

    #[test]
    fn test_tip_split_50_50() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender = Address::generate(&env);
        let creator_a = Address::generate(&env);
        let creator_b = Address::generate(&env);

        token_admin_client.mint(&sender, &1_000);

        let mut splits = soroban_sdk::Vec::new(&env);
        splits.push_back(TipSplit { creator: creator_a.clone(), percentage: 5_000 });
        splits.push_back(TipSplit { creator: creator_b.clone(), percentage: 5_000 });

        tipjar_client.tip_split(&sender, &splits, &200);

        assert_eq!(tipjar_client.get_withdrawable_balance(&creator_a), 100);
        assert_eq!(tipjar_client.get_withdrawable_balance(&creator_b), 100);
        assert_eq!(tipjar_client.get_total_tips(&creator_a), 100);
        assert_eq!(tipjar_client.get_total_tips(&creator_b), 100);
    }

    #[test]
    fn test_tip_split_dust_handling() {
        // 3-way equal split of 100 stroops: 33 + 33 + 34 (dust to last)
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender = Address::generate(&env);
        let creator_a = Address::generate(&env);
        let creator_b = Address::generate(&env);
        let creator_c = Address::generate(&env);

        token_admin_client.mint(&sender, &1_000);

        let mut splits = soroban_sdk::Vec::new(&env);
        splits.push_back(TipSplit { creator: creator_a.clone(), percentage: 3_333 });
        splits.push_back(TipSplit { creator: creator_b.clone(), percentage: 3_333 });
        splits.push_back(TipSplit { creator: creator_c.clone(), percentage: 3_334 });

        tipjar_client.tip_split(&sender, &splits, &100);

        let a = tipjar_client.get_withdrawable_balance(&creator_a);
        let b = tipjar_client.get_withdrawable_balance(&creator_b);
        let c = tipjar_client.get_withdrawable_balance(&creator_c);

        // Each gets floor(100 * 3333 / 10000) = 33; dust (1) goes to last creator.
        assert_eq!(a, 33);
        assert_eq!(b, 33);
        assert_eq!(c, 34);
        assert_eq!(a + b + c, 100);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #6)")]
    fn test_tip_split_invalid_percentage_total() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender = Address::generate(&env);
        let creator_a = Address::generate(&env);
        let creator_b = Address::generate(&env);

        token_admin_client.mint(&sender, &1_000);

        // Percentages sum to 9_999 — should revert with InvalidSplitTotal.
        let mut splits = soroban_sdk::Vec::new(&env);
        splits.push_back(TipSplit { creator: creator_a.clone(), percentage: 5_000 });
        splits.push_back(TipSplit { creator: creator_b.clone(), percentage: 4_999 });

        tipjar_client.tip_split(&sender, &splits, &200);
    }

    #[test]
    fn test_platform_fee_split() {
        // 2.5% fee (250 bps) on a 1000-stroop tip: fee=25, creator gets 975.
        let (env, contract_id, token_id, admin) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_client = token::Client::new(&env, &token_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender = Address::generate(&env);
        let creator = Address::generate(&env);
        let treasury = Address::generate(&env);

        tipjar_client.set_platform_fee(&admin, &250, &treasury);
        token_admin_client.mint(&sender, &1_000);
        tipjar_client.tip(&sender, &creator, &1_000);

        assert_eq!(token_client.balance(&treasury), 25);
        assert_eq!(tipjar_client.get_withdrawable_balance(&creator), 975);
        assert_eq!(tipjar_client.get_total_fees_collected(), 25);
    }

    #[test]
    #[should_panic(expected = "Unauthorized")]
    fn test_set_platform_fee_non_admin_reverts() {
        let (env, contract_id, _, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let non_admin = Address::generate(&env);
        let treasury = Address::generate(&env);

        tipjar_client.set_platform_fee(&non_admin, &100, &treasury);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #7)")]
    fn test_set_platform_fee_too_high_reverts() {
        let (env, contract_id, _, admin) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let treasury = Address::generate(&env);

        // 1_001 bps > 1_000 cap — should revert with FeeTooHigh.
        tipjar_client.set_platform_fee(&admin, &1_001, &treasury);
    }

    #[test]
    fn test_total_fees_collected_accumulates() {
        let (env, contract_id, token_id, admin) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let sender = Address::generate(&env);
        let creator = Address::generate(&env);
        let treasury = Address::generate(&env);

        // 10% fee (1000 bps).
        tipjar_client.set_platform_fee(&admin, &1_000, &treasury);
        token_admin_client.mint(&sender, &3_000);

        tipjar_client.tip(&sender, &creator, &1_000); // fee = 100
        tipjar_client.tip(&sender, &creator, &1_000); // fee = 100
        tipjar_client.tip(&sender, &creator, &1_000); // fee = 100

        assert_eq!(tipjar_client.get_total_fees_collected(), 300);
        assert_eq!(tipjar_client.get_withdrawable_balance(&creator), 2_700);
    }

    #[test]
    fn test_create_subscription_next_payment() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let subscriber = Address::generate(&env);
        let creator = Address::generate(&env);

        token_admin_client.mint(&subscriber, &10_000);

        // Set a known ledger timestamp.
        env.ledger().set(soroban_sdk::testutils::LedgerInfo {
            timestamp: 1_000,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3_110_400,
        });

        let interval: u64 = 2_592_000; // 30 days in seconds
        let sub_id = tipjar_client.create_subscription(&subscriber, &creator, &500, &interval);

        let sub = tipjar_client.get_subscription(&sub_id);
        assert_eq!(sub.next_payment, 1_000 + interval);
        assert!(sub.active);
        assert_eq!(sub.amount, 500);
    }

    #[test]
    fn test_process_subscription_payment_after_interval() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_client = token::Client::new(&env, &token_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let subscriber = Address::generate(&env);
        let creator = Address::generate(&env);

        token_admin_client.mint(&subscriber, &10_000);

        let interval: u64 = 2_592_000;

        env.ledger().set(soroban_sdk::testutils::LedgerInfo {
            timestamp: 1_000,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3_110_400,
        });

        let sub_id = tipjar_client.create_subscription(&subscriber, &creator, &500, &interval);

        // Advance time past the interval.
        env.ledger().set(soroban_sdk::testutils::LedgerInfo {
            timestamp: 1_000 + interval + 1,
            sequence_number: 2,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3_110_400,
        });

        tipjar_client.process_subscription_payment(&sub_id);

        // Creator's escrow should have received the payment.
        assert_eq!(tipjar_client.get_withdrawable_balance(&creator), 500);
        assert_eq!(token_client.balance(&contract_id), 500);

        // next_payment advances from scheduled time, not from now (drift prevention).
        let sub = tipjar_client.get_subscription(&sub_id);
        assert_eq!(sub.next_payment, 1_000 + interval + interval);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #8)")]
    fn test_process_subscription_payment_too_early_reverts() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let subscriber = Address::generate(&env);
        let creator = Address::generate(&env);

        token_admin_client.mint(&subscriber, &10_000);

        env.ledger().set(soroban_sdk::testutils::LedgerInfo {
            timestamp: 1_000,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3_110_400,
        });

        let sub_id = tipjar_client.create_subscription(&subscriber, &creator, &500, &2_592_000);

        // Do NOT advance time — should revert with NotDueYet.
        tipjar_client.process_subscription_payment(&sub_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_cancelled_subscription_cannot_be_processed() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        let subscriber = Address::generate(&env);
        let creator = Address::generate(&env);

        token_admin_client.mint(&subscriber, &10_000);

        env.ledger().set(soroban_sdk::testutils::LedgerInfo {
            timestamp: 1_000,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3_110_400,
        });

        let interval: u64 = 2_592_000;
        let sub_id = tipjar_client.create_subscription(&subscriber, &creator, &500, &interval);

        // Cancel as subscriber.
        tipjar_client.cancel_subscription(&sub_id, &subscriber);

        // Advance time past due date.
        env.ledger().set(soroban_sdk::testutils::LedgerInfo {
            timestamp: 1_000 + interval + 1,
            sequence_number: 2,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3_110_400,
        });

        // Should revert with SubscriptionNotActive.
        tipjar_client.process_subscription_payment(&sub_id);
    }
}
