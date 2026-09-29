# Payroll Enhancement Features

This document describes the lightweight enhancements added to improve payroll workflow validation and audit trail without exposing sensitive data.

## Per-Period Payout Count Guard (#545)

**Purpose**: Track and validate the number of payouts within a single payroll period to prevent excessive transactions and detect anomalies.

**Implementation**:
- Added `payout_count` field to `PeriodUsage` struct
- Guard enforced in `enforce_and_record_capacity()` function
- Max payout count configurable in `CapacityLimits`
- Privacy-safe: count increments without revealing individual salary amounts

**Error Handling**:
- Returns privacy-safe error code `CapacityLimitKind::PayoutCount` when exceeded
- Does not expose employee names or salary values in error messages

**Usage**:
```rust
// Configure limits including payout count
payroll.set_capacity_limits(admin, CapacityLimits {
    max_batches: 50,
    max_employees: 1000,
    max_total_value: 10_000_000,
    max_payouts: 5000,  // New field
});
```

## Payout Destination Blacklist Validation (#550)

**Purpose**: Block payments to flagged addresses (e.g., sanctioned wallets, compromised accounts).

**Implementation**:
- New `blacklisted_destinations` storage map
- Validation occurs before payment execution in `batch_process_payroll()`
- Admin-only management via `add_blacklisted_destination()` / `remove_blacklisted_destination()`
- Privacy-safe: rejection does not reveal which employee triggered the block

**Error Handling**:
- Returns generic `PayrollError::PayoutDestinationBlocked` error
- Event emits only boolean flag, not employee address

**Usage**:
```rust
// Add address to blacklist
payroll.add_blacklisted_destination(admin, flagged_address);

// Check if address is blacklisted
if payroll.is_destination_blacklisted(employee_address) {
    // Handle blocked payout
}
```

## Audit Event for Payroll Draft Expiration (#540)

**Purpose**: Provide audit trail when drafts expire to support compliance and operational monitoring.

**Implementation**:
- Enhanced `emit_draft_expired` event with additional context
- New `emit_draft_expiration_audit()` function in events module
- Includes: draft_id, expiration_reason_code, admin_actor, timestamp
- Privacy-safe: no salary amounts or employee data exposed

**Event Schema**:
```rust
emit_draft_expiration_audit(
    env,
    draft_id,          // u64
    reason_code,       // Symbol (e.g., "timeout", "admin_action")
    admin,             // Address
    created_timestamp, // u64
    expired_timestamp  // u64
);
```

**Reason Codes**:
- `timeout` - Draft exceeded configured expiration window
- `admin_action` - Manually expired by admin
- `replaced` - Superseded by new draft for same period

## Payroll Period Amendment Audit Trail (#505)

**Purpose**: Record all authorized amendments to payroll periods with actor, timestamp, and privacy-safe reason codes.

**Implementation**:
- New `PeriodAmendment` struct stored per amendment
- Tracks: amendment_id, period_label, admin_actor, reason_code, amended_at
- Emits `period_amended` event for off-chain indexing
- Cumulative counter `amendment_count` per period for quick audit checks

**Privacy Protection**:
- Uses coded reason symbols (e.g., "correction", "late_addition", "rate_change")
- Does not store or emit salary amounts, employee names, or commitment values
- Audit log queryable only by admin role

**Event Schema**:
```rust
emit_period_amended(
    env,
    period,            // Symbol
    amendment_id,      // u64
    admin,             // Address
    reason_code,       // Symbol
    amendment_count    // u32 - total amendments for this period
);
```

**Usage**:
```rust
// Amend a period (internally records audit trail)
payroll.amend_run_draft(admin, draft_id, new_total, new_count);

// Query amendment history (admin only)
let amendments = payroll.get_period_amendments(period);
for amendment in amendments {
    log!(
        "Period {} amended by {} at {} (reason: {})",
        amendment.period,
        amendment.admin,
        amendment.amended_at,
        amendment.reason_code
    );
}
```

## Single Active Payroll Period (#578)

**Purpose**: Guarantee that exactly one payroll period is active at a time, so
`get_current_period()` is never ambiguous and every run, draft and capacity
counter is attributed to one period.

**Implementation**:
- `open_capacity_period()` rejects a second period while one is already active
- New admin-only `close_capacity_period()` clears the active period and emits `capacity_period_closed`
- Only `DataKey::CurrentPeriod` is touched; per-period usage counters are keyed by period label and are untouched, so re-opening a period resumes its existing counters

**Error Handling**:
- Re-opening the period that is already active: `Payroll period is already the active period`
- Opening a different period while one is active: `An active payroll period already exists: close it before opening a new one`
- Closing with no active period: `No active payroll period to close`
- `close_capacity_period()` requires the company admin's authorisation

**Usage**:
```rust
// Open the first (or only) active period.
payroll.open_capacity_period(admin, symbol!("P2026_02"));

// Before moving to the next period, close the current one.
payroll.close_capacity_period(admin);
payroll.open_capacity_period(admin, symbol!("P2026_03"));
```

Period usage survives the close/re-open cycle, so closing a period does not
reset `get_period_usage()` for that label:

```rust
payroll.close_capacity_period(admin);
payroll.open_capacity_period(admin, symbol!("P2026_02"));
let usage = payroll.get_period_usage(&symbol!("P2026_02")); // counters resume, not reset
```

## Testing

All features include:
- Unit tests for happy path
- Edge case validation (e.g., boundary conditions, unauthorized access)
- Privacy verification (sensitive data not exposed in errors/events)

Test files:
- `tests/payout_count_guard_test.rs`
- `tests/blacklist_validation_test.rs`
- `tests/draft_expiration_audit_test.rs`
- `tests/period_amendment_audit_test.rs`
- `tests/period-capacity/period_capacity.rs` (active-period uniqueness)

## Compliance Notes

These enhancements support compliance and audit requirements while maintaining zero-knowledge privacy properties:
- No salary amounts exposed in events or errors
- No employee identities revealed in validation failures
- Audit trails use privacy-safe codes instead of descriptive text
- All changes follow repository security conventions
