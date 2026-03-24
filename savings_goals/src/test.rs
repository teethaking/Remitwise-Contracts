#![cfg(test)]

use super::*;
use soroban_sdk::testutils::storage::Instance as _;
use soroban_sdk::{
    testutils::{Address as AddressTrait, Events, Ledger, LedgerInfo},
    Address, Env, String, Symbol, TryFromVal,
};

use testutils::{set_ledger_time, setup_test_env};

// Removed local set_time in favor of testutils::set_ledger_time

#[test]
fn test_create_goal_unique_ids_succeeds() {
    setup_test_env!(env, SavingsGoalContract, client, user);
    client.init();

    let name1 = String::from_str(&env, "Goal 1");
    let name2 = String::from_str(&env, "Goal 2");

    let id1 = client.create_goal(&user, &name1, &1000, &1735689600);
    let id2 = client.create_goal(&user, &name2, &2000, &1735689600);

    assert_ne!(id1, id2);
}

/// Documented behavior: past target dates are allowed (e.g. for backfill or
/// data migration). This test locks in that create_goal accepts a target_date
/// earlier than the current ledger timestamp and persists it as provided.
#[test]
fn test_create_goal_allows_past_target_date() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();

    // Move ledger time forward so our target_date is clearly in the past.
    set_time(&env, 2_000_000_000);
    let past_target_date = 1_000_000_000u64;

    let name = String::from_str(&env, "Backfill Goal");
    let id = client.create_goal(&user, &name, &1000, &past_target_date);

    assert_eq!(id, 1);
    let goal = client.get_goal(&id).unwrap();
    assert_eq!(goal.target_date, past_target_date);
}

// ============================================================================
// init() idempotency and NEXT_ID behavior
//
// init() bootstraps storage (NEXT_ID and GOALS) only when keys are missing.
// In production or integration, init() may be called more than once (e.g. by
// different entrypoints or upgrade paths). These tests lock in that:
// - A second init() must not remove or alter existing goals.
// - NEXT_ID must not be reset by a second init(); the next created goal must
//   receive the expected incremented ID (no reuse, no gaps).
// ============================================================================

/// Double init() must not remove or alter existing goals; next created goal
/// must get the next ID (e.g. 2), not 1.
#[test]
fn test_init_idempotent_does_not_wipe_goals() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner_a = Address::generate(&env);

    // First init on a fresh contract
    client.init();

    let name1 = String::from_str(&env, "First Goal");
    let target1 = 5000i128;
    let target_date1 = 2000000000u64;

    let goal_id_1 = client.create_goal(&owner_a, &name1, &target1, &target_date1);
    assert_eq!(goal_id_1, 1, "first goal must receive goal_id == 1");

    // Simulate a second initialization attempt (e.g. from another entrypoint or upgrade)
    client.init();

    // Verify the existing goal is still present with same name, owner, amounts
    let goal_after_second_init = client
        .get_goal(&1)
        .expect("goal 1 must still exist after second init()");
    assert_eq!(goal_after_second_init.name, name1);
    assert_eq!(goal_after_second_init.owner, owner_a);
    assert_eq!(goal_after_second_init.target_amount, target1);
    assert_eq!(goal_after_second_init.current_amount, 0);

    let all_goals = client.get_all_goals(&owner_a);
    assert_eq!(all_goals.len(), 1, "get_all_goals must still return the one goal");

    // Verify NEXT_ID was not reset: next created goal must get goal_id == 2, not 1
    let name2 = String::from_str(&env, "Second Goal");
    let goal_id_2 = client.create_goal(&owner_a, &name2, &10000i128, &target_date1);
    assert_eq!(
        goal_id_2, 2,
        "after second init(), next goal must get goal_id == 2, not 1 (NEXT_ID must not be reset)"
    );
}

/// After init(), creating goals sequentially must yield IDs 1, 2, 3, ... with
/// no gaps or reuse.
#[test]
fn test_next_id_increments_sequentially() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    client.init();

    let ids = [
        client.create_goal(
            &owner,
            &String::from_str(&env, "G1"),
            &1000i128,
            &2000000000u64,
        ),
        client.create_goal(
            &owner,
            &String::from_str(&env, "G2"),
            &2000i128,
            &2000000000u64,
        ),
        client.create_goal(
            &owner,
            &String::from_str(&env, "G3"),
            &3000i128,
            &2000000000u64,
        ),
    ];

    assert_eq!(ids[0], 1, "first goal id must be 1");
    assert_eq!(ids[1], 2, "second goal id must be 2");
    assert_eq!(ids[2], 3, "third goal id must be 3");

    for (i, &id) in ids.iter().enumerate() {
        let goal = client.get_goal(&id).unwrap();
        assert_eq!(goal.id, id);
        let expected_name = String::from_str(&env, &format!("G{}", i + 1));
        assert_eq!(goal.name, expected_name);
    }
}

#[test]
fn test_add_to_goal_increments() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();

    env.mock_all_auths();
    let id = client.create_goal(&user, &String::from_str(&env, "Save"), &1000, &2000000000);

    let new_balance = client.add_to_goal(&user, &id, &500);
    assert_eq!(new_balance, 500);
}

#[test]
fn test_add_to_non_existent_goal() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let res = client.try_add_to_goal(&user, &99, &500);
    assert!(res.is_err());
}

#[test]
fn test_get_goal_retrieval() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let name = String::from_str(&env, "Car");
    let id = client.create_goal(&user, &name, &5000, &2000000000);

    let goal = client.get_goal(&id).unwrap();
    assert_eq!(goal.name, name);
}

#[test]
fn test_get_all_goals() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    client.create_goal(&user, &String::from_str(&env, "A"), &100, &2000000000);
    client.create_goal(&user, &String::from_str(&env, "B"), &200, &2000000000);

    let all_goals = client.get_all_goals(&user);
    assert_eq!(all_goals.len(), 2);
}

#[test]
fn test_is_goal_completed() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();

    // 1. Create a goal with a target of 1000
    let target = 1000;
    let name = String::from_str(&env, "Trip");
    let id = client.create_goal(&user, &name, &target, &2000000000);

    // 2. It should NOT be completed initially (balance is 0)
    assert!(
        !client.is_goal_completed(&id),
        "Goal should not be complete at start"
    );

    // 3. Add exactly the target amount
    client.add_to_goal(&user, &id, &target);

    // 4. Verify the balance actually updated in storage
    let goal = client.get_goal(&id).unwrap();
    assert_eq!(
        goal.current_amount, target,
        "The amount was not saved correctly"
    );

    // 5. This will now pass once you fix the .instance() vs .persistent() mismatch in lib.rs
    assert!(
        client.is_goal_completed(&id),
        "Goal should be completed when current == target"
    );

    // 6. Bonus: Check that it stays completed if we go over the target
    client.add_to_goal(&user, &id, &1);
    assert!(
        client.is_goal_completed(&id),
        "Goal should stay completed if overfunded"
    );
}

#[test]
fn test_edge_cases_large_amounts() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(
        &user,
        &String::from_str(&env, "Max"),
        &i128::MAX,
        &2000000000,
    );

    client.add_to_goal(&user, &id, &(i128::MAX - 100));
    let goal = client.get_goal(&id).unwrap();
    assert_eq!(goal.current_amount, i128::MAX - 100);
}

#[test]
fn test_zero_amount_fails() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let res = client.try_create_goal(&user, &String::from_str(&env, "Fail"), &0, &2000000000);
    assert!(res.is_err());
}

#[test]
fn test_multiple_goals_management() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id1 = client.create_goal(&user, &String::from_str(&env, "G1"), &1000, &2000000000);
    let id2 = client.create_goal(&user, &String::from_str(&env, "G2"), &2000, &2000000000);

    client.add_to_goal(&user, &id1, &500);
    client.add_to_goal(&user, &id2, &1500);

    let g1 = client.get_goal(&id1).unwrap();
    let g2 = client.get_goal(&id2).unwrap();

    assert_eq!(g1.current_amount, 500);
    assert_eq!(g2.current_amount, 1500);
}

#[test]
fn test_withdraw_from_goal_success() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(
        &user,
        &String::from_str(&env, "Success"),
        &1000,
        &2000000000,
    );

    client.unlock_goal(&user, &id);
    client.add_to_goal(&user, &id, &500);

    let new_balance = client.withdraw_from_goal(&user, &id, &200);
    assert_eq!(new_balance, 300);

    let goal = client.get_goal(&id).unwrap();
    assert_eq!(goal.current_amount, 300);
}

#[test]
fn test_withdraw_from_goal_insufficient_balance() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(
        &user,
        &String::from_str(&env, "Insufficient"),
        &1000,
        &2000000000,
    );

    client.unlock_goal(&user, &id);
    client.add_to_goal(&user, &id, &100);

    let res = client.try_withdraw_from_goal(&user, &id, &200);
    assert!(res.is_err());
}

#[test]
fn test_withdraw_from_goal_locked() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(&user, &String::from_str(&env, "Locked"), &1000, &2000000000);

    client.add_to_goal(&user, &id, &500);
    let res = client.try_withdraw_from_goal(&user, &id, &100);
    assert!(res.is_err());
}

#[test]
fn test_withdraw_from_goal_unauthorized() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let other = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(
        &user,
        &String::from_str(&env, "Unauthorized"),
        &1000,
        &2000000000,
    );

    client.unlock_goal(&user, &id);
    client.add_to_goal(&user, &id, &500);

    let res = client.try_withdraw_from_goal(&other, &id, &100);
    assert!(res.is_err());
}

#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_withdraw_from_goal_zero_amount_panics() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(&user, &String::from_str(&env, "Zero"), &1000, &2000000000);

    client.unlock_goal(&user, &id);
    client.add_to_goal(&user, &id, &500);
    client.withdraw_from_goal(&user, &id, &0);
}

#[test]
#[should_panic(expected = "Goal not found")]
fn test_withdraw_from_goal_nonexistent_goal_panics() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    client.withdraw_from_goal(&user, &999, &100);
}

#[test]
fn test_lock_unlock_goal() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(&user, &String::from_str(&env, "Lock"), &1000, &2000000000);

    let goal = client.get_goal(&id).unwrap();
    assert!(goal.locked);

    client.unlock_goal(&user, &id);
    let goal = client.get_goal(&id).unwrap();
    assert!(!goal.locked);

    client.lock_goal(&user, &id);
    let goal = client.get_goal(&id).unwrap();
    assert!(goal.locked);
}

#[test]
fn test_withdraw_full_balance() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(&user, &String::from_str(&env, "Full"), &1000, &2000000000);

    client.unlock_goal(&user, &id);
    client.add_to_goal(&user, &id, &500);

    let new_balance = client.withdraw_from_goal(&user, &id, &500);
    assert_eq!(new_balance, 0);

    let goal = client.get_goal(&id).unwrap();
    assert_eq!(goal.current_amount, 0);
    assert!(!client.is_goal_completed(&id));
}

#[test]
fn test_exact_goal_completion() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(&user, &String::from_str(&env, "Exact"), &1000, &2000000000);

    // Add 500 twice
    client.add_to_goal(&user, &id, &500);
    assert!(!client.is_goal_completed(&id));

    client.add_to_goal(&user, &id, &500);
    assert!(client.is_goal_completed(&id));

    let goal = client.get_goal(&id).unwrap();
    assert_eq!(goal.current_amount, 1000);
}

#[test]
fn test_set_time_lock_succeeds() {
    setup_test_env!(env, SavingsGoalContract, client, owner);
    client.init();
    set_ledger_time(&env, 1, 1000);

    let goal_id = client.create_goal(&owner, &String::from_str(&env, "Education"), &10000, &5000);

    client.set_time_lock(&owner, &goal_id, &10000);

    let goal = client.get_goal(&goal_id).unwrap();
    assert_eq!(goal.unlock_date, Some(10000));
}

#[test]
fn test_withdraw_time_locked_goal_before_unlock() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner = <soroban_sdk::Address as AddressTrait>::generate(&env);

    env.mock_all_auths();
    set_ledger_time(&env, 1, 1000);

    let goal_id = client.create_goal(&owner, &String::from_str(&env, "Education"), &10000, &5000);

    client.add_to_goal(&owner, &goal_id, &5000);
    client.unlock_goal(&owner, &goal_id);
    client.set_time_lock(&owner, &goal_id, &10000);

    let result = client.try_withdraw_from_goal(&owner, &goal_id, &1000);
    assert!(result.is_err());
}

#[test]
fn test_withdraw_time_locked_goal_after_unlock() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner = <soroban_sdk::Address as AddressTrait>::generate(&env);

    env.mock_all_auths();
    set_ledger_time(&env, 1, 1000);

    let goal_id = client.create_goal(&owner, &String::from_str(&env, "Education"), &10000, &5000);

    client.add_to_goal(&owner, &goal_id, &5000);
    client.unlock_goal(&owner, &goal_id);
    client.set_time_lock(&owner, &goal_id, &3000);

    set_ledger_time(&env, 1, 3500);
    let new_amount = client.withdraw_from_goal(&owner, &goal_id, &1000);
    assert_eq!(new_amount, 4000);
}

#[test]
fn test_create_savings_schedule() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner = <soroban_sdk::Address as AddressTrait>::generate(&env);

    env.mock_all_auths();
    set_ledger_time(&env, 1, 1000);

    let goal_id = client.create_goal(&owner, &String::from_str(&env, "Education"), &10000, &5000);

    let schedule_id = client.create_savings_schedule(&owner, &goal_id, &500, &3000, &86400);
    assert_eq!(schedule_id, 1);

    let schedule = client.get_savings_schedule(&schedule_id);
    assert!(schedule.is_some());
    let schedule = schedule.unwrap();
    assert_eq!(schedule.amount, 500);
    assert_eq!(schedule.next_due, 3000);
    assert!(schedule.active);
}

#[test]
fn test_modify_savings_schedule() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner = <soroban_sdk::Address as AddressTrait>::generate(&env);

    env.mock_all_auths();
    set_ledger_time(&env, 1, 1000);

    let goal_id = client.create_goal(&owner, &String::from_str(&env, "Education"), &10000, &5000);

    let schedule_id = client.create_savings_schedule(&owner, &goal_id, &500, &3000, &86400);
    client.modify_savings_schedule(&owner, &schedule_id, &1000, &4000, &172800);

    let schedule = client.get_savings_schedule(&schedule_id).unwrap();
    assert_eq!(schedule.amount, 1000);
    assert_eq!(schedule.next_due, 4000);
    assert_eq!(schedule.interval, 172800);
}

#[test]
fn test_cancel_savings_schedule() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner = <soroban_sdk::Address as AddressTrait>::generate(&env);

    env.mock_all_auths();
    set_ledger_time(&env, 1, 1000);

    let goal_id = client.create_goal(&owner, &String::from_str(&env, "Education"), &10000, &5000);

    let schedule_id = client.create_savings_schedule(&owner, &goal_id, &500, &3000, &86400);
    client.cancel_savings_schedule(&owner, &schedule_id);

    let schedule = client.get_savings_schedule(&schedule_id).unwrap();
    assert!(!schedule.active);
}

#[test]
fn test_execute_due_savings_schedules() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner = <soroban_sdk::Address as AddressTrait>::generate(&env);

    env.mock_all_auths();
    set_ledger_time(&env, 1, 1000);

    let goal_id = client.create_goal(&owner, &String::from_str(&env, "Education"), &10000, &5000);

    let schedule_id = client.create_savings_schedule(&owner, &goal_id, &500, &3000, &0);

    set_ledger_time(&env, 1, 3500);
    let executed = client.execute_due_savings_schedules();

    assert_eq!(executed.len(), 1);
    assert_eq!(executed.get(0).unwrap(), schedule_id);

    let goal = client.get_goal(&goal_id).unwrap();
    assert_eq!(goal.current_amount, 500);
}

#[test]
fn test_execute_recurring_savings_schedule() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner = <soroban_sdk::Address as AddressTrait>::generate(&env);

    env.mock_all_auths();
    set_ledger_time(&env, 1, 1000);

    let goal_id = client.create_goal(&owner, &String::from_str(&env, "Education"), &10000, &5000);

    let schedule_id = client.create_savings_schedule(&owner, &goal_id, &500, &3000, &86400);

    set_ledger_time(&env, 1, 3500);
    client.execute_due_savings_schedules();

    let schedule = client.get_savings_schedule(&schedule_id).unwrap();
    assert!(schedule.active);
    assert_eq!(schedule.next_due, 3000 + 86400);

    let goal = client.get_goal(&goal_id).unwrap();
    assert_eq!(goal.current_amount, 500);
}

#[test]
fn test_execute_missed_savings_schedules() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner = <soroban_sdk::Address as AddressTrait>::generate(&env);

    env.mock_all_auths();
    set_ledger_time(&env, 1, 1000);

    let goal_id = client.create_goal(&owner, &String::from_str(&env, "Education"), &10000, &5000);

    let schedule_id = client.create_savings_schedule(&owner, &goal_id, &500, &3000, &86400);

    set_ledger_time(&env, 1, 3000 + 86400 * 3 + 100);
    client.execute_due_savings_schedules();

    let schedule = client.get_savings_schedule(&schedule_id).unwrap();
    assert_eq!(schedule.missed_count, 3);
    assert!(schedule.next_due > 3000 + 86400 * 3);
}

#[test]
fn test_savings_schedule_goal_completion() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let owner = <soroban_sdk::Address as AddressTrait>::generate(&env);

    env.mock_all_auths();
    set_ledger_time(&env, 1, 1000);

    let goal_id = client.create_goal(&owner, &String::from_str(&env, "Education"), &1000, &5000);

    client.create_savings_schedule(&owner, &goal_id, &1000, &3000, &0);

    set_ledger_time(&env, 1, 3500);
    client.execute_due_savings_schedules();

    let goal = client.get_goal(&goal_id).unwrap();
    assert_eq!(goal.current_amount, 1000);
    assert!(client.is_goal_completed(&goal_id));
}

#[test]
fn test_lock_goal_success() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(
        &user,
        &String::from_str(&env, "Lock Test"),
        &1000,
        &2000000000,
    );

    client.unlock_goal(&user, &id);
    assert!(!client.get_goal(&id).unwrap().locked);

    client.lock_goal(&user, &id);
    assert!(client.get_goal(&id).unwrap().locked);
}

#[test]
fn test_unlock_goal_success() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(
        &user,
        &String::from_str(&env, "Unlock Test"),
        &1000,
        &2000000000,
    );

    assert!(client.get_goal(&id).unwrap().locked);

    client.unlock_goal(&user, &id);
    assert!(!client.get_goal(&id).unwrap().locked);
}

#[test]
fn test_lock_goal_unauthorized_panics() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let other = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(
        &user,
        &String::from_str(&env, "Auth Test"),
        &1000,
        &2000000000,
    );

    client.unlock_goal(&user, &id);

    let res = client.try_lock_goal(&other, &id);
    assert!(res.is_err());
}

#[test]
fn test_unlock_goal_unauthorized_panics() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let other = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(
        &user,
        &String::from_str(&env, "Auth Test"),
        &1000,
        &2000000000,
    );

    let res = client.try_unlock_goal(&other, &id);
    assert!(res.is_err());
}

#[test]
fn test_withdraw_after_lock_fails() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(
        &user,
        &String::from_str(&env, "Withdraw Fail"),
        &1000,
        &2000000000,
    );

    client.unlock_goal(&user, &id);
    client.add_to_goal(&user, &id, &500);
    client.lock_goal(&user, &id);

    let res = client.try_withdraw_from_goal(&user, &id, &100);
    assert!(res.is_err());
}

#[test]
fn test_withdraw_after_unlock_succeeds() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();
    let id = client.create_goal(
        &user,
        &String::from_str(&env, "Withdraw Success"),
        &1000,
        &2000000000,
    );

    client.unlock_goal(&user, &id);
    client.add_to_goal(&user, &id, &500);

    let new_balance = client.withdraw_from_goal(&user, &id, &200);
    assert_eq!(new_balance, 300);

    let goal = client.get_goal(&id).unwrap();
    assert_eq!(goal.current_amount, 300);
}

#[test]
fn test_lock_nonexistent_goal_panics() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();

    let res = client.try_lock_goal(&user, &99);
    assert!(res.is_err());
}

#[test]
fn test_create_goal_emits_event() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();

    // Create a goal
    let goal_id = client.create_goal(
        &user,
        &String::from_str(&env, "Education"),
        &10000,
        &1735689600, // Future date
    );
    assert_eq!(goal_id, 1);

    let events = env.events().all();
    let mut found_created_struct = false;
    let mut found_created_enum = false;

    for event in events.iter() {
        let topics = event.1;
        let topic0: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();

        if topic0 == GOAL_CREATED {
            let event_data: GoalCreatedEvent =
                GoalCreatedEvent::try_from_val(&env, &event.2).unwrap();
            assert_eq!(event_data.goal_id, goal_id);
            found_created_struct = true;
        }

        if topic0 == symbol_short!("savings") && topics.len() > 1 {
            let topic1: SavingsEvent =
                SavingsEvent::try_from_val(&env, &topics.get(1).unwrap()).unwrap();
            if matches!(topic1, SavingsEvent::GoalCreated) {
                found_created_enum = true;
            }
        }
    }

    assert!(
        found_created_struct,
        "GoalCreated struct event was not emitted"
    );
    assert!(
        found_created_enum,
        "SavingsEvent::GoalCreated was not emitted"
    );
}

#[test]
fn test_add_to_goal_emits_event() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();

    // Create a goal
    let goal_id = client.create_goal(
        &user,
        &String::from_str(&env, "Medical"),
        &5000,
        &1735689600,
    );

    // Add funds
    let new_amount = client.add_to_goal(&user, &goal_id, &1000);
    assert_eq!(new_amount, 1000);

    let events = env.events().all();
    let mut found_added_struct = false;
    let mut found_added_enum = false;

    for event in events.iter() {
        let topics = event.1;
        let topic0: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();

        if topic0 == FUNDS_ADDED {
            let event_data: FundsAddedEvent =
                FundsAddedEvent::try_from_val(&env, &event.2).unwrap();
            assert_eq!(event_data.goal_id, goal_id);
            assert_eq!(event_data.amount, 1000);
            found_added_struct = true;
        }

        if topic0 == symbol_short!("savings") && topics.len() > 1 {
            let topic1: SavingsEvent =
                SavingsEvent::try_from_val(&env, &topics.get(1).unwrap()).unwrap();
            if matches!(topic1, SavingsEvent::FundsAdded) {
                found_added_enum = true;
            }
        }
    }

    assert!(
        found_added_struct,
        "FundsAdded struct event was not emitted"
    );
    assert!(found_added_enum, "SavingsEvent::FundsAdded was not emitted");
}

#[test]
fn test_goal_completed_emits_event() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();

    // Create a goal with small target
    let goal_id = client.create_goal(
        &user,
        &String::from_str(&env, "Emergency Fund"),
        &1000,
        &1735689600,
    );

    // Add funds to complete the goal
    client.add_to_goal(&user, &goal_id, &1000);

    let events = env.events().all();
    let mut found_completed_struct = false;
    let mut found_completed_enum = false;

    for event in events.iter() {
        let topics = event.1;
        let topic0: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();

        if topic0 == GOAL_COMPLETED {
            let event_data: GoalCompletedEvent =
                GoalCompletedEvent::try_from_val(&env, &event.2).unwrap();
            assert_eq!(event_data.goal_id, goal_id);
            assert_eq!(event_data.final_amount, 1000);
            found_completed_struct = true;
        }

        if topic0 == symbol_short!("savings") && topics.len() > 1 {
            let topic1: SavingsEvent =
                SavingsEvent::try_from_val(&env, &topics.get(1).unwrap()).unwrap();
            if matches!(topic1, SavingsEvent::GoalCompleted) {
                found_completed_enum = true;
            }
        }
    }

    assert!(
        found_completed_struct,
        "GoalCompleted struct event was not emitted"
    );
    assert!(
        found_completed_enum,
        "SavingsEvent::GoalCompleted was not emitted"
    );
}

#[test]
fn test_withdraw_from_goal_emits_event() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();

    let goal_id = client.create_goal(
        &user,
        &String::from_str(&env, "Withdraw Event"),
        &5000,
        &1735689600,
    );
    client.unlock_goal(&user, &goal_id);
    client.add_to_goal(&user, &goal_id, &1500);
    client.withdraw_from_goal(&user, &goal_id, &600);

    let events = env.events().all();
    let mut found_withdrawn_enum = false;

    for event in events.iter() {
        let topics = event.1;
        let topic0: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
        if topic0 == symbol_short!("savings") && topics.len() > 1 {
            let topic1: SavingsEvent =
                SavingsEvent::try_from_val(&env, &topics.get(1).unwrap()).unwrap();
            if matches!(topic1, SavingsEvent::FundsWithdrawn) {
                found_withdrawn_enum = true;
            }
        }
    }

    assert!(
        found_withdrawn_enum,
        "SavingsEvent::FundsWithdrawn was not emitted"
    );
}

#[test]
fn test_lock_goal_emits_event() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();

    let goal_id = client.create_goal(
        &user,
        &String::from_str(&env, "Lock Event"),
        &5000,
        &1735689600,
    );
    client.unlock_goal(&user, &goal_id);
    client.lock_goal(&user, &goal_id);

    let events = env.events().all();
    let mut found_locked_enum = false;

    for event in events.iter() {
        let topics = event.1;
        let topic0: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
        if topic0 == symbol_short!("savings") && topics.len() > 1 {
            let topic1: SavingsEvent =
                SavingsEvent::try_from_val(&env, &topics.get(1).unwrap()).unwrap();
            if matches!(topic1, SavingsEvent::GoalLocked) {
                found_locked_enum = true;
            }
        }
    }

    assert!(
        found_locked_enum,
        "SavingsEvent::GoalLocked was not emitted"
    );
}

#[test]
fn test_unlock_goal_emits_event() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();

    let goal_id = client.create_goal(
        &user,
        &String::from_str(&env, "Unlock Event"),
        &5000,
        &1735689600,
    );
    client.unlock_goal(&user, &goal_id);

    let events = env.events().all();
    let mut found_unlocked_enum = false;

    for event in events.iter() {
        let topics = event.1;
        let topic0: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
        if topic0 == symbol_short!("savings") && topics.len() > 1 {
            let topic1: SavingsEvent =
                SavingsEvent::try_from_val(&env, &topics.get(1).unwrap()).unwrap();
            if matches!(topic1, SavingsEvent::GoalUnlocked) {
                found_unlocked_enum = true;
            }
        }
    }

    assert!(
        found_unlocked_enum,
        "SavingsEvent::GoalUnlocked was not emitted"
    );
}

#[test]
fn test_multiple_goals_emit_separate_events() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();
    env.mock_all_auths();

    // Create multiple goals
    client.create_goal(&user, &String::from_str(&env, "Goal 1"), &1000, &1735689600);
    client.create_goal(&user, &String::from_str(&env, "Goal 2"), &2000, &1735689600);
    client.create_goal(&user, &String::from_str(&env, "Goal 3"), &3000, &1735689600);

    // Should have 3 * 2 events = 6 events
    let events = env.events().all();
    assert_eq!(events.len(), 6);
}

// ============================================================================
// Storage TTL Extension Tests
//
// Verify that instance storage TTL is properly extended on state-changing
// operations, preventing unexpected data expiration.
//
// Contract TTL configuration:
//   INSTANCE_LIFETIME_THRESHOLD = 17,280 ledgers (~1 day)
//   INSTANCE_BUMP_AMOUNT        = 518,400 ledgers (~30 days)
//
// Operations extending instance TTL:
//   create_goal, add_to_goal, batch_add_to_goals, withdraw_from_goal,
//   lock_goal, unlock_goal, import_snapshot, set_time_lock,
//   create_savings_schedule, modify_savings_schedule,
//   cancel_savings_schedule, execute_due_savings_schedules
// ============================================================================

/// Verify that create_goal extends instance storage TTL.
#[test]
fn test_instance_ttl_extended_on_create_goal() {
    let env = Env::default();
    env.mock_all_auths();

    env.ledger().set(LedgerInfo {
        protocol_version: 20,
        sequence_number: 100,
        timestamp: 1000,
        network_id: [0; 32],
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 700_000,
    });

    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();

    // create_goal calls extend_instance_ttl
    let goal_id = client.create_goal(
        &user,
        &String::from_str(&env, "Emergency Fund"),
        &10000,
        &1735689600,
    );
    assert!(goal_id > 0);

    // Inspect instance TTL — must be at least INSTANCE_BUMP_AMOUNT
    let ttl = env.as_contract(&contract_id, || env.storage().instance().get_ttl());
    assert!(
        ttl >= 518_400,
        "Instance TTL ({}) must be >= INSTANCE_BUMP_AMOUNT (518,400) after create_goal",
        ttl
    );
}

/// Verify that add_to_goal refreshes instance TTL after ledger advancement.
///
/// extend_ttl(threshold, extend_to) only extends when TTL <= threshold.
/// We advance the ledger far enough for TTL to drop below 17,280.
#[test]
fn test_instance_ttl_refreshed_on_add_to_goal() {
    let env = Env::default();
    env.mock_all_auths();

    env.ledger().set(LedgerInfo {
        protocol_version: 20,
        sequence_number: 100,
        timestamp: 1000,
        network_id: [0; 32],
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 700_000,
    });

    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();

    let goal_id = client.create_goal(
        &user,
        &String::from_str(&env, "Vacation"),
        &5000,
        &2000000000,
    );

    // Advance ledger so TTL drops below threshold (17,280)
    // After create_goal: live_until = 518,500. At seq 510,000: TTL = 8,500
    env.ledger().set(LedgerInfo {
        protocol_version: 20,
        sequence_number: 510_000,
        timestamp: 500_000,
        network_id: [0; 32],
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 700_000,
    });

    // add_to_goal calls extend_instance_ttl → re-extends TTL to 518,400
    let new_balance = client.add_to_goal(&user, &goal_id, &500);
    assert_eq!(new_balance, 500);

    let ttl = env.as_contract(&contract_id, || env.storage().instance().get_ttl());
    assert!(
        ttl >= 518_400,
        "Instance TTL ({}) must be >= 518,400 after add_to_goal",
        ttl
    );
}

/// Verify data persists across repeated operations spanning multiple
/// ledger advancements, proving TTL is continuously renewed.
#[test]
fn test_savings_data_persists_across_ledger_advancements() {
    let env = Env::default();
    env.mock_all_auths();

    env.ledger().set(LedgerInfo {
        protocol_version: 20,
        sequence_number: 100,
        timestamp: 1000,
        network_id: [0; 32],
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 700_000,
    });

    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();

    // Phase 1: Create goals at seq 100. live_until = 518,500
    let id1 = client.create_goal(
        &user,
        &String::from_str(&env, "Education"),
        &10000,
        &2000000000,
    );
    let id2 = client.create_goal(&user, &String::from_str(&env, "House"), &50000, &2000000000);

    // Phase 2: Advance to seq 510,000 (TTL = 8,500 < 17,280)
    env.ledger().set(LedgerInfo {
        protocol_version: 20,
        sequence_number: 510_000,
        timestamp: 510_000,
        network_id: [0; 32],
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 700_000,
    });

    client.add_to_goal(&user, &id1, &3000);

    // Phase 3: Advance to seq 1,020,000 (TTL = 8,400 < 17,280)
    env.ledger().set(LedgerInfo {
        protocol_version: 20,
        sequence_number: 1_020_000,
        timestamp: 1_020_000,
        network_id: [0; 32],
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 700_000,
    });

    // Add more funds to second goal
    client.add_to_goal(&user, &id2, &10000);

    // All goals should be accessible with correct data
    let goal1 = client.get_goal(&id1);
    assert!(
        goal1.is_some(),
        "First goal must persist across ledger advancements"
    );
    assert_eq!(goal1.unwrap().current_amount, 3000);

    let goal2 = client.get_goal(&id2);
    assert!(goal2.is_some(), "Second goal must persist");
    assert_eq!(goal2.unwrap().current_amount, 10000);

    // TTL should be fully refreshed
    let ttl = env.as_contract(&contract_id, || env.storage().instance().get_ttl());
    assert!(
        ttl >= 518_400,
        "Instance TTL ({}) must remain >= 518,400 after repeated operations",
        ttl
    );
}

/// Verify that lock_goal extends instance TTL.
#[test]
fn test_instance_ttl_extended_on_lock_goal() {
    let env = Env::default();
    env.mock_all_auths();

    env.ledger().set(LedgerInfo {
        protocol_version: 20,
        sequence_number: 100,
        timestamp: 1000,
        network_id: [0; 32],
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 700_000,
    });

    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.init();

    let goal_id = client.create_goal(
        &user,
        &String::from_str(&env, "Retirement"),
        &100000,
        &2000000000,
    );

    // Advance ledger past threshold
    env.ledger().set(LedgerInfo {
        protocol_version: 20,
        sequence_number: 510_000,
        timestamp: 510_000,
        network_id: [0; 32],
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 700_000,
    });

    // lock_goal calls extend_instance_ttl
    client.lock_goal(&user, &goal_id);

    let ttl = env.as_contract(&contract_id, || env.storage().instance().get_ttl());
    assert!(
        ttl >= 518_400,
        "Instance TTL ({}) must be >= 518,400 after lock_goal",
        ttl
    );
}

fn setup_goals(env: &Env, client: &SavingsGoalContractClient, owner: &Address, count: u32) {
    for i in 0..count {
        client.create_goal(
            owner,
            &soroban_sdk::String::from_str(env, "Goal"),
            &(1000i128 * (i as i128 + 1)),
            &(env.ledger().timestamp() + 86400 * (i as u64 + 1)),
        );
    }
}

#[test]
fn test_get_goals_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &id);
    let owner = Address::generate(&env);

    client.init();
    let page = client.get_goals(&owner, &0, &0);
    assert_eq!(page.count, 0);
    assert_eq!(page.next_cursor, 0);
    assert_eq!(page.items.len(), 0);
}

#[test]
fn test_get_goals_single_page() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &id);
    let owner = Address::generate(&env);

    client.init();
    setup_goals(&env, &client, &owner, 5);

    let page = client.get_goals(&owner, &0, &10);
    assert_eq!(page.count, 5);
    assert_eq!(page.next_cursor, 0);
}

#[test]
fn test_get_goals_multiple_pages() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &id);
    let owner = Address::generate(&env);

    client.init();
    setup_goals(&env, &client, &owner, 9);

    // Page 1
    let page1 = client.get_goals(&owner, &0, &4);
    assert_eq!(page1.count, 4);
    assert!(page1.next_cursor > 0);

    // Page 2
    let page2 = client.get_goals(&owner, &page1.next_cursor, &4);
    assert_eq!(page2.count, 4);
    assert!(page2.next_cursor > 0);

    // Page 3 (last)
    let page3 = client.get_goals(&owner, &page2.next_cursor, &4);
    assert_eq!(page3.count, 1);
    assert_eq!(page3.next_cursor, 0);
}

#[test]
fn test_get_goals_multi_owner_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &id);
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);

    client.init();
    setup_goals(&env, &client, &owner_a, 3);
    setup_goals(&env, &client, &owner_b, 4);

    let page_a = client.get_goals(&owner_a, &0, &20);
    assert_eq!(page_a.count, 3);
    for g in page_a.items.iter() {
        assert_eq!(g.owner, owner_a);
    }

    let page_b = client.get_goals(&owner_b, &0, &20);
    assert_eq!(page_b.count, 4);
}

#[test]
fn test_get_goals_cursor_is_exclusive() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &id);
    let owner = Address::generate(&env);

    client.init();
    setup_goals(&env, &client, &owner, 4);

    let first = client.get_goals(&owner, &0, &2);
    assert_eq!(first.count, 2);
    let last_id = first.items.get(1).unwrap().id;

    // cursor should be exclusive — next page should NOT include last_id
    let second = client.get_goals(&owner, &last_id, &2);
    for g in second.items.iter() {
        assert!(g.id > last_id, "cursor should be exclusive");
    }
}

#[test]
fn test_limit_zero_uses_default() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &id);
    let owner = Address::generate(&env);

    client.init();
    setup_goals(&env, &client, &owner, 3);
    let page = client.get_goals(&owner, &0, &0);
    assert_eq!(page.count, 3); // 3 < DEFAULT_PAGE_LIMIT so all returned
}

#[test]
fn test_get_all_goals_backward_compat() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &id);
    let owner = Address::generate(&env);

    client.init();
    setup_goals(&env, &client, &owner, 5);
    let all = client.get_all_goals(&owner);
    assert_eq!(all.len(), 5);
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_add_to_goal_non_owner_auth_failure() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let other = Address::generate(&env);

    client.init();
    client.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &user,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "create_goal",
            args: (
                &user,
                String::from_str(&env, "Auth"),
                1000i128,
                2000000000u64,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let id = client.create_goal(&user, &String::from_str(&env, "Auth"), &1000, &2000000000);
    client.add_to_goal(&other, &id, &500);
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_withdraw_from_goal_non_owner_auth_failure() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let other = Address::generate(&env);

    client.init();
    client.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &user,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "create_goal",
            args: (
                &user,
                String::from_str(&env, "Auth"),
                1000i128,
                2000000000u64,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let id = client.create_goal(&user, &String::from_str(&env, "Auth"), &1000, &2000000000);
    client.withdraw_from_goal(&other, &id, &100);
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_lock_goal_non_owner_auth_failure() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let other = Address::generate(&env);

    client.init();
    client.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &user,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "create_goal",
            args: (
                &user,
                String::from_str(&env, "Auth"),
                1000i128,
                2000000000u64,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let id = client.create_goal(&user, &String::from_str(&env, "Auth"), &1000, &2000000000);
    client.lock_goal(&other, &id);
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_unlock_goal_non_owner_auth_failure() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let other = Address::generate(&env);

    client.init();
    client.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &user,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "create_goal",
            args: (
                &user,
                String::from_str(&env, "Auth"),
                1000i128,
                2000000000u64,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let id = client.create_goal(&user, &String::from_str(&env, "Auth"), &1000, &2000000000);
    client.unlock_goal(&other, &id);
}

#[test]
fn test_get_all_goals_filters_by_owner() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SavingsGoalContract);
    let client = SavingsGoalContractClient::new(&env, &contract_id);

    client.init();
    env.mock_all_auths();

    // Create two different owners
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);

    // Create goals for owner_a
    let goal_a1 = client.create_goal(
        &owner_a,
        &String::from_str(&env, "Goal A1"),
        &1000,
        &1735689600,
    );
    let goal_a2 = client.create_goal(
        &owner_a,
        &String::from_str(&env, "Goal A2"),
        &2000,
        &1735689600,
    );
    let goal_a3 = client.create_goal(
        &owner_a,
        &String::from_str(&env, "Goal A3"),
        &3000,
        &1735689600,
    );

    // Create goals for owner_b
    let goal_b1 = client.create_goal(
        &owner_b,
        &String::from_str(&env, "Goal B1"),
        &5000,
        &1735689600,
    );
    let goal_b2 = client.create_goal(
        &owner_b,
        &String::from_str(&env, "Goal B2"),
        &6000,
        &1735689600,
    );

    // Get all goals for owner_a
    let goals_a = client.get_all_goals(&owner_a);
    assert_eq!(goals_a.len(), 3, "Owner A should have exactly 3 goals");

    // Verify all goals returned for owner_a belong to owner_a
    for goal in goals_a.iter() {
        assert_eq!(
            goal.owner, owner_a,
            "Goal {} should belong to owner_a",
            goal.id
        );
    }

    // Verify goal IDs for owner_a are correct
    let goal_a_ids: Vec<u32> = goals_a.iter().map(|g| g.id).collect();
    assert!(goal_a_ids.contains(&goal_a1), "Goals for A should contain goal_a1");
    assert!(goal_a_ids.contains(&goal_a2), "Goals for A should contain goal_a2");
    assert!(goal_a_ids.contains(&goal_a3), "Goals for A should contain goal_a3");

    // Get all goals for owner_b
    let goals_b = client.get_all_goals(&owner_b);
    assert_eq!(goals_b.len(), 2, "Owner B should have exactly 2 goals");

    // Verify all goals returned for owner_b belong to owner_b
    for goal in goals_b.iter() {
        assert_eq!(
            goal.owner, owner_b,
            "Goal {} should belong to owner_b",
            goal.id
        );
    }

    // Verify goal IDs for owner_b are correct
    let goal_b_ids: Vec<u32> = goals_b.iter().map(|g| g.id).collect();
    assert!(goal_b_ids.contains(&goal_b1), "Goals for B should contain goal_b1");
    assert!(goal_b_ids.contains(&goal_b2), "Goals for B should contain goal_b2");

    // Verify that goal IDs between owner_a and owner_b are disjoint
    for goal_a_id in &goal_a_ids {
        assert!(
            !goal_b_ids.contains(goal_a_id),
            "Goal ID {} from owner A should not appear in owner B's goals",
            goal_a_id
        );
    }

    // Verify owner_a's goals do not appear in owner_b's goals and vice versa
    for goal_a_id in goal_a_ids {
        for goal in goals_b.iter() {
            assert_ne!(
                goal.id, goal_a_id,
                "Owner B's goal list should not contain owner A's goals"
            );
        }
    }
}

// ============================================================================
// End-to-end migration compatibility tests — savings_goals ↔ data_migration
//
// These tests exercise the full export ↔ import pipeline across both
// packages: the Soroban contract (savings_goals) and the off-chain migration
// utilities (data_migration). All four format paths are covered.
//
// Approach:
//   1. Use the Soroban test env to create real on-chain goal state.
//   2. Call `export_snapshot()` to get a `GoalsExportSnapshot`.
//   3. Convert to `data_migration::SavingsGoalsExport` (field mapping).
//   4. Use `data_migration` helpers to serialize, deserialize, and validate.
//   5. Assert field fidelity and security invariants.
//
// Security invariants validated:
//   - Checksum integrity is preserved across all format paths.
//   - Tampered checksums are rejected by `validate_for_import`.
//   - Incompatible schema versions are rejected.
//   - `locked` and `unlock_date` flags are faithfully exported.
// ============================================================================
#[cfg(test)]
mod migration_e2e_tests {
    use super::*;
    use data_migration::{
        build_savings_snapshot, export_to_binary, export_to_csv, export_to_encrypted_payload,
        export_to_json, import_from_binary, import_from_encrypted_payload, import_from_json,
        import_goals_from_csv, ExportFormat, MigrationError, SavingsGoalExport,
        SavingsGoalsExport, SnapshotPayload, SCHEMA_VERSION,
    };
    use soroban_sdk::{testutils::Address as AddressTrait, Address, Env};
    extern crate alloc;
    use alloc::vec::Vec as StdVec;

    // -------------------------------------------------------------------------
    // Helper: convert an on-chain GoalsExportSnapshot into a data_migration export.
    // -------------------------------------------------------------------------

    /// Convert a `GoalsExportSnapshot` (from the contract) into a
    /// `data_migration::SavingsGoalsExport` (for off-chain processing).
    ///
    /// The `owner` field in `SavingsGoal` is a `soroban_sdk::Address`; we
    /// convert it to a hex string using its debug representation so the
    /// off-chain struct can store it as a plain `String`.
    fn to_migration_export(snapshot: &GoalsExportSnapshot, _env: &Env) -> SavingsGoalsExport {
        let mut goals: StdVec<SavingsGoalExport> = StdVec::new();
        for i in 0..snapshot.goals.len() {
            if let Some(g) = snapshot.goals.get(i) {
                // Convert soroban_sdk::String to alloc String via byte buffer.
                let name_str: alloc::string::String = {
                    let len = g.name.len() as usize;
                    let mut buf = alloc::vec![0u8; len];
                    g.name.copy_into_slice(&mut buf);
                    alloc::string::String::from_utf8_lossy(&buf).into_owned()
                };
                goals.push(SavingsGoalExport {
                    id: g.id,
                    owner: alloc::format!("{:?}", g.owner),
                    name: name_str,
                    // SavingsGoal uses i128; data_migration stores i64.
                    // Test amounts are small so the cast is safe.
                    target_amount: g.target_amount as i64,
                    current_amount: g.current_amount as i64,
                    target_date: g.target_date,
                    locked: g.locked,
                });
            }
        }
        SavingsGoalsExport {
            next_id: snapshot.next_id,
            goals,
        }
    }

    // -------------------------------------------------------------------------
    // JSON format
    // -------------------------------------------------------------------------

    /// E2E: export on-chain goals → data_migration JSON bytes → import → verify fields.
    ///
    /// Tests the complete pipeline: contract state → `export_snapshot` →
    /// `SavingsGoalsExport` → `build_savings_snapshot` (JSON) →
    /// `export_to_json` → `import_from_json` → field assertions.
    #[test]
    fn test_e2e_contract_export_import_json_roundtrip() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SavingsGoalContract);
        let client = SavingsGoalContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);

        client.init();
        let goal_id = client.create_goal(
            &owner,
            &String::from_str(&env, "Vacation"),
            &10_000i128,
            &2_000_000_000u64,
        );
        client.add_to_goal(&owner, &goal_id, &3_500i128);

        // Export on-chain snapshot.
        let snapshot = client.export_snapshot(&owner);
        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.goals.len(), 1);

        // Convert and build migration snapshot.
        let migration_export = to_migration_export(&snapshot, &env);
        assert_eq!(migration_export.next_id, 1);
        assert_eq!(migration_export.goals.len(), 1);
        let mig_goal = &migration_export.goals[0];
        assert_eq!(mig_goal.id, 1);
        assert_eq!(mig_goal.target_amount, 10_000);
        assert_eq!(mig_goal.current_amount, 3_500);
        assert_eq!(mig_goal.target_date, 2_000_000_000);

        let mig_snapshot = build_savings_snapshot(migration_export, ExportFormat::Json);
        assert!(mig_snapshot.verify_checksum());

        // Serialize to JSON and reimport.
        let bytes = export_to_json(&mig_snapshot).unwrap();
        let loaded = import_from_json(&bytes).unwrap();
        assert_eq!(loaded.header.version, SCHEMA_VERSION);
        assert!(loaded.verify_checksum());

        if let SnapshotPayload::SavingsGoals(ref g) = loaded.payload {
            assert_eq!(g.goals.len(), 1);
            assert_eq!(g.goals[0].target_amount, 10_000);
            assert_eq!(g.goals[0].current_amount, 3_500);
            assert_eq!(g.goals[0].target_date, 2_000_000_000);
        } else {
            panic!("Expected SavingsGoals payload");
        }
    }

    // -------------------------------------------------------------------------
    // Binary format
    // -------------------------------------------------------------------------

    /// E2E: contract export → binary serialization → import → checksum verified.
    #[test]
    fn test_e2e_contract_export_import_binary_roundtrip() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SavingsGoalContract);
        let client = SavingsGoalContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);

        client.init();
        let goal_id = client.create_goal(
            &owner,
            &String::from_str(&env, "Emergency"),
            &20_000i128,
            &1_900_000_000u64,
        );
        client.add_to_goal(&owner, &goal_id, &5_000i128);

        let snapshot = client.export_snapshot(&owner);
        let migration_export = to_migration_export(&snapshot, &env);

        let mig_snapshot = build_savings_snapshot(migration_export, ExportFormat::Binary);
        assert!(mig_snapshot.verify_checksum());

        let bytes = export_to_binary(&mig_snapshot).unwrap();
        assert!(!bytes.is_empty());

        let loaded = import_from_binary(&bytes).unwrap();
        assert_eq!(loaded.header.version, SCHEMA_VERSION);
        assert_eq!(loaded.header.format, "binary");
        assert!(loaded.verify_checksum());

        if let SnapshotPayload::SavingsGoals(ref g) = loaded.payload {
            assert_eq!(g.goals[0].target_amount, 20_000);
            assert_eq!(g.goals[0].current_amount, 5_000);
        } else {
            panic!("Expected SavingsGoals payload");
        }
    }

    // -------------------------------------------------------------------------
    // CSV format
    // -------------------------------------------------------------------------

    /// E2E: multiple contract goals → CSV export → import → all records preserved.
    #[test]
    fn test_e2e_contract_export_import_csv_roundtrip() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SavingsGoalContract);
        let client = SavingsGoalContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);

        client.init();
        let id1 = client.create_goal(
            &owner,
            &String::from_str(&env, "Trip"),
            &8_000i128,
            &2_000_000_000u64,
        );
        let id2 = client.create_goal(
            &owner,
            &String::from_str(&env, "Gadget"),
            &3_000i128,
            &2_000_000_000u64,
        );
        client.add_to_goal(&owner, &id1, &2_000i128);
        client.add_to_goal(&owner, &id2, &1_500i128);

        let snapshot = client.export_snapshot(&owner);
        assert_eq!(snapshot.goals.len(), 2);

        let migration_export = to_migration_export(&snapshot, &env);
        let csv_bytes = export_to_csv(&migration_export).unwrap();
        assert!(!csv_bytes.is_empty());

        let goals = import_goals_from_csv(&csv_bytes).unwrap();
        assert_eq!(goals.len(), 2, "both goals must survive CSV roundtrip");

        // Verify amounts are preserved.
        let g1 = goals.iter().find(|g| g.id == 1).expect("goal 1 must be present");
        let g2 = goals.iter().find(|g| g.id == 2).expect("goal 2 must be present");
        assert_eq!(g1.target_amount, 8_000);
        assert_eq!(g1.current_amount, 2_000);
        assert_eq!(g2.target_amount, 3_000);
        assert_eq!(g2.current_amount, 1_500);
    }

    // -------------------------------------------------------------------------
    // Encrypted format
    // -------------------------------------------------------------------------

    /// E2E: contract export → JSON bytes → base64 wrap → decode → re-import.
    ///
    /// Simulates the encrypted-channel path: caller serialises to JSON, wraps
    /// in base64 (as would an encryption layer), transmits, then decodes
    /// and re-imports.
    #[test]
    fn test_e2e_contract_export_import_encrypted_roundtrip() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SavingsGoalContract);
        let client = SavingsGoalContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);

        client.init();
        let goal_id = client.create_goal(
            &owner,
            &String::from_str(&env, "House"),
            &500_000i128,
            &2_100_000_000u64,
        );
        client.add_to_goal(&owner, &goal_id, &100_000i128);

        let snapshot = client.export_snapshot(&owner);
        let migration_export = to_migration_export(&snapshot, &env);

        // Build and serialize to JSON ("plaintext" before encryption).
        let mig_snapshot = build_savings_snapshot(migration_export, ExportFormat::Encrypted);
        assert!(mig_snapshot.verify_checksum());
        let plain_bytes = export_to_json(&mig_snapshot).unwrap();

        // Encrypt (base64 encode).
        let encoded = export_to_encrypted_payload(&plain_bytes);
        assert!(!encoded.is_empty());

        // Decrypt (base64 decode).
        let decoded = import_from_encrypted_payload(&encoded).unwrap();
        assert_eq!(decoded, plain_bytes);

        // Re-import and validate.
        let loaded = import_from_json(&decoded).unwrap();
        assert!(loaded.verify_checksum());
        if let SnapshotPayload::SavingsGoals(ref g) = loaded.payload {
            assert_eq!(g.goals[0].target_amount, 500_000);
            assert_eq!(g.goals[0].current_amount, 100_000);
        } else {
            panic!("Expected SavingsGoals payload");
        }
    }

    // -------------------------------------------------------------------------
    // Security: tampered checksum rejected
    // -------------------------------------------------------------------------

    /// E2E: mutating the header checksum after export must fail import validation.
    ///
    /// Security invariant: any post-export mutation is detected by the SHA-256
    /// checksum and causes `validate_for_import` to return `ChecksumMismatch`.
    #[test]
    fn test_e2e_tampered_checksum_fails_import() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SavingsGoalContract);
        let client = SavingsGoalContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);

        client.init();
        client.create_goal(
            &owner,
            &String::from_str(&env, "Security Test"),
            &1_000i128,
            &2_000_000_000u64,
        );

        let snapshot = client.export_snapshot(&owner);
        let migration_export = to_migration_export(&snapshot, &env);
        let mut mig_snapshot = build_savings_snapshot(migration_export, ExportFormat::Json);

        assert!(mig_snapshot.verify_checksum(), "fresh snapshot must be valid");

        // Tamper.
        mig_snapshot.header.checksum = "00000000000000000000000000000000".into();

        assert!(!mig_snapshot.verify_checksum());
        assert_eq!(
            mig_snapshot.validate_for_import(),
            Err(MigrationError::ChecksumMismatch)
        );
    }

    // -------------------------------------------------------------------------
    // Security: incompatible version rejected
    // -------------------------------------------------------------------------

    /// E2E: setting schema version below `MIN_SUPPORTED_VERSION` must cause
    /// `validate_for_import` to return `IncompatibleVersion`.
    #[test]
    fn test_e2e_incompatible_version_fails_import() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SavingsGoalContract);
        let client = SavingsGoalContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);

        client.init();
        client.create_goal(
            &owner,
            &String::from_str(&env, "Version Test"),
            &500i128,
            &2_000_000_000u64,
        );

        let snapshot = client.export_snapshot(&owner);
        let migration_export = to_migration_export(&snapshot, &env);
        let mut mig_snapshot = build_savings_snapshot(migration_export, ExportFormat::Json);

        mig_snapshot.header.version = 0; // unsupported

        assert!(matches!(
            mig_snapshot.validate_for_import(),
            Err(MigrationError::IncompatibleVersion { found: 0, .. })
        ));
    }

    // -------------------------------------------------------------------------
    // Edge case: empty contract state
    // -------------------------------------------------------------------------

    /// E2E: exporting a contract with zero goals must produce a valid empty snapshot
    /// that survives the JSON roundtrip.
    #[test]
    fn test_e2e_empty_contract_export_json_roundtrip() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SavingsGoalContract);
        let client = SavingsGoalContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);

        client.init();

        // Export with no goals created.
        let snapshot = client.export_snapshot(&owner);
        assert_eq!(snapshot.goals.len(), 0);

        let migration_export = to_migration_export(&snapshot, &env);
        assert_eq!(migration_export.goals.len(), 0);

        let mig_snapshot = build_savings_snapshot(migration_export, ExportFormat::Json);
        assert!(mig_snapshot.verify_checksum());

        let bytes = export_to_json(&mig_snapshot).unwrap();
        let loaded = import_from_json(&bytes).unwrap();
        assert!(loaded.verify_checksum());

        if let SnapshotPayload::SavingsGoals(ref g) = loaded.payload {
            assert_eq!(g.goals.len(), 0);
        } else {
            panic!("Expected SavingsGoals payload");
        }
    }

    // -------------------------------------------------------------------------
    // Edge case: locked goal preserved through migration
    // -------------------------------------------------------------------------

    /// E2E: a goal with `locked: true` must have its locked flag faithfully
    /// preserved through the full export → JSON → import pipeline.
    ///
    /// Validates that the `locked` field survives the contract-to-migration
    /// struct conversion and the JSON serialization layer.
    #[test]
    fn test_e2e_locked_goal_preserved_through_migration() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SavingsGoalContract);
        let client = SavingsGoalContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);

        client.init();
        let goal_id = client.create_goal(
            &owner,
            &String::from_str(&env, "Locked Goal"),
            &10_000i128,
            &2_000_000_000u64,
        );
        client.add_to_goal(&owner, &goal_id, &5_000i128);
        // Goal is created locked by default; verify it is still locked.
        let goal = client.get_goal(&goal_id).unwrap();
        assert!(goal.locked, "goal must be locked after create_goal");

        // Export and convert.
        let snapshot = client.export_snapshot(&owner);
        let migration_export = to_migration_export(&snapshot, &env);
        assert!(
            migration_export.goals[0].locked,
            "locked flag must survive contract → migration conversion"
        );

        // Roundtrip through JSON.
        let mig_snapshot = build_savings_snapshot(migration_export, ExportFormat::Json);
        let bytes = export_to_json(&mig_snapshot).unwrap();
        let loaded = import_from_json(&bytes).unwrap();

        if let SnapshotPayload::SavingsGoals(ref g) = loaded.payload {
            assert!(
                g.goals[0].locked,
                "locked flag must be true after JSON roundtrip"
            );
        } else {
            panic!("Expected SavingsGoals payload");
        }
    }

    // -------------------------------------------------------------------------
    // Determinism: same state → same checksum
    // -------------------------------------------------------------------------

    /// E2E: exporting the same contract state twice and building migration
    /// snapshots from both must yield identical checksums.
    #[test]
    fn test_e2e_snapshot_checksum_is_stable() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SavingsGoalContract);
        let client = SavingsGoalContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);

        client.init();
        let goal_id = client.create_goal(
            &owner,
            &String::from_str(&env, "Stable"),
            &7_000i128,
            &2_000_000_000u64,
        );
        client.add_to_goal(&owner, &goal_id, &2_000i128);

        // Export twice.
        let snap_a = client.export_snapshot(&owner);
        let snap_b = client.export_snapshot(&owner);

        let mig_a = build_savings_snapshot(to_migration_export(&snap_a, &env), ExportFormat::Json);
        let mig_b = build_savings_snapshot(to_migration_export(&snap_b, &env), ExportFormat::Json);

        assert_eq!(
            mig_a.header.checksum, mig_b.header.checksum,
            "same contract state must produce deterministic checksums"
        );
    }

    // -------------------------------------------------------------------------
    // Multi-goal, multi-owner export
    // -------------------------------------------------------------------------

    /// E2E: export goals from two separate contract owners, then roundtrip via
    /// JSON — all goals and owner IDs must be preserved.
    #[test]
    fn test_e2e_multi_owner_export_import_json_roundtrip() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SavingsGoalContract);
        let client = SavingsGoalContractClient::new(&env, &contract_id);
        let owner_a = Address::generate(&env);
        let owner_b = Address::generate(&env);

        client.init();

        // Create goals for owner A.
        let a1 = client.create_goal(
            &owner_a,
            &String::from_str(&env, "A Car"),
            &30_000i128,
            &2_000_000_000u64,
        );
        client.add_to_goal(&owner_a, &a1, &10_000i128);

        // Create goals for owner B.
        let b1 = client.create_goal(
            &owner_b,
            &String::from_str(&env, "B Education"),
            &50_000i128,
            &2_000_000_000u64,
        );
        client.add_to_goal(&owner_b, &b1, &15_000i128);

        // Export full contract state via owner A's call.
        // `export_snapshot` returns ALL goals (not filtered by caller).
        let snapshot = client.export_snapshot(&owner_a);
        assert_eq!(snapshot.goals.len(), 2, "both owners' goals must appear in snapshot");

        let migration_export = to_migration_export(&snapshot, &env);
        let mig_snapshot = build_savings_snapshot(migration_export, ExportFormat::Json);
        assert!(mig_snapshot.verify_checksum());

        let bytes = export_to_json(&mig_snapshot).unwrap();
        let loaded = import_from_json(&bytes).unwrap();
        assert!(loaded.verify_checksum());

        if let SnapshotPayload::SavingsGoals(ref g) = loaded.payload {
            assert_eq!(g.goals.len(), 2);

            let ga = g.goals.iter().find(|g| g.id == 1).expect("goal 1");
            let gb = g.goals.iter().find(|g| g.id == 2).expect("goal 2");

            assert_eq!(ga.target_amount, 30_000);
            assert_eq!(ga.current_amount, 10_000);
            assert_eq!(gb.target_amount, 50_000);
            assert_eq!(gb.current_amount, 15_000);
        } else {
            panic!("Expected SavingsGoals payload");
        }
    }
}

