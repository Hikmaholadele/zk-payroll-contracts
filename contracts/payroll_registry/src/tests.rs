use super::*;
use soroban_sdk::testutils::{Address as _, Events};
use soroban_sdk::{Env, IntoVal, String, Symbol, TryIntoVal, Val, Vec};

const VALID_EMPLOYEE_WALLET: &str = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";
const BAD_CHECKSUM_EMPLOYEE_WALLET: &str =
    "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHA";

fn setup() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PayrollRegistry);
    (env, contract_id)
}

fn setup_no_auth_mock() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register_contract(None, PayrollRegistry);
    (env, contract_id)
}

/// Number of contract events recorded so far, in publication order.
///
/// `Env::events().all()` returns the XDR-backed `ContractEvents` type, so this
/// is the supported way to count them again.
fn event_count(env: &Env) -> usize {
    env.events().all().events().len()
}

/// Topics of the contract event at `index`, in publication order.
fn event_topics(env: &Env, index: usize) -> Vec<Val> {
    let recorded = env.events().all();
    let event = &recorded.events()[index];
    let body = match &event.body {
        soroban_sdk::xdr::ContractEventBody::V0(v0) => v0,
    };
    let topics: alloc::vec::Vec<Val> = body
        .topics
        .iter()
        .map(|topic| {
            topic
                .clone()
                .try_into_val(env)
                .expect("event topic must decode to a host value")
        })
        .collect();
    Vec::from_slice(env, &topics)
}

#[test]
fn test_register_company_returns_sequential_ids() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin0 = Address::generate(&env);
    let admin1 = Address::generate(&env);
    let treasury = Address::generate(&env);

    let id0 = client.register_company(&admin0, &treasury);
    let id1 = client.register_company(&admin1, &treasury);

    assert_eq!(id0, 0u64);
    assert_eq!(id1, 1u64);
}

#[test]
fn test_register_company_requires_admin_auth() {
    let (env, contract_id) = setup_no_auth_mock();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let result = client.try_register_company(&admin, &treasury);
    assert!(result.is_err());
}

#[test]
fn test_register_company_updates_company_sequence() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin0 = Address::generate(&env);
    let admin1 = Address::generate(&env);
    let treasury = Address::generate(&env);

    client.register_company(&admin0, &treasury);
    client.register_company(&admin1, &treasury);

    let seq: u64 = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::CompanySequence)
            .expect("company sequence should be stored")
    });
    assert_eq!(seq, 2u64);
}

#[test]
fn test_add_employee_stores_commitment() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);

    let stored: BytesN<32> = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::Employee(company_id, employee))
            .expect("employee commitment should be stored")
    });
    assert_eq!(stored, commitment);
}

#[test]
fn test_validate_employee_wallet_format_accepts_valid_account_strkey() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let wallet = String::from_str(&env, VALID_EMPLOYEE_WALLET);

    assert!(client.validate_employee_wallet_format(&wallet));
}

#[test]
fn test_validate_employee_wallet_format_rejects_invalid_wallets() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let short_wallet = String::from_str(&env, "GSHORT");
    let bad_checksum_wallet = String::from_str(&env, BAD_CHECKSUM_EMPLOYEE_WALLET);
    let contract_strkey = Address::generate(&env).to_string();

    assert!(!client.validate_employee_wallet_format(&short_wallet));
    assert!(!client.validate_employee_wallet_format(&bad_checksum_wallet));
    assert!(!client.validate_employee_wallet_format(&contract_strkey));
}

#[test]
fn test_add_employee_by_wallet_stores_commitment() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let wallet = String::from_str(&env, VALID_EMPLOYEE_WALLET);
    let employee = Address::from_string(&wallet);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee_by_wallet(&company_id, &wallet, &commitment);

    assert_eq!(client.get_commitment(&company_id, &employee), commitment);
    assert_eq!(
        client.get_employee_status(&company_id, &employee),
        EmployeeStatus::Active,
    );
}

#[test]
fn test_add_employee_by_wallet_rejects_invalid_wallet_before_storage() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let bad_wallet = String::from_str(&env, BAD_CHECKSUM_EMPLOYEE_WALLET);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    let result = client.try_add_employee_by_wallet(&company_id, &bad_wallet, &commitment);

    assert!(result.is_err());
    // A rejected call must not publish any event.
    assert_eq!(event_count(&env), 0);
}

#[test]
fn test_remove_employee_hard_deletes() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[2u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);
    client.remove_employee(&company_id, &employee);

    let new_commitment = BytesN::from_array(&env, &[3u8; 32]);
    let result = client.try_update_commitment(&company_id, &employee, &new_commitment);
    assert!(result.is_err());
}

#[test]
fn test_update_commitment_replaces_value() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let old_commitment = BytesN::from_array(&env, &[1u8; 32]);
    let new_commitment = BytesN::from_array(&env, &[9u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &old_commitment);
    client.update_commitment(&company_id, &employee, &new_commitment);

    let stored: BytesN<32> = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::Employee(company_id, employee))
            .expect("employee commitment should be updated")
    });
    assert_eq!(stored, new_commitment);
}

#[test]
fn test_add_employee_unknown_company_panics() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[0u8; 32]);

    let result = client.try_add_employee(&99u64, &employee, &commitment);
    assert!(result.is_err());
}

#[test]
fn test_register_company_stores_company_info() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);

    let stored: CompanyInfo = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::Company(company_id))
            .expect("company info should be stored")
    });
    assert_eq!(stored.admin, admin);
    assert_eq!(stored.treasury, treasury);
}

/// Acceptance Criteria: Authorization (Access Control)
/// - Attempt to call add_employee using a keypair that is not the registered HR Admin.
/// - Assert Panic.
#[test]
#[should_panic(expected = "authorized")]
fn test_authorization_add_employee_fails_for_non_admin() {
    let env = Env::default();

    // We intentionally do NOT mock_all_auths() here, because we want to test that
    // the registry correctly enforces `require_auth` dynamically against the correct admin.

    let contract_id = env.register_contract(None, PayrollRegistry);
    let registry = PayrollRegistryClient::new(&env, &contract_id);

    // Register a company with a specific admin address
    let correct_admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &correct_admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "register_company",
            args: (correct_admin.clone(), treasury.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let company_id = registry.register_company(&correct_admin, &treasury);

    // Provide a random rogue address representing the non-registered user
    let attacker = Address::generate(&env);
    let mock_employee = Address::generate(&env);
    let fake_commitment = BytesN::from_array(&env, &[9u8; 32]);

    // The attacker tries to authorize themselves to act on the contract. Setting mock auths globally
    // to mimic the attacker signing the transaction with their *own* key.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "add_employee",
            args: (company_id, mock_employee.clone(), fake_commitment.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    // Attack: call `add_employee`. The registry calls `info.admin.require_auth()`.
    // The attacker's signature is in the auth list, but it does not match `info.admin` (which is `correct_admin`).
    // Expected: Panic from the Soroban host terminating the execution for a missing correct signature.
    registry.add_employee(&company_id, &mock_employee, &fake_commitment);
}

#[test]
fn test_get_company_returns_company_info() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let company = client.get_company(&company_id);

    assert_eq!(company.admin, admin);
    assert_eq!(company.treasury, treasury);
}

#[test]
fn test_get_commitment_returns_employee_commitment() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[7u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);

    let got = client.get_commitment(&company_id, &employee);
    assert_eq!(got, commitment);
}

// ?? Issue #90: employee eligibility ??????????????????????????????????????????

#[test]
fn test_add_employee_sets_active_status() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);

    assert_eq!(
        client.get_employee_status(&company_id, &employee),
        EmployeeStatus::Active,
    );
    assert!(client.is_eligible(&company_id, &employee));
}

#[test]
fn test_set_employee_status_suspended_makes_ineligible() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[2u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);

    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Suspended);

    assert_eq!(
        client.get_employee_status(&company_id, &employee),
        EmployeeStatus::Suspended,
    );
    assert!(!client.is_eligible(&company_id, &employee));
}

#[test]
fn test_set_employee_status_incomplete_makes_ineligible() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[3u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);

    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Incomplete);

    assert!(!client.is_eligible(&company_id, &employee));
}

#[test]
fn test_unregistered_employee_is_not_eligible() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let stranger = Address::generate(&env);

    assert!(!client.is_eligible(&company_id, &stranger));
}

#[test]
fn test_reactivating_suspended_employee_restores_eligibility() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[4u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);
    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Suspended);
    assert!(!client.is_eligible(&company_id, &employee));

    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Active);
    assert!(client.is_eligible(&company_id, &employee));
}

// ---------------------------------------------------------------------------
// Issue #615: employee eligibility status evaluation
// ---------------------------------------------------------------------------

/// Register a company with one employee and return both, for reuse below.
fn setup_with_employee() -> (Env, PayrollRegistryClient<'static>, u64, Address) {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[7u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);
    (env, client, company_id, employee)
}

#[test]
fn test_evaluate_eligibility_reports_eligible_for_active_employee() {
    let (_env, client, company_id, employee) = setup_with_employee();

    let assessment = client.evaluate_eligibility(&company_id, &employee);

    assert!(assessment.eligible, "an added employee starts Active");
    assert_eq!(assessment.reason, EligibilityReason::Eligible);
    assert_eq!(assessment.status, EmployeeStatus::Active);
}

#[test]
fn test_evaluate_eligibility_reports_unregistered_for_unknown_address() {
    let (env, client, company_id, _employee) = setup_with_employee();
    let stranger = Address::generate(&env);

    let assessment = client.evaluate_eligibility(&company_id, &stranger);

    assert!(!assessment.eligible);
    assert_eq!(assessment.reason, EligibilityReason::Unregistered);
    // Never-set status still reports the documented `Incomplete` default.
    assert_eq!(assessment.status, EmployeeStatus::Incomplete);
}

#[test]
fn test_evaluate_eligibility_reports_suspended_with_remediation() {
    let (_env, client, company_id, employee) = setup_with_employee();
    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Suspended);

    let assessment = client.evaluate_eligibility(&company_id, &employee);

    assert!(!assessment.eligible);
    assert_eq!(assessment.reason, EligibilityReason::Suspended);
    assert_eq!(assessment.status, EmployeeStatus::Suspended);
}

#[test]
fn test_evaluate_eligibility_reports_incomplete() {
    let (_env, client, company_id, employee) = setup_with_employee();
    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Incomplete);

    let assessment = client.evaluate_eligibility(&company_id, &employee);

    assert!(!assessment.eligible);
    assert_eq!(assessment.reason, EligibilityReason::Incomplete);
}

#[test]
fn test_evaluate_eligibility_reports_offboarded() {
    let (_env, client, company_id, employee) = setup_with_employee();
    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Offboarded);

    let assessment = client.evaluate_eligibility(&company_id, &employee);

    assert!(!assessment.eligible);
    assert_eq!(assessment.reason, EligibilityReason::Offboarded);
    assert_eq!(assessment.status, EmployeeStatus::Offboarded);
}

#[test]
fn test_evaluate_eligibility_flags_removed_employee_despite_stale_active_status() {
    // `remove_employee` deletes the record but leaves the status key behind, so
    // a removed employee still reports status `Active`. The record check must
    // still win, otherwise a re-registered stranger could be paid by accident.
    let (_env, client, company_id, employee) = setup_with_employee();
    client.remove_employee(&company_id, &employee);

    let assessment = client.evaluate_eligibility(&company_id, &employee);

    assert_eq!(
        client.get_employee_status(&company_id, &employee),
        EmployeeStatus::Active
    );
    assert!(!assessment.eligible);
    assert_eq!(assessment.reason, EligibilityReason::Unregistered);
}

#[test]
fn test_evaluate_eligibility_agrees_with_is_eligible_for_every_status() {
    let (_env, client, company_id, employee) = setup_with_employee();
    let statuses = [
        EmployeeStatus::Active,
        EmployeeStatus::Suspended,
        EmployeeStatus::Incomplete,
    ];

    for status in statuses {
        client.set_employee_status(&company_id, &employee, &status);

        let assessment = client.evaluate_eligibility(&company_id, &employee);

        assert_eq!(
            assessment.eligible,
            client.is_eligible(&company_id, &employee),
            "eligible flag diverged from is_eligible for status {status:?}"
        );
        assert_eq!(
            assessment.eligible,
            client.is_employee_active(&company_id, &employee),
            "eligible flag diverged from is_employee_active for status {status:?}"
        );
        assert_eq!(
            assessment.eligible,
            assessment.reason == EligibilityReason::Eligible
        );
    }
}

#[test]
fn test_require_eligible_returns_status_for_eligible_employee() {
    let (_env, client, company_id, employee) = setup_with_employee();

    assert_eq!(
        client.require_eligible(&company_id, &employee),
        EmployeeStatus::Active
    );
}

#[test]
#[should_panic(expected = "employee is not registered with this company")]
fn test_require_eligible_rejects_unregistered_employee() {
    let (env, client, company_id, _employee) = setup_with_employee();
    let stranger = Address::generate(&env);

    client.require_eligible(&company_id, &stranger);
}

#[test]
#[should_panic(expected = "employee is suspended")]
fn test_require_eligible_rejects_suspended_employee() {
    let (_env, client, company_id, employee) = setup_with_employee();
    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Suspended);

    client.require_eligible(&company_id, &employee);
}

#[test]
#[should_panic(expected = "employee is offboarded")]
fn test_require_eligible_rejects_offboarded_employee() {
    let (_env, client, company_id, employee) = setup_with_employee();
    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Offboarded);

    client.require_eligible(&company_id, &employee);
}

#[test]
fn test_each_ineligibility_reason_explains_its_remediation() {
    let reasons = [
        EligibilityReason::Eligible,
        EligibilityReason::Unregistered,
        EligibilityReason::Incomplete,
        EligibilityReason::Suspended,
        EligibilityReason::Offboarded,
    ];

    let mut seen: alloc::vec::Vec<&str> = alloc::vec::Vec::new();
    for reason in reasons {
        let message = reason.as_str();
        assert!(!message.is_empty(), "{reason:?} must explain itself");
        assert!(
            !seen.contains(&message),
            "{reason:?} reuses another reason's message"
        );
        seen.push(message);
    }
}

// ---------------------------------------------------------------------------
// Event emission tests
// ---------------------------------------------------------------------------

#[test]
fn test_register_company_emits_event() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let after = event_count(&env);
    assert_eq!(after, 1, "registration must emit exactly one event");

    let topics = event_topics(&env, after - 1);
    assert_eq!(topics.len(), 2);
    let sym0: Symbol = topics.get(0).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(sym0, Symbol::new(&env, "CompanyRegistered"));
    let comp_id: u64 = topics.get(1).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(comp_id, company_id);
}

#[test]
fn test_add_employee_emits_event() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);
    let after = event_count(&env);
    assert_eq!(after, 1);

    let topics = event_topics(&env, after - 1);
    assert_eq!(topics.len(), 3);
    let sym0: Symbol = topics.get(0).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(sym0, Symbol::new(&env, "EmployeeAdded"));
    let comp_id: u64 = topics.get(1).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(comp_id, company_id);
    let emp_addr: Address = topics.get(2).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(emp_addr, employee);
}

#[test]
fn test_remove_employee_emits_event() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);
    client.remove_employee(&company_id, &employee);
    let after = event_count(&env);
    assert_eq!(after, 1, "removal must emit exactly one event");

    let topics = event_topics(&env, after - 1);
    assert_eq!(topics.len(), 3);
    let sym0: Symbol = topics.get(0).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(sym0, Symbol::new(&env, "EmployeeRemoved"));
    let comp_id: u64 = topics.get(1).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(comp_id, company_id);
    let emp_addr: Address = topics.get(2).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(emp_addr, employee);
}

#[test]
fn test_update_commitment_emits_event() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let old_commitment = BytesN::from_array(&env, &[1u8; 32]);
    let new_commitment = BytesN::from_array(&env, &[9u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &old_commitment);
    client.update_commitment(&company_id, &employee, &new_commitment);
    let after = event_count(&env);
    assert_eq!(after, 1, "commitment update must emit exactly one event");

    let topics = event_topics(&env, after - 1);
    assert_eq!(topics.len(), 3);
    let sym0: Symbol = topics.get(0).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(sym0, Symbol::new(&env, "CommitmentUpdated"));
    let comp_id: u64 = topics.get(1).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(comp_id, company_id);
    let emp_addr: Address = topics.get(2).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(emp_addr, employee);
}

#[test]
fn test_deactivate_employee_emits_lifecycle_event() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[5u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);
    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Suspended);
    let after = event_count(&env);
    // The suspension event, then the `EmployeeDeactivated` lifecycle event.
    assert_eq!(
        after, 2,
        "suspension must emit the suspension and lifecycle events"
    );

    let topics = event_topics(&env, after - 1);
    assert_eq!(topics.len(), 3);
    let sym0: Symbol = topics.get(0).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(sym0, Symbol::new(&env, "EmployeeDeactivated"));
    let comp_id: u64 = topics.get(1).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(comp_id, company_id);
    let emp_addr: Address = topics.get(2).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(emp_addr, employee);
}

#[test]
fn test_reactivate_employee_emits_lifecycle_event() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[6u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);
    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Suspended);
    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Active);
    let after = event_count(&env);
    // The activation event, then the `EmployeeReactivated` lifecycle event.
    assert_eq!(
        after, 2,
        "reactivation must emit the activation and lifecycle events"
    );

    let topics = event_topics(&env, after - 1);
    assert_eq!(topics.len(), 3);
    let sym0: Symbol = topics.get(0).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(sym0, Symbol::new(&env, "EmployeeReactivated"));
    let comp_id: u64 = topics.get(1).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(comp_id, company_id);
    let emp_addr: Address = topics.get(2).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(emp_addr, employee);
}

// ?? Issue #91: company admin/treasury rotation ????????????????????????????????

#[test]
fn test_admin_rotation_full_flow() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let new_admin = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &admin, &new_admin);
    client.accept_admin_rotation(&company_id, &new_admin);

    let info = client.get_company(&company_id);
    assert_eq!(info.admin, new_admin);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not the company admin")]
fn test_propose_admin_rotation_rejects_non_admin() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let attacker = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &attacker, &new_admin);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not the proposed admin")]
fn test_accept_admin_rotation_rejects_wrong_address() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let new_admin = Address::generate(&env);
    let impostor = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &admin, &new_admin);
    client.accept_admin_rotation(&company_id, &impostor);
}

#[test]
fn test_cancel_admin_rotation_clears_proposal() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let new_admin = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &admin, &new_admin);
    client.cancel_admin_rotation(&company_id, &admin);

    // Admin should remain unchanged
    let info = client.get_company(&company_id);
    assert_eq!(info.admin, admin);
}

#[test]
fn test_treasury_rotation_full_flow() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let new_treasury = Address::generate(&env);

    client.propose_treasury_rotation(&company_id, &admin, &new_treasury);
    client.accept_treasury_rotation(&company_id, &new_treasury);

    let info = client.get_company(&company_id);
    assert_eq!(info.treasury, new_treasury);
}

#[test]
#[should_panic(expected = "A pending admin rotation already exists for this company")]
fn test_duplicate_admin_rotation_proposal_rejected() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let new_admin = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &admin, &new_admin);
    client.propose_admin_rotation(&company_id, &admin, &new_admin);
}

// ?? Issue #152: Duplicate company registration rejection tests ???????????????

#[test]
fn test_register_company_duplicate_registration_fails() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    // First registration should succeed
    let id0 = client.register_company(&admin, &treasury);
    assert_eq!(id0, 0u64);

    // Second registration with the same admin must be rejected
    let result = client.try_register_company(&admin, &treasury);
    assert!(result.is_err());
}

#[test]
#[should_panic(expected = "Company already registered")]
fn test_register_company_duplicate_registration_panics() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    client.register_company(&admin, &treasury);
    // Duplicate registration panics
    client.register_company(&admin, &treasury);
}

// ?? Issue #171: admin / treasury role-separation tests ???????????????????????

/// Role separation: holding the company's *treasury* address does not
/// confer HR-admin privileges. `add_employee` must still require the
/// registered company admin's signature ? the treasury holder signing for
/// themselves is not enough.
#[test]
#[should_panic(expected = "authorized")]
fn test_treasury_cannot_add_employee() {
    let env = Env::default();

    let contract_id = env.register_contract(None, PayrollRegistry);
    let registry = PayrollRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "register_company",
            args: (admin.clone(), treasury.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let company_id = registry.register_company(&admin, &treasury);

    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[3u8; 32]);

    // The company's own treasury address signs the call ? a legitimate
    // role in this system, just not the HR-admin role.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &treasury,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "add_employee",
            args: (company_id, employee.clone(), commitment.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    registry.add_employee(&company_id, &employee, &commitment);
}

/// Role separation: the treasury address is not the admin, so it must be
/// rejected when passed as `current_admin` to an admin-rotation call ?
/// business-logic identity checks, not just signatures, must hold.
#[test]
#[should_panic(expected = "Unauthorized: caller is not the company admin")]
fn test_treasury_cannot_propose_admin_rotation() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let new_admin = Address::generate(&env);

    client.propose_admin_rotation(&company_id, &treasury, &new_admin);
}

/// Mirrors `test_propose_admin_rotation_rejects_non_admin` for the
/// treasury-rotation path, which previously had no equivalent coverage.
#[test]
#[should_panic(expected = "Unauthorized: caller is not the company admin")]
fn test_propose_treasury_rotation_rejects_non_admin() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let attacker = Address::generate(&env);
    let new_treasury = Address::generate(&env);

    client.propose_treasury_rotation(&company_id, &attacker, &new_treasury);
}

/// Role separation: the admin who proposed a treasury rotation cannot
/// short-circuit the two-step handoff by accepting it themselves in place
/// of the actual proposed treasury holder.
#[test]
#[should_panic(expected = "Unauthorized: caller is not the proposed treasury")]
fn test_admin_cannot_accept_treasury_rotation_in_place_of_proposed_treasury() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let company_id = client.register_company(&admin, &treasury);
    let new_treasury = Address::generate(&env);

    client.propose_treasury_rotation(&company_id, &admin, &new_treasury);
    client.accept_treasury_rotation(&company_id, &admin);
}

#[test]
fn test_propose_treasury_rotation_emits_event() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let company_id = client.register_company(&admin, &treasury);
    let new_treasury = Address::generate(&env);

    client.propose_treasury_rotation(&company_id, &admin, &new_treasury);
    let after = event_count(&env);
    assert_eq!(after, 1, "propose must emit exactly one event");

    let topics = event_topics(&env, after - 1);
    let sym0: Symbol = topics.get(0).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(sym0, Symbol::new(&env, "TreasuryRotationProposed"));
    let comp_id: u64 = topics.get(1).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(comp_id, company_id);
}

#[test]
fn test_accept_treasury_rotation_emits_event() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let company_id = client.register_company(&admin, &treasury);
    let new_treasury = Address::generate(&env);

    client.propose_treasury_rotation(&company_id, &admin, &new_treasury);

    client.accept_treasury_rotation(&company_id, &new_treasury);
    let after = event_count(&env);
    // `TreasuryRotated` first, then the admin config version bump.
    assert_eq!(
        after, 2,
        "accept must emit TreasuryRotated and a config version update"
    );

    let topics = event_topics(&env, 0);
    let sym0: Symbol = topics.get(0).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(sym0, Symbol::new(&env, "TreasuryRotated"));
    let comp_id: u64 = topics.get(1).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(comp_id, company_id);

    let company = client.get_company(&company_id);
    assert_eq!(company.treasury, new_treasury);
}

#[test]
fn test_cancel_treasury_rotation_emits_event() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let company_id = client.register_company(&admin, &treasury);
    let new_treasury = Address::generate(&env);

    client.propose_treasury_rotation(&company_id, &admin, &new_treasury);

    client.cancel_treasury_rotation(&company_id, &admin);
    let after = event_count(&env);
    assert_eq!(after, 1, "cancel must emit exactly one event");

    let topics = event_topics(&env, after - 1);
    let sym0: Symbol = topics.get(0).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(sym0, Symbol::new(&env, "TreasuryRotationCancelled"));
    let comp_id: u64 = topics.get(1).unwrap().try_into_val(&env.clone()).unwrap();
    assert_eq!(comp_id, company_id);
}

// -- Issue #422: employee active status query helper --------------------------

#[test]
fn test_is_employee_active_helper_tracks_status_without_exposing_commitment() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[7u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    assert!(!client.is_employee_active(&company_id, &employee));

    client.add_employee(&company_id, &employee, &commitment);
    assert!(client.is_employee_active(&company_id, &employee));

    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Suspended);
    assert!(!client.is_employee_active(&company_id, &employee));

    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Active);
    assert!(client.is_employee_active(&company_id, &employee));
}
// ── Issue #486: Employee Payout Destination Update Flow Tests ───────────────

#[test]
fn test_successful_payout_destination_update_by_employee() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let new_destination = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);

    // Initial payout destination defaults to employee address
    assert_eq!(
        client.get_payout_destination(&company_id, &employee),
        employee
    );

    // Employee updates their payout destination
    client.update_payout_destination(&company_id, &employee, &new_destination);
    assert_eq!(
        client.get_payout_destination(&company_id, &employee),
        new_destination
    );
}

#[test]
#[should_panic(expected = "authorized")]
fn test_update_payout_destination_rejects_non_owner() {
    let env = Env::default();
    let contract_id = env.register_contract(None, PayrollRegistry);
    let registry = PayrollRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "register_company",
            args: (admin.clone(), treasury.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let company_id = registry.register_company(&admin, &treasury);

    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "add_employee",
            args: (company_id, employee.clone(), commitment.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    registry.add_employee(&company_id, &employee, &commitment);

    // Attacker (not employee) attempts to update employee's payout destination
    let attacker = Address::generate(&env);
    let new_destination = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "update_payout_destination",
            args: (company_id, employee.clone(), new_destination.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    registry.update_payout_destination(&company_id, &employee, &new_destination);
}

#[test]
#[should_panic(expected = "Cannot set zero address as payout destination")]
fn test_update_payout_destination_rejects_zero_address() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);

    let zero_addr = Address::from_string(&String::from_str(&env, VALID_EMPLOYEE_WALLET));
    client.update_payout_destination(&company_id, &employee, &zero_addr);
}

#[test]
#[should_panic(expected = "Destination address is already on file")]
fn test_update_payout_destination_rejects_duplicate_address() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);

    // Attempting to update to the same address already on file (the employee address itself)
    client.update_payout_destination(&company_id, &employee, &employee);
}

#[test]
#[should_panic(expected = "Invalid employee wallet address format")]
fn test_update_payout_destination_by_wallet_rejects_invalid_wallet() {
    let (env, contract_id) = setup();
    let client = PayrollRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[1u8; 32]);

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);

    let bad_wallet = String::from_str(&env, BAD_CHECKSUM_EMPLOYEE_WALLET);
    client.update_payout_destination_wallet(&company_id, &employee, &bad_wallet);
}

#[test]
#[should_panic(expected = "Offboarded employee status cannot be changed")]
fn test_offboarded_employee_cannot_be_changed() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let employee = Address::generate(&env);
    let commitment = BytesN::from_array(&env, &[0; 32]);
    let client = PayrollRegistryClient::new(&env, &env.register_contract(None, PayrollRegistry {}));

    let company_id = client.register_company(&admin, &treasury);
    client.add_employee(&company_id, &employee, &commitment);

    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Offboarded);

    // Attempting to change status should panic
    client.set_employee_status(&company_id, &employee, &EmployeeStatus::Active);
}
