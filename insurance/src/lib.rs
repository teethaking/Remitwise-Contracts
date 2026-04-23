#![no_std]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use remitwise_common::CoverageType;
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, Map, String, Symbol, Vec,
};

// Storage TTL constants
const INSTANCE_LIFETIME_THRESHOLD: u32 = 17_280; // ~1 day
const INSTANCE_BUMP_AMOUNT: u32 = 518_400; // ~30 days

// Pagination constants (used by tests)
pub const DEFAULT_PAGE_LIMIT: u32 = 20;
pub const MAX_PAGE_LIMIT: u32 = 50;

// Storage keys
const KEY_PAUSE_ADMIN: Symbol = symbol_short!("PAUSE_ADM");
const KEY_NEXT_ID: Symbol = symbol_short!("NEXT_ID");
const KEY_POLICIES: Symbol = symbol_short!("POLICIES");
const KEY_OWNER_INDEX: Symbol = symbol_short!("OWN_IDX");
const KEY_ARCHIVED_POLICIES: Symbol = symbol_short!("ARCH_POL");
const KEY_EXTERNAL_REF_INDEX: Symbol = symbol_short!("EXT_IDX");

const MAX_EXTERNAL_REF_LEN: u32 = 128;

const EVENT_EXTERNAL_REF_UPDATED: Symbol = symbol_short!("ext_ref");

#[contracttype]
#[derive(Clone)]
pub struct ExternalRefUpdatedEvent {
    pub policy_id: u32,
    pub owner: Address,
    pub external_ref: Option<String>,
}

#[contracttype]
#[derive(Clone)]
pub struct InsurancePolicy {
    pub id: u32,
    pub owner: Address,
    pub name: String,
    pub external_ref: Option<String>,
    pub coverage_type: CoverageType,
    pub monthly_premium: i128,
    pub coverage_amount: i128,
    pub active: bool,
    pub next_payment_date: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct PolicyPage {
    pub items: Vec<InsurancePolicy>,
    pub next_cursor: u32,
    pub count: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct ArchivedPolicy {
    pub id: u32,
    pub owner: Address,
    pub name: String,
    pub external_ref: Option<String>,
    pub coverage_type: CoverageType,
    pub monthly_premium: i128,
    pub coverage_amount: i128,
    pub next_payment_date: u64,
}

#[contract]
pub struct Insurance;

#[contractimpl]
impl Insurance {
    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    fn clamp_limit(limit: u32) -> u32 {
        if limit == 0 {
            DEFAULT_PAGE_LIMIT
        } else if limit > MAX_PAGE_LIMIT {
            MAX_PAGE_LIMIT
        } else {
            limit
        }
    }

    fn is_allowed_external_ref_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':')
    }

    fn validate_external_ref(external_ref: &String) {
        let len = external_ref.len();
        assert!(
            len > 0 && len <= MAX_EXTERNAL_REF_LEN,
            "invalid external_ref length"
        );

        let copy_len = len as usize;
        let mut buf = [0u8; MAX_EXTERNAL_REF_LEN as usize];
        external_ref.copy_into_slice(&mut buf[..copy_len]);
        let valid = buf[..copy_len]
            .iter()
            .all(|&b| Self::is_allowed_external_ref_byte(b));
        assert!(valid, "invalid external_ref charset");
    }

    fn get_external_ref_index(env: &Env) -> Map<(Address, String), u32> {
        env.storage()
            .instance()
            .get(&KEY_EXTERNAL_REF_INDEX)
            .unwrap_or_else(|| Map::new(env))
    }

    fn save_external_ref_index(env: &Env, index: &Map<(Address, String), u32>) {
        env.storage().instance().set(&KEY_EXTERNAL_REF_INDEX, index);
    }

    fn ensure_external_ref_available(
        env: &Env,
        owner: &Address,
        external_ref: &String,
        current_policy_id: Option<u32>,
    ) {
        let index = Self::get_external_ref_index(env);
        if let Some(existing_id) = index.get((owner.clone(), external_ref.clone())) {
            assert!(
                current_policy_id == Some(existing_id),
                "external_ref already in use for owner"
            );
        }
    }

    fn bind_external_ref(
        env: &Env,
        owner: &Address,
        policy_id: u32,
        external_ref: &Option<String>,
    ) {
        if let Some(ref_value) = external_ref.clone() {
            Self::validate_external_ref(&ref_value);
            Self::ensure_external_ref_available(env, owner, &ref_value, Some(policy_id));
            let mut index = Self::get_external_ref_index(env);
            index.set((owner.clone(), ref_value), policy_id);
            Self::save_external_ref_index(env, &index);
        }
    }

    fn unbind_external_ref(
        env: &Env,
        owner: &Address,
        policy_id: u32,
        external_ref: &Option<String>,
    ) {
        if let Some(ref_value) = external_ref.clone() {
            let mut index = Self::get_external_ref_index(env);
            if let Some(existing_id) = index.get((owner.clone(), ref_value.clone())) {
                if existing_id == policy_id {
                    index.remove((owner.clone(), ref_value));
                    Self::save_external_ref_index(env, &index);
                }
            }
        }
    }

    pub fn set_pause_admin(env: Env, caller: Address, new_admin: Address) -> bool {
        caller.require_auth();
        Self::extend_instance_ttl(&env);
        env.storage().instance().set(&KEY_PAUSE_ADMIN, &new_admin);
        true
    }

    pub fn create_policy(
        env: Env,
        owner: Address,
        name: String,
        coverage_type: CoverageType,
        monthly_premium: i128,
        coverage_amount: i128,
        external_ref: Option<String>,
    ) -> u32 {
        owner.require_auth();
        Self::extend_instance_ttl(&env);

        if let Some(ref_value) = external_ref.clone() {
            Self::validate_external_ref(&ref_value);
            Self::ensure_external_ref_available(&env, &owner, &ref_value, None);
        }

        let mut next_id: u32 = env.storage().instance().get(&KEY_NEXT_ID).unwrap_or(0);
        next_id += 1;

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&KEY_POLICIES)
            .unwrap_or_else(|| Map::new(&env));

        let policy = InsurancePolicy {
            id: next_id,
            owner: owner.clone(),
            name,
            external_ref: external_ref.clone(),
            coverage_type,
            monthly_premium,
            coverage_amount,
            active: true,
            next_payment_date: env.ledger().timestamp() + (30 * 86_400),
        };
        policies.set(next_id, policy);
        env.storage().instance().set(&KEY_POLICIES, &policies);

        Self::bind_external_ref(&env, &owner, next_id, &external_ref);

        let mut index: Map<Address, Vec<u32>> = env
            .storage()
            .instance()
            .get(&KEY_OWNER_INDEX)
            .unwrap_or_else(|| Map::new(&env));
        let mut ids = index.get(owner.clone()).unwrap_or_else(|| Vec::new(&env));
        ids.push_back(next_id);
        index.set(owner, ids);
        env.storage().instance().set(&KEY_OWNER_INDEX, &index);

        env.storage().instance().set(&KEY_NEXT_ID, &next_id);
        next_id
    }

    pub fn get_policy(env: Env, policy_id: u32) -> Option<InsurancePolicy> {
        Self::extend_instance_ttl(&env);
        let policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&KEY_POLICIES)
            .unwrap_or_else(|| Map::new(&env));
        policies.get(policy_id)
    }

    pub fn deactivate_policy(env: Env, caller: Address, policy_id: u32) -> bool {
        caller.require_auth();
        Self::extend_instance_ttl(&env);

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&KEY_POLICIES)
            .unwrap_or_else(|| Map::new(&env));
        let mut policy = match policies.get(policy_id) {
            Some(p) => p,
            None => return false,
        };
        if policy.owner != caller {
            return false;
        }

        if !policy.active {
            return false;
        }

        Self::unbind_external_ref(&env, &policy.owner, policy_id, &policy.external_ref);
        policy.active = false;
        policies.set(policy_id, policy);
        env.storage().instance().set(&KEY_POLICIES, &policies);
        true
    }

    pub fn set_external_ref(
        env: Env,
        caller: Address,
        policy_id: u32,
        external_ref: Option<String>,
    ) -> bool {
        caller.require_auth();
        Self::extend_instance_ttl(&env);

        if let Some(ref_value) = external_ref.clone() {
            Self::validate_external_ref(&ref_value);
        }

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&KEY_POLICIES)
            .unwrap_or_else(|| Map::new(&env));
        let mut policy = match policies.get(policy_id) {
            Some(p) => p,
            None => return false,
        };

        if policy.owner != caller {
            return false;
        }

        let old_external_ref = policy.external_ref.clone();

        if external_ref == old_external_ref {
            return true;
        }

        if let Some(new_ref) = external_ref.clone() {
            Self::ensure_external_ref_available(&env, &policy.owner, &new_ref, Some(policy_id));
        }

        Self::unbind_external_ref(&env, &policy.owner, policy_id, &old_external_ref);
        Self::bind_external_ref(&env, &policy.owner, policy_id, &external_ref);

        policy.external_ref = external_ref.clone();
        policies.set(policy_id, policy);
        env.storage().instance().set(&KEY_POLICIES, &policies);

        env.events().publish(
            (EVENT_EXTERNAL_REF_UPDATED,),
            ExternalRefUpdatedEvent {
                policy_id,
                owner: caller,
                external_ref,
            },
        );

        true
    }

    pub fn archive_policy(env: Env, caller: Address, policy_id: u32) -> bool {
        caller.require_auth();
        Self::extend_instance_ttl(&env);

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&KEY_POLICIES)
            .unwrap_or_else(|| Map::new(&env));
        let mut archived: Map<u32, ArchivedPolicy> = env
            .storage()
            .instance()
            .get(&KEY_ARCHIVED_POLICIES)
            .unwrap_or_else(|| Map::new(&env));

        let policy = match policies.get(policy_id) {
            Some(p) => p,
            None => return false,
        };
        if policy.owner != caller {
            return false;
        }

        Self::unbind_external_ref(&env, &policy.owner, policy_id, &policy.external_ref);

        archived.set(
            policy_id,
            ArchivedPolicy {
                id: policy.id,
                owner: policy.owner,
                name: policy.name,
                external_ref: policy.external_ref,
                coverage_type: policy.coverage_type,
                monthly_premium: policy.monthly_premium,
                coverage_amount: policy.coverage_amount,
                next_payment_date: policy.next_payment_date,
            },
        );
        policies.remove(policy_id);

        env.storage().instance().set(&KEY_POLICIES, &policies);
        env.storage()
            .instance()
            .set(&KEY_ARCHIVED_POLICIES, &archived);
        true
    }

    pub fn restore_policy(env: Env, caller: Address, policy_id: u32) -> bool {
        caller.require_auth();
        Self::extend_instance_ttl(&env);

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&KEY_POLICIES)
            .unwrap_or_else(|| Map::new(&env));
        let mut archived: Map<u32, ArchivedPolicy> = env
            .storage()
            .instance()
            .get(&KEY_ARCHIVED_POLICIES)
            .unwrap_or_else(|| Map::new(&env));

        let archived_policy = match archived.get(policy_id) {
            Some(p) => p,
            None => return false,
        };
        if archived_policy.owner != caller {
            return false;
        }

        if let Some(ref_value) = archived_policy.external_ref.clone() {
            let index = Self::get_external_ref_index(&env);
            if let Some(existing_id) = index.get((archived_policy.owner.clone(), ref_value)) {
                if existing_id != policy_id {
                    return false;
                }
            }
        }

        Self::bind_external_ref(
            &env,
            &archived_policy.owner,
            policy_id,
            &archived_policy.external_ref,
        );

        policies.set(
            policy_id,
            InsurancePolicy {
                id: archived_policy.id,
                owner: archived_policy.owner,
                name: archived_policy.name,
                external_ref: archived_policy.external_ref,
                coverage_type: archived_policy.coverage_type,
                monthly_premium: archived_policy.monthly_premium,
                coverage_amount: archived_policy.coverage_amount,
                active: true,
                next_payment_date: archived_policy.next_payment_date,
            },
        );
        archived.remove(policy_id);

        env.storage().instance().set(&KEY_POLICIES, &policies);
        env.storage()
            .instance()
            .set(&KEY_ARCHIVED_POLICIES, &archived);
        true
    }

    pub fn get_archived_policy(env: Env, policy_id: u32) -> Option<ArchivedPolicy> {
        Self::extend_instance_ttl(&env);
        let archived: Map<u32, ArchivedPolicy> = env
            .storage()
            .instance()
            .get(&KEY_ARCHIVED_POLICIES)
            .unwrap_or_else(|| Map::new(&env));
        archived.get(policy_id)
    }

    pub fn get_policy_id_by_external_ref(
        env: Env,
        owner: Address,
        external_ref: String,
    ) -> Option<u32> {
        Self::extend_instance_ttl(&env);

        let len = external_ref.len();
        if len == 0 || len > MAX_EXTERNAL_REF_LEN {
            return None;
        }
        let copy_len = len as usize;
        let mut buf = [0u8; MAX_EXTERNAL_REF_LEN as usize];
        external_ref.copy_into_slice(&mut buf[..copy_len]);
        if !buf[..copy_len]
            .iter()
            .all(|&b| Self::is_allowed_external_ref_byte(b))
        {
            return None;
        }

        let index = Self::get_external_ref_index(&env);
        index.get((owner, external_ref))
    }

    pub fn pay_premium(env: Env, caller: Address, policy_id: u32) -> bool {
        caller.require_auth();
        Self::extend_instance_ttl(&env);

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&KEY_POLICIES)
            .unwrap_or_else(|| Map::new(&env));
        let mut policy = match policies.get(policy_id) {
            Some(p) => p,
            None => return false,
        };
        if policy.owner != caller || !policy.active {
            return false;
        }
        policy.next_payment_date = env.ledger().timestamp() + (30 * 86_400);
        policies.set(policy_id, policy);
        env.storage().instance().set(&KEY_POLICIES, &policies);
        true
    }

    pub fn batch_pay_premiums(env: Env, caller: Address, policy_ids: Vec<u32>) -> u32 {
        caller.require_auth();
        Self::extend_instance_ttl(&env);

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&KEY_POLICIES)
            .unwrap_or_else(|| Map::new(&env));

        let mut count: u32 = 0;
        let next_date = env.ledger().timestamp() + (30 * 86_400);
        for id in policy_ids.iter() {
            if let Some(mut p) = policies.get(id) {
                if p.owner == caller && p.active {
                    p.next_payment_date = next_date;
                    policies.set(id, p);
                    count += 1;
                }
            }
        }
        env.storage().instance().set(&KEY_POLICIES, &policies);
        count
    }

    pub fn get_total_monthly_premium(env: Env, owner: Address) -> i128 {
        Self::extend_instance_ttl(&env);

        let policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&KEY_POLICIES)
            .unwrap_or_else(|| Map::new(&env));
        let index: Map<Address, Vec<u32>> = env
            .storage()
            .instance()
            .get(&KEY_OWNER_INDEX)
            .unwrap_or_else(|| Map::new(&env));

        let ids = index.get(owner).unwrap_or_else(|| Vec::new(&env));
        let mut total: i128 = 0;
        for id in ids.iter() {
            if let Some(p) = policies.get(id) {
                if p.active {
                    total += p.monthly_premium;
                }
            }
        }
        total
    }

    /// Returns a stable, cursor-based page of active policies for an owner.
    pub fn get_active_policies(env: Env, owner: Address, cursor: u32, limit: u32) -> PolicyPage {
        Self::extend_instance_ttl(&env);
        let limit = Self::clamp_limit(limit);

        let policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&KEY_POLICIES)
            .unwrap_or_else(|| Map::new(&env));
        let index: Map<Address, Vec<u32>> = env
            .storage()
            .instance()
            .get(&KEY_OWNER_INDEX)
            .unwrap_or_else(|| Map::new(&env));
        let ids = index.get(owner).unwrap_or_else(|| Vec::new(&env));

        let mut items: Vec<InsurancePolicy> = Vec::new(&env);
        let mut next_cursor: u32 = 0;

        for id in ids.iter() {
            if id <= cursor {
                continue;
            }
            if let Some(p) = policies.get(id) {
                if !p.active {
                    continue;
                }
                items.push_back(p);
                next_cursor = id;
                if items.len() >= limit {
                    break;
                }
            }
        }

        // If we returned a full page, we may or may not have more items;
        // keep the cursor as the last returned id (caller can continue).
        // If we returned less than a full page, no more data -> cursor 0.
        let out_cursor = if items.len() < limit { 0 } else { next_cursor };

        let count = items.len();
        PolicyPage {
            items,
            next_cursor: out_cursor,
            count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Insurance, InsuranceClient};
    use remitwise_common::CoverageType;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Address, Env, String};

    fn setup() -> (Env, InsuranceClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, Insurance);
        let client = InsuranceClient::new(&env, &contract_id);
        let owner_a = Address::generate(&env);
        let owner_b = Address::generate(&env);
        (env, client, owner_a, owner_b)
    }

    fn policy_name(env: &Env) -> String {
        String::from_str(env, "Policy")
    }

    fn ext(env: &Env, v: &str) -> Option<String> {
        Some(String::from_str(env, v))
    }

    #[test]
    #[should_panic(expected = "external_ref already in use for owner")]
    fn duplicate_external_ref_same_owner_panics() {
        let (env, client, owner, _) = setup();
        let name = policy_name(&env);
        client.create_policy(
            &owner,
            &name,
            &CoverageType::Health,
            &100,
            &10_000,
            &ext(&env, "INV-001"),
        );
        client.create_policy(
            &owner,
            &name,
            &CoverageType::Life,
            &200,
            &20_000,
            &ext(&env, "INV-001"),
        );
    }

    #[test]
    fn duplicate_external_ref_different_owners_allowed() {
        let (env, client, owner_a, owner_b) = setup();
        let name = policy_name(&env);

        let id_a = client.create_policy(
            &owner_a,
            &name,
            &CoverageType::Health,
            &100,
            &10_000,
            &ext(&env, "INV-001"),
        );
        let id_b = client.create_policy(
            &owner_b,
            &name,
            &CoverageType::Health,
            &100,
            &10_000,
            &ext(&env, "INV-001"),
        );

        assert_eq!(
            client.get_policy_id_by_external_ref(&owner_a, &String::from_str(&env, "INV-001")),
            Some(id_a)
        );
        assert_eq!(
            client.get_policy_id_by_external_ref(&owner_b, &String::from_str(&env, "INV-001")),
            Some(id_b)
        );
    }

    #[test]
    #[should_panic(expected = "invalid external_ref charset")]
    fn invalid_external_ref_charset_panics() {
        let (env, client, owner, _) = setup();
        let name = policy_name(&env);
        client.create_policy(
            &owner,
            &name,
            &CoverageType::Health,
            &100,
            &10_000,
            &ext(&env, "bad ref"),
        );
    }

    #[test]
    fn clearing_external_ref_allows_reuse() {
        let (env, client, owner, _) = setup();
        let name = policy_name(&env);

        let id1 = client.create_policy(
            &owner,
            &name,
            &CoverageType::Health,
            &100,
            &10_000,
            &ext(&env, "INV-002"),
        );

        assert!(client.set_external_ref(&owner, &id1, &None));
        assert_eq!(
            client.get_policy_id_by_external_ref(&owner, &String::from_str(&env, "INV-002")),
            None
        );

        let id2 = client.create_policy(
            &owner,
            &name,
            &CoverageType::Life,
            &100,
            &10_000,
            &ext(&env, "INV-002"),
        );
        assert_eq!(
            client.get_policy_id_by_external_ref(&owner, &String::from_str(&env, "INV-002")),
            Some(id2)
        );
    }

    #[test]
    fn deactivate_clears_external_ref_mapping_allows_reuse() {
        let (env, client, owner, _) = setup();
        let name = policy_name(&env);

        let id1 = client.create_policy(
            &owner,
            &name,
            &CoverageType::Health,
            &100,
            &10_000,
            &ext(&env, "INV-003"),
        );
        assert!(client.deactivate_policy(&owner, &id1));

        let id2 = client.create_policy(
            &owner,
            &name,
            &CoverageType::Property,
            &100,
            &10_000,
            &ext(&env, "INV-003"),
        );
        assert_eq!(
            client.get_policy_id_by_external_ref(&owner, &String::from_str(&env, "INV-003")),
            Some(id2)
        );
    }

    #[test]
    fn restore_with_conflicting_external_ref_fails_closed() {
        let (env, client, owner, _) = setup();
        let name = policy_name(&env);

        let id1 = client.create_policy(
            &owner,
            &name,
            &CoverageType::Health,
            &100,
            &10_000,
            &ext(&env, "INV-004"),
        );
        assert!(client.archive_policy(&owner, &id1));

        let id2 = client.create_policy(
            &owner,
            &name,
            &CoverageType::Auto,
            &120,
            &12_000,
            &ext(&env, "INV-004"),
        );

        assert!(!client.restore_policy(&owner, &id1));
        assert_eq!(
            client.get_policy_id_by_external_ref(&owner, &String::from_str(&env, "INV-004")),
            Some(id2)
        );
        assert!(client.get_archived_policy(&id1).is_some());
    }
}
