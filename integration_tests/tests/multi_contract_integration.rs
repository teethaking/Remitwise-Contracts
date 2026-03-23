//! Multi-Contract Integration Tests for Admin Role Transfer
//! 
//! This module provides comprehensive regression tests for upgrade admin transfer logic
//! across all RemitWise contracts. Tests cover unauthorized transfer attempts, 
//! locked-state behaviors, and cross-contract security assumptions.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation},
    Address, Env, IntoVal, Symbol,
};

// Import all contract types
use bill_payments::{BillPayments, BillPaymentsClient};
use family_wallet::{FamilyWallet, FamilyWalletClient};
use insurance::{Insurance, InsuranceClient};
use remittance_split::{RemittanceSplit, RemittanceSplitClient};
use savings_goals::{SavingsGoals, SavingsGoalsClient};

/// Test environment setup with all contracts deployed
struct MultiContractTestEnv {
    env: Env,
    
    // Contract clients
    bill_payments: BillPaymentsClient<'static>,
    family_wallet: FamilyWalletClient<'static>,
    insurance: InsuranceClient<'static>,
    remittance_split: RemittanceSplitClient<'static>,
    savings_goals: SavingsGoalsClient<'static>,
    
    // Test addresses
    owner: Address,
    admin1: Address,
    admin2: Address,
    unauthorized_user: Address,
}

impl MultiContractTestEnv {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        
        // Generate test addresses
        let owner = Address::generate(&env);
        let admin1 = Address::generate(&env);
        let admin2 = Address::generate(&env);
        let unauthorized_user = Address::generate(&env);
        
        // Deploy all contracts
        let bill_payments_id = env.register_contract(None, BillPayments);
        let family_wallet_id = env.register_contract(None, FamilyWallet);
        let insurance_id = env.register_contract(None, Insurance);
        let remittance_split_id = env.register_contract(None, RemittanceSplit);
        let savings_goals_id = env.register_contract(None, SavingsGoals);
        
        // Create clients
        let bill_payments = BillPaymentsClient::new(&env, &bill_payments_id);
        let family_wallet = FamilyWalletClient::new(&env, &family_wallet_id);
        let insurance = InsuranceClient::new(&env, &insurance_id);
        let remittance_split = RemittanceSplitClient::new(&env, &remittance_split_id);
        let savings_goals = SavingsGoalsClient::new(&env, &savings_goals_id);
        
        // Initialize contracts where needed
        let initial_members = soroban_sdk::vec![&env, owner.clone()];
        family_wallet.init(&owner, &initial_members);
        
        // Initialize remittance split with basic config
        let token_address = Address::generate(&env);
        remittance_split.initialize_split(
            &owner,
            &token_address,
            &25, // savings_percentage
            &15, // bills_percentage  
            &10, // insurance_percentage
            &50, // spending_percentage
        );
        
        // Initialize savings goals
        savings_goals.init();
        
        Self {
            env,
            bill_payments,
            family_wallet,
            insurance,
            remittance_split,
            savings_goals,
            owner,
            admin1,
            admin2,
            unauthorized_user,
        }
    }
}

/// Test bootstrap admin setup across all contracts
#[test]
fn test_bootstrap_admin_setup_all_contracts() {
    let test_env = MultiContractTestEnv::new();
    
    // Test bootstrap pattern for contracts that support it
    
    // 1. Bill Payments - bootstrap pattern (caller == new_admin)
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    assert!(result.is_ok(), "Bill payments bootstrap should succeed");
    
    let current_admin = test_env.bill_payments.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin1.clone()));
    
    // 2. Insurance - bootstrap pattern (caller == new_admin)
    let result = test_env.insurance.try_set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    assert!(result.is_ok(), "Insurance bootstrap should succeed");
    
    let current_admin = test_env.insurance.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin1.clone()));
    
    // 3. Savings Goals - bootstrap pattern (caller == new_admin)
    test_env.savings_goals.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    
    let current_admin = test_env.savings_goals.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin1.clone()));
    
    // 4. Remittance Split - owner-based setup
    let result = test_env.remittance_split.try_set_upgrade_admin(&test_env.owner, &test_env.admin1);
    assert!(result.is_ok(), "Remittance split owner setup should succeed");
    
    let current_admin = test_env.remittance_split.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin1.clone()));
    
    // 5. Family Wallet - owner-based setup
    let result = test_env.family_wallet.set_upgrade_admin(&test_env.owner, &test_env.admin1);
    assert!(result, "Family wallet owner setup should succeed");
    
    let current_admin = test_env.family_wallet.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin1.clone()));
}

/// Test unauthorized bootstrap attempts across all contracts
#[test]
fn test_unauthorized_bootstrap_attempts() {
    let test_env = MultiContractTestEnv::new();
    
    // Test unauthorized bootstrap attempts (caller != new_admin for bootstrap contracts)
    
    // 1. Bill Payments - unauthorized bootstrap should fail
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.unauthorized_user, &test_env.admin1);
    assert!(result.is_err(), "Unauthorized bill payments bootstrap should fail");
    
    // 2. Insurance - unauthorized bootstrap should fail  
    let result = test_env.insurance.try_set_upgrade_admin(&test_env.unauthorized_user, &test_env.admin1);
    assert!(result.is_err(), "Unauthorized insurance bootstrap should fail");
    
    // 3. Remittance Split - non-owner setup should fail
    let result = test_env.remittance_split.try_set_upgrade_admin(&test_env.unauthorized_user, &test_env.admin1);
    assert!(result.is_err(), "Unauthorized remittance split setup should fail");
    
    // Verify no admin was set
    let admin = test_env.bill_payments.get_upgrade_admin_public();
    assert_eq!(admin, None, "No admin should be set after failed bootstrap");
    
    let admin = test_env.insurance.get_upgrade_admin_public();
    assert_eq!(admin, None, "No admin should be set after failed bootstrap");
    
    let admin = test_env.remittance_split.get_upgrade_admin_public();
    assert_eq!(admin, None, "No admin should be set after failed bootstrap");
}

/// Test admin transfer between authorized parties
#[test]
fn test_authorized_admin_transfer() {
    let mut test_env = MultiContractTestEnv::new();
    
    // Setup initial admins
    test_env.bill_payments.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.insurance.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.savings_goals.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.remittance_split.set_upgrade_admin(&test_env.owner, &test_env.admin1);
    test_env.family_wallet.set_upgrade_admin(&test_env.owner, &test_env.admin1);
    
    // Transfer admin from admin1 to admin2 across all contracts
    
    // 1. Bill Payments
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.admin1, &test_env.admin2);
    assert!(result.is_ok(), "Authorized bill payments transfer should succeed");
    
    let current_admin = test_env.bill_payments.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin2.clone()));
    
    // 2. Insurance
    let result = test_env.insurance.try_set_upgrade_admin(&test_env.admin1, &test_env.admin2);
    assert!(result.is_ok(), "Authorized insurance transfer should succeed");
    
    let current_admin = test_env.insurance.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin2.clone()));
    
    // 3. Savings Goals
    test_env.savings_goals.set_upgrade_admin(&test_env.admin1, &test_env.admin2);
    
    let current_admin = test_env.savings_goals.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin2.clone()));
    
    // 4. Remittance Split
    let result = test_env.remittance_split.try_set_upgrade_admin(&test_env.admin1, &test_env.admin2);
    assert!(result.is_ok(), "Authorized remittance split transfer should succeed");
    
    let current_admin = test_env.remittance_split.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin2.clone()));
    
    // 5. Family Wallet
    let result = test_env.family_wallet.set_upgrade_admin(&test_env.owner, &test_env.admin2);
    assert!(result, "Authorized family wallet transfer should succeed");
    
    let current_admin = test_env.family_wallet.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin2.clone()));
}

/// Test unauthorized admin transfer attempts
#[test]
fn test_unauthorized_admin_transfer() {
    let mut test_env = MultiContractTestEnv::new();
    
    // Setup initial admins
    test_env.bill_payments.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.insurance.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.savings_goals.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.remittance_split.set_upgrade_admin(&test_env.owner, &test_env.admin1);
    test_env.family_wallet.set_upgrade_admin(&test_env.owner, &test_env.admin1);
    
    // Attempt unauthorized transfers (non-admin trying to transfer)
    
    // 1. Bill Payments - unauthorized transfer should fail
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.unauthorized_user, &test_env.admin2);
    assert!(result.is_err(), "Unauthorized bill payments transfer should fail");
    
    // 2. Insurance - unauthorized transfer should fail
    let result = test_env.insurance.try_set_upgrade_admin(&test_env.unauthorized_user, &test_env.admin2);
    assert!(result.is_err(), "Unauthorized insurance transfer should fail");
    
    // 3. Remittance Split - unauthorized transfer should fail
    let result = test_env.remittance_split.try_set_upgrade_admin(&test_env.unauthorized_user, &test_env.admin2);
    assert!(result.is_err(), "Unauthorized remittance split transfer should fail");
    
    // Verify admin remains unchanged
    let admin = test_env.bill_payments.get_upgrade_admin_public();
    assert_eq!(admin, Some(test_env.admin1.clone()), "Admin should remain unchanged after failed transfer");
    
    let admin = test_env.insurance.get_upgrade_admin_public();
    assert_eq!(admin, Some(test_env.admin1.clone()), "Admin should remain unchanged after failed transfer");
    
    let admin = test_env.remittance_split.get_upgrade_admin_public();
    assert_eq!(admin, Some(test_env.admin1.clone()), "Admin should remain unchanged after failed transfer");
}

/// Test admin operations while contracts are paused (locked state)
#[test]
fn test_admin_operations_while_paused() {
    let mut test_env = MultiContractTestEnv::new();
    
    // Setup admins and pause admins
    test_env.bill_payments.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.bill_payments.set_pause_admin(&test_env.admin1, &test_env.admin1);
    
    test_env.insurance.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.insurance.set_pause_admin(&test_env.admin1, &test_env.admin1);
    
    test_env.savings_goals.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.savings_goals.set_pause_admin(&test_env.admin1, &test_env.admin1);
    
    test_env.remittance_split.set_upgrade_admin(&test_env.owner, &test_env.admin1);
    test_env.remittance_split.set_pause_admin(&test_env.owner, &test_env.admin1);
    
    // Pause all contracts
    let _ = test_env.bill_payments.try_pause(&test_env.admin1);
    let _ = test_env.insurance.try_pause(&test_env.admin1);
    test_env.savings_goals.pause(&test_env.admin1);
    let _ = test_env.remittance_split.try_pause(&test_env.admin1);
    
    // Verify contracts are paused
    assert!(test_env.bill_payments.is_paused(), "Bill payments should be paused");
    assert!(test_env.insurance.is_paused(), "Insurance should be paused");
    assert!(test_env.savings_goals.is_paused(), "Savings goals should be paused");
    assert!(test_env.remittance_split.is_paused(), "Remittance split should be paused");
    
    // Test that admin transfer still works while paused (admin functions should not be blocked)
    
    // 1. Bill Payments - admin transfer should work while paused
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.admin1, &test_env.admin2);
    assert!(result.is_ok(), "Admin transfer should work while contract is paused");
    
    let current_admin = test_env.bill_payments.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin2.clone()));
    
    // 2. Insurance - admin transfer should work while paused
    let result = test_env.insurance.try_set_upgrade_admin(&test_env.admin1, &test_env.admin2);
    assert!(result.is_ok(), "Admin transfer should work while contract is paused");
    
    let current_admin = test_env.insurance.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin2.clone()));
    
    // 3. Savings Goals - admin transfer should work while paused
    test_env.savings_goals.set_upgrade_admin(&test_env.admin1, &test_env.admin2);
    
    let current_admin = test_env.savings_goals.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin2.clone()));
    
    // 4. Remittance Split - admin transfer should work while paused
    let result = test_env.remittance_split.try_set_upgrade_admin(&test_env.admin1, &test_env.admin2);
    assert!(result.is_ok(), "Admin transfer should work while contract is paused");
    
    let current_admin = test_env.remittance_split.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin2.clone()));
}

/// Test version upgrade operations with proper admin authorization
#[test]
fn test_version_upgrade_authorization() {
    let mut test_env = MultiContractTestEnv::new();
    
    // Setup upgrade admins
    test_env.bill_payments.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.insurance.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.savings_goals.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.remittance_split.set_upgrade_admin(&test_env.owner, &test_env.admin1);
    
    // Test authorized version upgrades
    let new_version = 2u32;
    
    // 1. Bill Payments - authorized upgrade should succeed
    let result = test_env.bill_payments.try_set_version(&test_env.admin1, &new_version);
    assert!(result.is_ok(), "Authorized version upgrade should succeed");
    
    let current_version = test_env.bill_payments.get_version();
    assert_eq!(current_version, new_version);
    
    // 2. Insurance - authorized upgrade should succeed
    let result = test_env.insurance.try_set_version(&test_env.admin1, &new_version);
    assert!(result.is_ok(), "Authorized version upgrade should succeed");
    
    let current_version = test_env.insurance.get_version();
    assert_eq!(current_version, new_version);
    
    // 3. Savings Goals - authorized upgrade should succeed
    test_env.savings_goals.set_version(&test_env.admin1, &new_version);
    
    let current_version = test_env.savings_goals.get_version();
    assert_eq!(current_version, new_version);
    
    // 4. Remittance Split - authorized upgrade should succeed
    let result = test_env.remittance_split.try_set_version(&test_env.admin1, &new_version);
    assert!(result.is_ok(), "Authorized version upgrade should succeed");
    
    let current_version = test_env.remittance_split.get_version();
    assert_eq!(current_version, new_version);
    
    // Test unauthorized version upgrades
    let newer_version = 3u32;
    
    // 1. Bill Payments - unauthorized upgrade should fail
    let result = test_env.bill_payments.try_set_version(&test_env.unauthorized_user, &newer_version);
    assert!(result.is_err(), "Unauthorized version upgrade should fail");
    
    // 2. Insurance - unauthorized upgrade should fail
    let result = test_env.insurance.try_set_version(&test_env.unauthorized_user, &newer_version);
    assert!(result.is_err(), "Unauthorized version upgrade should fail");
    
    // 3. Remittance Split - unauthorized upgrade should fail
    let result = test_env.remittance_split.try_set_version(&test_env.unauthorized_user, &newer_version);
    assert!(result.is_err(), "Unauthorized version upgrade should fail");
    
    // Verify versions remain unchanged after failed upgrades
    let version = test_env.bill_payments.get_version();
    assert_eq!(version, new_version, "Version should remain unchanged after failed upgrade");
    
    let version = test_env.insurance.get_version();
    assert_eq!(version, new_version, "Version should remain unchanged after failed upgrade");
    
    let version = test_env.remittance_split.get_version();
    assert_eq!(version, new_version, "Version should remain unchanged after failed upgrade");
}

/// Test cross-contract admin consistency and isolation
#[test]
fn test_cross_contract_admin_isolation() {
    let mut test_env = MultiContractTestEnv::new();
    
    // Setup different admins for different contracts
    test_env.bill_payments.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.insurance.set_upgrade_admin(&test_env.admin2, &test_env.admin2);
    test_env.savings_goals.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    test_env.remittance_split.set_upgrade_admin(&test_env.owner, &test_env.admin2);
    
    // Verify admin isolation - admin1 cannot control admin2's contracts
    
    // admin1 should not be able to transfer admin2's contracts
    let result = test_env.insurance.try_set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    assert!(result.is_err(), "Admin1 should not control insurance contract");
    
    let result = test_env.remittance_split.try_set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    assert!(result.is_err(), "Admin1 should not control remittance split contract");
    
    // admin2 should not be able to transfer admin1's contracts
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.admin2, &test_env.admin2);
    assert!(result.is_err(), "Admin2 should not control bill payments contract");
    
    // Verify each admin can only control their assigned contracts
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.admin1, &test_env.owner);
    assert!(result.is_ok(), "Admin1 should control bill payments contract");
    
    let result = test_env.insurance.try_set_upgrade_admin(&test_env.admin2, &test_env.owner);
    assert!(result.is_ok(), "Admin2 should control insurance contract");
}

/// Test edge cases and error conditions
#[test]
fn test_admin_transfer_edge_cases() {
    let mut test_env = MultiContractTestEnv::new();
    
    // Test self-transfer (admin transferring to themselves)
    test_env.bill_payments.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    assert!(result.is_ok(), "Self-transfer should be allowed");
    
    let current_admin = test_env.bill_payments.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin1.clone()));
    
    // Test transfer to zero address (if supported by the platform)
    // Note: This test may need to be adjusted based on Soroban's address validation
    
    // Test rapid successive transfers
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.admin1, &test_env.admin2);
    assert!(result.is_ok(), "First transfer should succeed");
    
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.admin2, &test_env.admin1);
    assert!(result.is_ok(), "Immediate reverse transfer should succeed");
    
    let current_admin = test_env.bill_payments.get_upgrade_admin_public();
    assert_eq!(current_admin, Some(test_env.admin1.clone()));
}

/// Test admin transfer event emission
#[test]
fn test_admin_transfer_events() {
    let mut test_env = MultiContractTestEnv::new();
    
    // Setup initial admin
    test_env.bill_payments.set_upgrade_admin(&test_env.admin1, &test_env.admin1);
    
    // Clear any existing events
    test_env.env.events().all().clear();
    
    // Perform admin transfer
    let result = test_env.bill_payments.try_set_upgrade_admin(&test_env.admin1, &test_env.admin2);
    assert!(result.is_ok(), "Admin transfer should succeed");
    
    // Verify event was emitted
    let events = test_env.env.events().all();
    assert!(!events.is_empty(), "Admin transfer should emit events");
    
    // Look for admin transfer event
    let admin_transfer_events: Vec<_> = events
        .iter()
        .filter(|event| {
            if let Ok(topics) = event.topics.get(0) {
                if let Ok(contract_symbol) = Symbol::try_from_val(&test_env.env, &topics) {
                    return contract_symbol == Symbol::new(&test_env.env, "adm_xfr");
                }
            }
            false
        })
        .collect();
    
    assert!(!admin_transfer_events.is_empty(), "Should emit admin transfer event");
}