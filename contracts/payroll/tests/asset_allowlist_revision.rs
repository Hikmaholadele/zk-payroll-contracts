mod common;

use soroban_sdk::testutils::Address as _;
use soroban_sdk::Address;

#[test]
fn asset_allowlist_revision_increments_only_when_status_changes() {
    let env = soroban_sdk::Env::default();
    let (payroll, token, _) = common::setup(&env);

    // Initialization establishes the initial allowlist state, not an admin
    // update, so the first observable revision is zero.
    assert_eq!(payroll.get_asset_allowlist_revision(&token), 0);

    payroll.set_asset_allowed(&token, &false);
    assert_eq!(payroll.get_asset_allowlist_revision(&token), 1);

    // Repeating the same value must not make clients refresh unnecessarily.
    payroll.set_asset_allowed(&token, &false);
    assert_eq!(payroll.get_asset_allowlist_revision(&token), 1);

    payroll.set_asset_allowed(&token, &true);
    assert_eq!(payroll.get_asset_allowlist_revision(&token), 2);
}

#[test]
fn unknown_asset_revision_is_zero() {
    let env = soroban_sdk::Env::default();
    let (payroll, _, _) = common::setup(&env);
    let unknown_asset = Address::generate(&env);

    assert_eq!(payroll.get_asset_allowlist_revision(&unknown_asset), 0);
}
