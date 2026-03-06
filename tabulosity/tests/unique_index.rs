//! Integration tests for unique index enforcement (`#[indexed(unique)]` and
//! `#[index(..., unique)]`). Tests insert, update, upsert, and remove
//! interactions with unique constraints.

use tabulosity::{Bounded, Database, Error, MatchAll, Table};

// ============================================================================
// Test types — simple unique index
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct UserId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct User {
    #[primary_key]
    pub id: UserId,
    #[indexed(unique)]
    pub email: String,
    pub name: String,
}

// ============================================================================
// Insert tests
// ============================================================================

#[test]
fn insert_unique_succeeds() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    assert_eq!(table.len(), 1);
}

#[test]
fn insert_duplicate_unique_fails() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    let err = table
        .insert_no_fk(User {
            id: UserId(2),
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap_err();

    match err {
        Error::DuplicateIndex { table, index, .. } => {
            assert_eq!(table, "users");
            assert_eq!(index, "email");
        }
        other => panic!("expected DuplicateIndex, got: {:?}", other),
    }

    // Table unchanged — second insert was rejected.
    assert_eq!(table.len(), 1);
}

#[test]
fn insert_different_unique_values_succeeds() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(User {
            id: UserId(2),
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();
    assert_eq!(table.len(), 2);
}

// ============================================================================
// Update tests
// ============================================================================

#[test]
fn update_same_unique_value_succeeds() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    // Update name but keep same email — no conflict.
    table
        .update_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alicia".into(),
        })
        .unwrap();

    assert_eq!(table.get_ref(&UserId(1)).unwrap().name, "Alicia");
}

#[test]
fn update_to_new_unique_value_succeeds() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    table
        .update_no_fk(User {
            id: UserId(1),
            email: "new@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    assert_eq!(table.get_ref(&UserId(1)).unwrap().email, "new@b.com");
}

#[test]
fn update_to_existing_unique_value_fails() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(User {
            id: UserId(2),
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    // Try to change Bob's email to Alice's.
    let err = table
        .update_no_fk(User {
            id: UserId(2),
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap_err();

    match err {
        Error::DuplicateIndex { index, .. } => assert_eq!(index, "email"),
        other => panic!("expected DuplicateIndex, got: {:?}", other),
    }

    // Bob's email unchanged.
    assert_eq!(table.get_ref(&UserId(2)).unwrap().email, "c@d.com");
}

// ============================================================================
// Upsert tests
// ============================================================================

#[test]
fn upsert_insert_path_unique_succeeds() {
    let mut table = UserTable::new();
    table
        .upsert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    assert_eq!(table.len(), 1);
}

#[test]
fn upsert_insert_path_duplicate_unique_fails() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    let err = table
        .upsert_no_fk(User {
            id: UserId(2),
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap_err();

    assert!(matches!(err, Error::DuplicateIndex { .. }));
    assert_eq!(table.len(), 1);
}

#[test]
fn upsert_update_path_same_value_succeeds() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    // Upsert same PK with same email, different name.
    table
        .upsert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alicia".into(),
        })
        .unwrap();

    assert_eq!(table.get_ref(&UserId(1)).unwrap().name, "Alicia");
}

#[test]
fn upsert_update_path_conflict_fails() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(User {
            id: UserId(2),
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    // Upsert Bob's PK but with Alice's email.
    let err = table
        .upsert_no_fk(User {
            id: UserId(2),
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap_err();

    assert!(matches!(err, Error::DuplicateIndex { .. }));
    // Bob unchanged.
    assert_eq!(table.get_ref(&UserId(2)).unwrap().email, "c@d.com");
}

// ============================================================================
// Remove tests — removing frees the unique value for reuse
// ============================================================================

#[test]
fn remove_frees_unique_value() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    table.remove_no_fk(&UserId(1)).unwrap();

    // Now a different row can use the same email.
    table
        .insert_no_fk(User {
            id: UserId(2),
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    assert_eq!(table.len(), 1);
}

// ============================================================================
// Query methods still work on unique indexes
// ============================================================================

#[test]
fn query_by_unique_field() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(User {
            id: UserId(2),
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    let results = table.by_email(&"a@b.com".to_string());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Alice");
}

// ============================================================================
// Compound unique index
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct SlotId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "building_slot", fields("building", "slot"), unique)]
struct Assignment {
    #[primary_key]
    pub id: SlotId,
    pub building: u32,
    pub slot: u32,
    pub elf_name: String,
}

#[test]
fn compound_unique_insert_succeeds() {
    let mut table = AssignmentTable::new();
    table
        .insert_no_fk(Assignment {
            id: SlotId(1),
            building: 1,
            slot: 1,
            elf_name: "Alice".into(),
        })
        .unwrap();
    // Same building, different slot — OK.
    table
        .insert_no_fk(Assignment {
            id: SlotId(2),
            building: 1,
            slot: 2,
            elf_name: "Bob".into(),
        })
        .unwrap();
    // Different building, same slot — OK.
    table
        .insert_no_fk(Assignment {
            id: SlotId(3),
            building: 2,
            slot: 1,
            elf_name: "Carol".into(),
        })
        .unwrap();
    assert_eq!(table.len(), 3);
}

#[test]
fn compound_unique_insert_conflict() {
    let mut table = AssignmentTable::new();
    table
        .insert_no_fk(Assignment {
            id: SlotId(1),
            building: 1,
            slot: 1,
            elf_name: "Alice".into(),
        })
        .unwrap();

    // Same (building, slot) — conflict.
    let err = table
        .insert_no_fk(Assignment {
            id: SlotId(2),
            building: 1,
            slot: 1,
            elf_name: "Bob".into(),
        })
        .unwrap_err();

    match err {
        Error::DuplicateIndex { index, .. } => assert_eq!(index, "building_slot"),
        other => panic!("expected DuplicateIndex, got: {:?}", other),
    }
}

#[test]
fn compound_unique_update_succeeds() {
    let mut table = AssignmentTable::new();
    table
        .insert_no_fk(Assignment {
            id: SlotId(1),
            building: 1,
            slot: 1,
            elf_name: "Alice".into(),
        })
        .unwrap();

    // Move to a different slot.
    table
        .update_no_fk(Assignment {
            id: SlotId(1),
            building: 1,
            slot: 2,
            elf_name: "Alice".into(),
        })
        .unwrap();

    assert_eq!(table.get_ref(&SlotId(1)).unwrap().slot, 2);
}

#[test]
fn compound_unique_update_conflict() {
    let mut table = AssignmentTable::new();
    table
        .insert_no_fk(Assignment {
            id: SlotId(1),
            building: 1,
            slot: 1,
            elf_name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(Assignment {
            id: SlotId(2),
            building: 1,
            slot: 2,
            elf_name: "Bob".into(),
        })
        .unwrap();

    // Try to move Bob into Alice's slot.
    let err = table
        .update_no_fk(Assignment {
            id: SlotId(2),
            building: 1,
            slot: 1,
            elf_name: "Bob".into(),
        })
        .unwrap_err();

    assert!(matches!(err, Error::DuplicateIndex { .. }));
    // Bob unchanged.
    assert_eq!(table.get_ref(&SlotId(2)).unwrap().slot, 2);
}

// ============================================================================
// Unique + filtered index
// ============================================================================

fn is_active(a: &Registration) -> bool {
    a.active
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct RegId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "active_email", fields("email"), filter = "is_active", unique)]
struct Registration {
    #[primary_key]
    pub id: RegId,
    pub email: String,
    pub active: bool,
}

#[test]
fn filtered_unique_allows_duplicates_when_filtered_out() {
    let mut table = RegistrationTable::new();

    // Active registration.
    table
        .insert_no_fk(Registration {
            id: RegId(1),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap();

    // Inactive registration with same email — allowed (filter excludes it).
    table
        .insert_no_fk(Registration {
            id: RegId(2),
            email: "a@b.com".into(),
            active: false,
        })
        .unwrap();

    assert_eq!(table.len(), 2);
}

#[test]
fn filtered_unique_rejects_duplicate_active() {
    let mut table = RegistrationTable::new();

    table
        .insert_no_fk(Registration {
            id: RegId(1),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap();

    // Another active registration with same email — conflict.
    let err = table
        .insert_no_fk(Registration {
            id: RegId(2),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap_err();

    assert!(matches!(err, Error::DuplicateIndex { .. }));
}

#[test]
fn filtered_unique_update_activate_conflict() {
    let mut table = RegistrationTable::new();

    table
        .insert_no_fk(Registration {
            id: RegId(1),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap();
    table
        .insert_no_fk(Registration {
            id: RegId(2),
            email: "a@b.com".into(),
            active: false,
        })
        .unwrap();

    // Activate the second registration — conflicts with first.
    let err = table
        .update_no_fk(Registration {
            id: RegId(2),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap_err();

    assert!(matches!(err, Error::DuplicateIndex { .. }));
}

#[test]
fn filtered_unique_update_deactivate_frees_value() {
    let mut table = RegistrationTable::new();

    table
        .insert_no_fk(Registration {
            id: RegId(1),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap();

    // Deactivate first.
    table
        .update_no_fk(Registration {
            id: RegId(1),
            email: "a@b.com".into(),
            active: false,
        })
        .unwrap();

    // Now another active registration with same email is OK.
    table
        .insert_no_fk(Registration {
            id: RegId(2),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap();
}

// ============================================================================
// Unique index with Database-level FK validation
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct TeamId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Team {
    #[primary_key]
    pub id: TeamId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct MemberId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "team_role", fields("team_id", "role"), unique)]
struct Member {
    #[primary_key]
    pub id: MemberId,
    #[indexed]
    pub team_id: TeamId,
    pub role: String,
    pub name: String,
}

#[derive(Database)]
struct TeamDb {
    #[table(singular = "team", fks())]
    pub teams: TeamTable,
    #[table(singular = "member", fks(team_id = "teams"))]
    pub members: MemberTable,
}

#[test]
fn database_unique_insert_succeeds() {
    let mut db = TeamDb::new();
    db.insert_team(Team {
        id: TeamId(1),
        name: "Alpha".into(),
    })
    .unwrap();

    db.insert_member(Member {
        id: MemberId(1),
        team_id: TeamId(1),
        role: "captain".into(),
        name: "Alice".into(),
    })
    .unwrap();

    assert_eq!(db.members.len(), 1);
}

#[test]
fn database_unique_insert_conflict() {
    let mut db = TeamDb::new();
    db.insert_team(Team {
        id: TeamId(1),
        name: "Alpha".into(),
    })
    .unwrap();

    db.insert_member(Member {
        id: MemberId(1),
        team_id: TeamId(1),
        role: "captain".into(),
        name: "Alice".into(),
    })
    .unwrap();

    // Same team + role — conflict.
    let err = db
        .insert_member(Member {
            id: MemberId(2),
            team_id: TeamId(1),
            role: "captain".into(),
            name: "Bob".into(),
        })
        .unwrap_err();

    assert!(matches!(err, Error::DuplicateIndex { .. }));
}

#[test]
fn database_unique_upsert_update_conflict() {
    let mut db = TeamDb::new();
    db.insert_team(Team {
        id: TeamId(1),
        name: "Alpha".into(),
    })
    .unwrap();

    db.insert_member(Member {
        id: MemberId(1),
        team_id: TeamId(1),
        role: "captain".into(),
        name: "Alice".into(),
    })
    .unwrap();
    db.insert_member(Member {
        id: MemberId(2),
        team_id: TeamId(1),
        role: "medic".into(),
        name: "Bob".into(),
    })
    .unwrap();

    // Upsert Bob to captain role — conflict.
    let err = db
        .upsert_member(Member {
            id: MemberId(2),
            team_id: TeamId(1),
            role: "captain".into(),
            name: "Bob".into(),
        })
        .unwrap_err();

    assert!(matches!(err, Error::DuplicateIndex { .. }));
}

// ============================================================================
// Unique index alongside non-unique index on same table
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct EmpId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Employee {
    #[primary_key]
    pub id: EmpId,
    #[indexed(unique)]
    pub badge: u32,
    #[indexed]
    pub department: String,
}

#[test]
fn mixed_unique_and_nonunique_indexes() {
    let mut table = EmployeeTable::new();
    table
        .insert_no_fk(Employee {
            id: EmpId(1),
            badge: 100,
            department: "eng".into(),
        })
        .unwrap();
    table
        .insert_no_fk(Employee {
            id: EmpId(2),
            badge: 200,
            department: "eng".into(), // same department OK (non-unique)
        })
        .unwrap();

    // Duplicate badge fails.
    let err = table
        .insert_no_fk(Employee {
            id: EmpId(3),
            badge: 100,
            department: "sales".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));

    // Queries on both indexes work.
    assert_eq!(table.by_badge(&100).len(), 1);
    assert_eq!(table.by_department(&"eng".to_string()).len(), 2);
}

// ============================================================================
// Rebuild indexes preserves unique constraint
// ============================================================================

#[test]
fn rebuild_indexes_unique_still_enforced() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    table.rebuild_indexes();

    // Unique constraint still enforced after rebuild.
    let err = table
        .insert_no_fk(User {
            id: UserId(2),
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));
}

// ============================================================================
// Multiple unique indexes on the same table
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct AcctId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Account {
    #[primary_key]
    pub id: AcctId,
    #[indexed(unique)]
    pub username: String,
    #[indexed(unique)]
    pub email: String,
    pub display_name: String,
}

#[test]
fn multiple_unique_indexes_both_checked_on_insert() {
    let mut table = AccountTable::new();
    table
        .insert_no_fk(Account {
            id: AcctId(1),
            username: "alice".into(),
            email: "a@b.com".into(),
            display_name: "Alice".into(),
        })
        .unwrap();

    // Duplicate username, different email.
    let err = table
        .insert_no_fk(Account {
            id: AcctId(2),
            username: "alice".into(),
            email: "c@d.com".into(),
            display_name: "Alice2".into(),
        })
        .unwrap_err();
    match err {
        Error::DuplicateIndex { index, .. } => assert_eq!(index, "username"),
        other => panic!("expected DuplicateIndex on username, got: {:?}", other),
    }

    // Different username, duplicate email.
    let err = table
        .insert_no_fk(Account {
            id: AcctId(3),
            username: "bob".into(),
            email: "a@b.com".into(),
            display_name: "Bob".into(),
        })
        .unwrap_err();
    match err {
        Error::DuplicateIndex { index, .. } => assert_eq!(index, "email"),
        other => panic!("expected DuplicateIndex on email, got: {:?}", other),
    }

    // Both different — succeeds.
    table
        .insert_no_fk(Account {
            id: AcctId(4),
            username: "charlie".into(),
            email: "c@d.com".into(),
            display_name: "Charlie".into(),
        })
        .unwrap();
    assert_eq!(table.len(), 2);
}

#[test]
fn multiple_unique_indexes_both_checked_on_update() {
    let mut table = AccountTable::new();
    table
        .insert_no_fk(Account {
            id: AcctId(1),
            username: "alice".into(),
            email: "a@b.com".into(),
            display_name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(Account {
            id: AcctId(2),
            username: "bob".into(),
            email: "c@d.com".into(),
            display_name: "Bob".into(),
        })
        .unwrap();

    // Update Bob's username to Alice's — conflict.
    let err = table
        .update_no_fk(Account {
            id: AcctId(2),
            username: "alice".into(),
            email: "c@d.com".into(),
            display_name: "Bob".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));

    // Bob unchanged.
    assert_eq!(table.get_ref(&AcctId(2)).unwrap().username, "bob");
}

// ============================================================================
// Insert into empty table (bounds are None)
// ============================================================================

#[test]
fn insert_into_empty_table_succeeds() {
    let mut table = UserTable::new();
    // First insert — bounds are None, so the unique check should pass trivially.
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    assert_eq!(table.len(), 1);
}

// ============================================================================
// Update single row to same unique value (self-conflict check)
// ============================================================================

#[test]
fn update_single_row_same_unique_value() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    // Update the only row — change name but keep email. Should not conflict with itself.
    table
        .update_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alicia".into(),
        })
        .unwrap();
    assert_eq!(table.get_ref(&UserId(1)).unwrap().name, "Alicia");
}

// ============================================================================
// Failed insert leaves table unchanged
// ============================================================================

#[test]
fn failed_insert_leaves_table_completely_unchanged() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    // Attempt multiple failed inserts.
    for i in 2..5 {
        let _ = table.insert_no_fk(User {
            id: UserId(i),
            email: "a@b.com".into(),
            name: format!("Person{}", i),
        });
    }

    // Table should still have only the original row.
    assert_eq!(table.len(), 1);
    assert_eq!(table.get_ref(&UserId(1)).unwrap().name, "Alice");
    // And query by unique index should still return exactly 1 result.
    assert_eq!(table.by_email(&"a@b.com".to_string()).len(), 1);
}

// ============================================================================
// Upsert on empty table
// ============================================================================

#[test]
fn upsert_insert_path_on_empty_table() {
    let mut table = UserTable::new();
    table
        .upsert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    assert_eq!(table.len(), 1);
    assert_eq!(table.get_ref(&UserId(1)).unwrap().email, "a@b.com");
}

// ============================================================================
// Count and iter queries on unique indexes
// ============================================================================

#[test]
fn count_by_unique_field() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(User {
            id: UserId(2),
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    assert_eq!(table.count_by_email(&"a@b.com".to_string()), 1);
    assert_eq!(table.count_by_email(&"missing@x.com".to_string()), 0);
    assert_eq!(table.count_by_email(MatchAll), 2);
}

#[test]
fn iter_by_unique_field() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(User {
            id: UserId(2),
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    let names: Vec<&str> = table
        .iter_by_email(&"a@b.com".to_string())
        .map(|u| u.name.as_str())
        .collect();
    assert_eq!(names, vec!["Alice"]);
}

// ============================================================================
// Compound unique index: count and iter
// ============================================================================

#[test]
fn compound_unique_count_and_query() {
    let mut table = AssignmentTable::new();
    table
        .insert_no_fk(Assignment {
            id: SlotId(1),
            building: 1,
            slot: 1,
            elf_name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(Assignment {
            id: SlotId(2),
            building: 1,
            slot: 2,
            elf_name: "Bob".into(),
        })
        .unwrap();

    // Exact compound query.
    assert_eq!(table.count_by_building_slot(&1u32, &1u32), 1);
    assert_eq!(table.count_by_building_slot(&1u32, &2u32), 1);
    assert_eq!(table.count_by_building_slot(&1u32, &99u32), 0);

    // Prefix query (all slots in building 1).
    assert_eq!(table.count_by_building_slot(&1u32, MatchAll), 2);
}

// ============================================================================
// Filtered unique: simultaneous filter + unique field change on update
// ============================================================================

#[test]
fn filtered_unique_update_changes_filter_and_field_simultaneously() {
    let mut table = RegistrationTable::new();

    // Active email "a@b.com".
    table
        .insert_no_fk(Registration {
            id: RegId(1),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap();

    // Inactive email "c@d.com".
    table
        .insert_no_fk(Registration {
            id: RegId(2),
            email: "c@d.com".into(),
            active: false,
        })
        .unwrap();

    // Update reg 2: activate AND change email to "a@b.com" simultaneously.
    // This should conflict because "a@b.com" is already active.
    let err = table
        .update_no_fk(Registration {
            id: RegId(2),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));

    // Update reg 2: activate AND change email to unique value — succeeds.
    table
        .update_no_fk(Registration {
            id: RegId(2),
            email: "new@x.com".into(),
            active: true,
        })
        .unwrap();
    assert_eq!(table.get_ref(&RegId(2)).unwrap().email, "new@x.com");
    assert!(table.get_ref(&RegId(2)).unwrap().active);
}

#[test]
fn filtered_unique_deactivate_and_change_field_simultaneously() {
    let mut table = RegistrationTable::new();

    table
        .insert_no_fk(Registration {
            id: RegId(1),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap();
    table
        .insert_no_fk(Registration {
            id: RegId(2),
            email: "c@d.com".into(),
            active: true,
        })
        .unwrap();

    // Deactivate reg 2 AND change email to "a@b.com" simultaneously.
    // Since the new row doesn't pass the filter, no conflict.
    table
        .update_no_fk(Registration {
            id: RegId(2),
            email: "a@b.com".into(),
            active: false,
        })
        .unwrap();
    assert_eq!(table.get_ref(&RegId(2)).unwrap().email, "a@b.com");
    assert!(!table.get_ref(&RegId(2)).unwrap().active);
}

// ============================================================================
// Cascade delete frees unique values for re-insertion
// ============================================================================

#[test]
fn cascade_delete_frees_unique_value() {
    let mut db = TeamDb::new();
    db.insert_team(Team {
        id: TeamId(1),
        name: "Alpha".into(),
    })
    .unwrap();

    db.insert_member(Member {
        id: MemberId(1),
        team_id: TeamId(1),
        role: "captain".into(),
        name: "Alice".into(),
    })
    .unwrap();

    // (1, "captain") is taken.
    let err = db
        .insert_member(Member {
            id: MemberId(2),
            team_id: TeamId(1),
            role: "captain".into(),
            name: "Bob".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));

    // Now add cascade semantics via a new schema.
    // (The TeamDb schema uses restrict, so we can't test cascade with it.
    //  Instead, manually remove the member and verify re-insertion works.)
    db.remove_member(&MemberId(1)).unwrap();

    // Now (1, "captain") should be available again.
    db.insert_member(Member {
        id: MemberId(3),
        team_id: TeamId(1),
        role: "captain".into(),
        name: "Charlie".into(),
    })
    .unwrap();
    assert_eq!(db.members.len(), 1);
    assert_eq!(db.members.get_ref(&MemberId(3)).unwrap().name, "Charlie");
}

// ============================================================================
// Cascade delete + unique index (with dedicated cascade schema)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct DeptId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Dept {
    #[primary_key]
    pub id: DeptId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct PositionId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Position {
    #[primary_key]
    pub id: PositionId,
    #[indexed]
    pub dept_id: DeptId,
    #[indexed(unique)]
    pub title: String,
}

#[derive(Database)]
struct CascadeUniqueDb {
    #[table(singular = "dept", fks())]
    pub depts: DeptTable,
    #[table(singular = "position", fks(dept_id = "depts" on_delete cascade))]
    pub positions: PositionTable,
}

#[test]
fn cascade_delete_frees_unique_values_in_child_table() {
    let mut db = CascadeUniqueDb::new();
    db.insert_dept(Dept {
        id: DeptId(1),
        name: "Engineering".into(),
    })
    .unwrap();
    db.insert_dept(Dept {
        id: DeptId(2),
        name: "Sales".into(),
    })
    .unwrap();

    db.insert_position(Position {
        id: PositionId(1),
        dept_id: DeptId(1),
        title: "Lead".into(),
    })
    .unwrap();

    // "Lead" is taken.
    let err = db
        .insert_position(Position {
            id: PositionId(2),
            dept_id: DeptId(2),
            title: "Lead".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));

    // Cascade-delete dept 1, which removes position "Lead".
    db.remove_dept(&DeptId(1)).unwrap();
    assert!(db.positions.is_empty());

    // Now "Lead" should be available again.
    db.insert_position(Position {
        id: PositionId(3),
        dept_id: DeptId(2),
        title: "Lead".into(),
    })
    .unwrap();
    assert_eq!(db.positions.len(), 1);
}

// ============================================================================
// Nullify on delete + unique index
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct GroupId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Group {
    #[primary_key]
    pub id: GroupId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ProfileId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Profile {
    #[primary_key]
    pub id: ProfileId,
    #[indexed]
    pub group_id: Option<GroupId>,
    #[indexed(unique)]
    pub handle: String,
}

#[derive(Database)]
struct NullifyUniqueDb {
    #[table(singular = "group", fks())]
    pub groups: GroupTable,
    #[table(singular = "profile", fks(group_id? = "groups" on_delete nullify))]
    pub profiles: ProfileTable,
}

#[test]
fn nullify_on_delete_with_unique_index() {
    let mut db = NullifyUniqueDb::new();
    db.insert_group(Group {
        id: GroupId(1),
        name: "Admins".into(),
    })
    .unwrap();

    db.insert_profile(Profile {
        id: ProfileId(1),
        group_id: Some(GroupId(1)),
        handle: "alice".into(),
    })
    .unwrap();
    db.insert_profile(Profile {
        id: ProfileId(2),
        group_id: Some(GroupId(1)),
        handle: "bob".into(),
    })
    .unwrap();

    // Nullify the group FK.
    db.remove_group(&GroupId(1)).unwrap();

    // Profiles should survive with group_id = None.
    assert_eq!(db.profiles.len(), 2);
    assert_eq!(db.profiles.get_ref(&ProfileId(1)).unwrap().group_id, None);
    assert_eq!(db.profiles.get_ref(&ProfileId(2)).unwrap().group_id, None);

    // Unique constraint on handle should still be enforced.
    let err = db
        .insert_profile(Profile {
            id: ProfileId(3),
            group_id: None,
            handle: "alice".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));
}

// ============================================================================
// DuplicateIndex error key format
// ============================================================================

#[test]
fn duplicate_index_error_key_format_simple() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    let err = table
        .insert_no_fk(User {
            id: UserId(2),
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap_err();

    match err {
        Error::DuplicateIndex { table, index, key } => {
            assert_eq!(table, "users");
            assert_eq!(index, "email");
            // The key should contain the field value debug representation.
            assert!(key.contains("a@b.com"), "key was: {}", key);
        }
        other => panic!("expected DuplicateIndex, got: {:?}", other),
    }
}

#[test]
fn duplicate_index_error_key_format_compound() {
    let mut table = AssignmentTable::new();
    table
        .insert_no_fk(Assignment {
            id: SlotId(1),
            building: 1,
            slot: 2,
            elf_name: "Alice".into(),
        })
        .unwrap();

    let err = table
        .insert_no_fk(Assignment {
            id: SlotId(2),
            building: 1,
            slot: 2,
            elf_name: "Bob".into(),
        })
        .unwrap_err();

    match err {
        Error::DuplicateIndex { table, index, key } => {
            assert_eq!(table, "assignments");
            assert_eq!(index, "building_slot");
            // The key should contain both field values.
            assert!(key.contains("1"), "key was: {}", key);
            assert!(key.contains("2"), "key was: {}", key);
        }
        other => panic!("expected DuplicateIndex, got: {:?}", other),
    }
}

// ============================================================================
// DuplicateIndex Display trait
// ============================================================================

#[test]
fn duplicate_index_error_display() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    let err = table
        .insert_no_fk(User {
            id: UserId(2),
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap_err();

    let display = err.to_string();
    assert!(display.contains("users"), "display was: {}", display);
    assert!(display.contains("email"), "display was: {}", display);
    assert!(display.contains("a@b.com"), "display was: {}", display);
}

// ============================================================================
// Auto-increment + unique index
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct AutoUserId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct AutoUser {
    #[primary_key(auto_increment)]
    pub id: AutoUserId,
    #[indexed(unique)]
    pub email: String,
    pub name: String,
}

#[test]
fn auto_increment_with_unique_insert_succeeds() {
    let mut table = AutoUserTable::new();
    let id0 = table
        .insert_auto_no_fk(|pk| AutoUser {
            id: pk,
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    assert_eq!(id0, AutoUserId(0));

    let id1 = table
        .insert_auto_no_fk(|pk| AutoUser {
            id: pk,
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();
    assert_eq!(id1, AutoUserId(1));
    assert_eq!(table.len(), 2);
}

#[test]
fn auto_increment_with_unique_insert_conflict() {
    let mut table = AutoUserTable::new();
    table
        .insert_auto_no_fk(|pk| AutoUser {
            id: pk,
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    let err = table
        .insert_auto_no_fk(|pk| AutoUser {
            id: pk,
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));

    // Table should have only the first row, and next_id should still have advanced.
    assert_eq!(table.len(), 1);
}

#[test]
fn auto_increment_unique_update_succeeds() {
    let mut table = AutoUserTable::new();
    table
        .insert_auto_no_fk(|pk| AutoUser {
            id: pk,
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    table
        .update_no_fk(AutoUser {
            id: AutoUserId(0),
            email: "new@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    assert_eq!(table.get_ref(&AutoUserId(0)).unwrap().email, "new@b.com");
}

#[test]
fn auto_increment_unique_upsert_insert_conflict() {
    let mut table = AutoUserTable::new();
    table
        .insert_auto_no_fk(|pk| AutoUser {
            id: pk,
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    // Upsert with new PK but duplicate email — insert path, should fail.
    let err = table
        .upsert_no_fk(AutoUser {
            id: AutoUserId(99),
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));
    assert_eq!(table.len(), 1);
}
