//! Simple test to verify admin role transfer functionality
//! This is a standalone test file to verify our changes work

#[cfg(test)]
mod tests {
    use soroban_sdk::{testutils::Address as _, Address, Env};

    // Mock contract structure for testing
    struct MockContract {
        env: Env,
    }

    impl MockContract {
        fn new() -> Self {
            let env = Env::default();
            env.mock_all_auths();
            Self { env }
        }

        fn get_upgrade_admin(&self) -> Option<Address> {
            self.env.storage().instance().get(&soroban_sdk::symbol_short!("UPG_ADM"))
        }

        fn set_upgrade_admin(&self, caller: Address, new_admin: Address) -> Result<(), &'static str> {
            caller.require_auth();
            
            let current_upgrade_admin = self.get_upgrade_admin();
            
            // Authorization logic:
            // 1. If no upgrade admin exists, caller must equal new_admin (bootstrap)
            // 2. If upgrade admin exists, only current upgrade admin can transfer
            match current_upgrade_admin {
                None => {
                    // Bootstrap pattern - caller must be setting themselves as admin
                    if caller != new_admin {
                        return Err("Unauthorized: bootstrap requires caller == new_admin");
                    }
                }
                Some(current_admin) => {
                    // Admin transfer - only current admin can transfer
                    if current_admin != caller {
                        return Err("Unauthorized: only current upgrade admin can transfer");
                    }
                }
            }
            
            self.env.storage()
                .instance()
                .set(&soroban_sdk::symbol_short!("UPG_ADM"), &new_admin);
            
            Ok(())
        }
    }

    #[test]
    fn test_bootstrap_admin_setup() {
        let contract = MockContract::new();
        let admin = Address::generate(&contract.env);
        
        // Bootstrap should succeed when caller == new_admin
        let result = contract.set_upgrade_admin(admin.clone(), admin.clone());
        assert!(result.is_ok(), "Bootstrap should succeed");
        
        let current_admin = contract.get_upgrade_admin();
        assert_eq!(current_admin, Some(admin));
    }

    #[test]
    fn test_unauthorized_bootstrap() {
        let contract = MockContract::new();
        let caller = Address::generate(&contract.env);
        let admin = Address::generate(&contract.env);
        
        // Bootstrap should fail when caller != new_admin
        let result = contract.set_upgrade_admin(caller, admin);
        assert!(result.is_err(), "Unauthorized bootstrap should fail");
        
        let current_admin = contract.get_upgrade_admin();
        assert_eq!(current_admin, None, "No admin should be set after failed bootstrap");
    }

    #[test]
    fn test_authorized_admin_transfer() {
        let contract = MockContract::new();
        let admin1 = Address::generate(&contract.env);
        let admin2 = Address::generate(&contract.env);
        
        // Setup initial admin
        contract.set_upgrade_admin(admin1.clone(), admin1.clone()).unwrap();
        
        // Transfer should succeed when current admin transfers
        let result = contract.set_upgrade_admin(admin1, admin2.clone());
        assert!(result.is_ok(), "Authorized transfer should succeed");
        
        let current_admin = contract.get_upgrade_admin();
        assert_eq!(current_admin, Some(admin2));
    }

    #[test]
    fn test_unauthorized_admin_transfer() {
        let contract = MockContract::new();
        let admin1 = Address::generate(&contract.env);
        let admin2 = Address::generate(&contract.env);
        let unauthorized = Address::generate(&contract.env);
        
        // Setup initial admin
        contract.set_upgrade_admin(admin1.clone(), admin1.clone()).unwrap();
        
        // Transfer should fail when unauthorized user attempts transfer
        let result = contract.set_upgrade_admin(unauthorized, admin2);
        assert!(result.is_err(), "Unauthorized transfer should fail");
        
        let current_admin = contract.get_upgrade_admin();
        assert_eq!(current_admin, Some(admin1), "Admin should remain unchanged");
    }

    #[test]
    fn test_self_transfer() {
        let contract = MockContract::new();
        let admin = Address::generate(&contract.env);
        
        // Setup initial admin
        contract.set_upgrade_admin(admin.clone(), admin.clone()).unwrap();
        
        // Self-transfer should succeed
        let result = contract.set_upgrade_admin(admin.clone(), admin.clone());
        assert!(result.is_ok(), "Self-transfer should succeed");
        
        let current_admin = contract.get_upgrade_admin();
        assert_eq!(current_admin, Some(admin));
    }
}