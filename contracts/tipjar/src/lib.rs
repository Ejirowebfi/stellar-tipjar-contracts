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
    NotDueYet = 6,
    SubscriptionNotActive = 7,
    SubscriptionNotFound = 8,
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
    pub fn tip(env: Env, sender: Address, creator: Address, amount: i128) {
        Self::require_not_paused(&env);
        if amount <= 0 {
            panic_with_error!(&env, TipJarError::InvalidAmount);
        }

        sender.require_auth();

        let token_client = token::Client::new(&env, &Self::read_token(&env));
        let contract_address = env.current_contract_address();

        token_client.transfer(&sender, &contract_address, &amount);

        let balance_key = DataKey::CreatorBalance(creator.clone());
        let total_key = DataKey::CreatorTotal(creator.clone());

        let next_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0) + amount;
        let next_total: i128 = env.storage().persistent().get(&total_key).unwrap_or(0) + amount;

        env.storage().persistent().set(&balance_key, &next_balance);
        env.storage().persistent().set(&total_key, &next_total);

        env.events().publish((symbol_short!("tip"), creator), (sender, amount));
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

        let token_client = token::Client::new(&env, &Self::read_token(&env));
        let contract_address = env.current_contract_address();

        token_client.transfer(&sender, &contract_address, &amount);

        let balance_key = DataKey::CreatorBalance(creator.clone());
        let total_key = DataKey::CreatorTotal(creator.clone());
        let msgs_key = DataKey::CreatorMessages(creator.clone());

        let next_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0) + amount;
        let next_total: i128 = env.storage().persistent().get(&total_key).unwrap_or(0) + amount;

        env.storage().persistent().set(&balance_key, &next_balance);
        env.storage().persistent().set(&total_key, &next_total);

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
            .get(&msgs_key)
            .unwrap_or_else(|| Vec::new(&env));
        messages.push_back(payload);
        env.storage().persistent().set(&msgs_key, &messages);

        env.events().publish(
            (symbol_short!("tip_msg"), creator),
            (sender, amount, message, metadata),
        );
    }

    /// Returns total historical tips for a creator.
    pub fn get_total_tips(env: Env, creator: Address) -> i128 {
        env.storage().persistent().get(&DataKey::CreatorTotal(creator)).unwrap_or(0)
    }

    /// Returns stored messages for a creator.
    pub fn get_messages(env: Env, creator: Address) -> Vec<TipWithMessage> {
        env.storage()
            .persistent()
            .get(&DataKey::CreatorMessages(creator))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns currently withdrawable escrowed tips for a creator.
    pub fn get_withdrawable_balance(env: Env, creator: Address) -> i128 {
        env.storage().persistent().get(&DataKey::CreatorBalance(creator)).unwrap_or(0)
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

        let token_client = token::Client::new(&env, &Self::read_token(&env));
        token_client.transfer(&env.current_contract_address(), &creator, &amount);
        env.storage().persistent().set(&key, &0i128);

        env.events().publish((symbol_short!("withdraw"), creator), amount);
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

        let idx_key = DataKey::SubscriberSubs(subscriber);
        let mut ids: Vec<u64> = env.storage().persistent().get(&idx_key).unwrap_or_else(|| Vec::new(&env));
        ids.push_back(next_id);
        env.storage().persistent().set(&idx_key, &ids);

        next_id
    }

    /// Processes a due subscription payment (pull-payment model).
    /// next_payment advances from the scheduled time to prevent drift on late processing.
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
        token_client.transfer(&sub.subscriber, &env.current_contract_address(), &sub.amount);

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

        // Advance from scheduled time, not now, to prevent drift.
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

    fn require_not_paused(env: &Env) {
        let is_paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);
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
        tipjar_client.withdraw(&creator);
        assert_eq!(token_client.balance(&creator), 400);
    }

    #[test]
    fn test_create_subscription_next_payment() {
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

        assert_eq!(tipjar_client.get_withdrawable_balance(&creator), 500);
        assert_eq!(token_client.balance(&contract_id), 500);

        // next_payment advances from scheduled time (drift prevention).
        let sub = tipjar_client.get_subscription(&sub_id);
        assert_eq!(sub.next_payment, 1_000 + interval + interval);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #6)")]
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
        tipjar_client.process_subscription_payment(&sub_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #7)")]
    fn test_cancelled_subscription_cannot_be_processed() {
        let (env, contract_id, token_id, _) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);
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
        tipjar_client.cancel_subscription(&sub_id, &subscriber);

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
    }

    #[test]
    fn test_pause_unpause() {
        let (env, contract_id, _token_id, admin) = setup();
        let tipjar_client = TipJarContractClient::new(&env, &contract_id);

        tipjar_client.pause(&admin);

        let sender = Address::generate(&env);
        let creator = Address::generate(&env);

        let result = tipjar_client.try_tip(&sender, &creator, &100);
        assert!(result.is_err());

        tipjar_client.unpause(&admin);

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
}
