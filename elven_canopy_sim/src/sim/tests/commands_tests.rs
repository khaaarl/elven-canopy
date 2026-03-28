//! Tests for SimCommand/SimAction processing — notifications, player
//! registration, selection groups, and tree ownership. Narrow scope:
//! only tests that exercise command dispatch for these specific subsystems
//! live here. Domain-specific tests (combat, movement, needs, etc.) are
//! in their respective test modules.

use super::*;

// -----------------------------------------------------------------------
// Notification tests
// -----------------------------------------------------------------------

#[test]
fn debug_notification_command_creates_notification() {
    let mut sim = test_sim(42);
    assert_eq!(sim.db.notifications.iter_all().count(), 0);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DebugNotification {
            message: "hello world".to_string(),
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(sim.db.notifications.iter_all().count(), 1);
    let notif = sim.db.notifications.iter_all().next().unwrap();
    assert_eq!(notif.message, "hello world");
    assert_eq!(notif.tick, 1);
}

/// The first notification gets ID 0. The bridge's `get_max_notification_id`
/// must return -1 (not 0) when no notifications exist, otherwise the polling
/// cursor `_last_notification_id` (initialized to `get_max_notification_id()`)
/// will equal the first notification's ID and `get_notifications_after(0)`
/// will skip it (the `<=` filter excludes ID 0).
#[test]
fn first_notification_gets_id_zero() {
    let mut sim = test_sim(42);
    sim.add_notification("first".to_string());

    let notif = sim.db.notifications.iter_all().next().unwrap();
    assert_eq!(
        notif.id,
        NotificationId(0),
        "first auto-increment notification should have ID 0"
    );
}

/// Verify the polling-cursor logic used by the bridge and GDScript:
/// notifications with `id <= after_id` are excluded. With `after_id = -1`
/// (the sentinel for "no notifications seen"), notification ID 0 must be
/// included. With `after_id = 0`, it must be excluded.
#[test]
fn notification_polling_cursor_filters_correctly() {
    let mut sim = test_sim(42);
    sim.add_notification("first".to_string());
    sim.add_notification("second".to_string());

    // Simulate the bridge's get_notifications_after filter:
    //   `if (notif.id.0 as i64) <= after_id { continue; }`
    let filter = |after_id: i64| -> Vec<String> {
        sim.db
            .notifications
            .iter_all()
            .filter(|n| (n.id.0 as i64) > after_id)
            .map(|n| n.message.clone())
            .collect()
    };

    // Sentinel -1: both notifications (IDs 0 and 1) are included.
    let all = filter(-1);
    assert_eq!(all.len(), 2, "after_id=-1 should include all notifications");
    assert_eq!(all[0], "first");
    assert_eq!(all[1], "second");

    // After seeing ID 0: only notification with ID 1 is included.
    let after_zero = filter(0);
    assert_eq!(
        after_zero.len(),
        1,
        "after_id=0 should exclude the first notification"
    );
    assert_eq!(after_zero[0], "second");

    // After seeing ID 1: no notifications remain.
    let after_one = filter(1);
    assert!(
        after_one.is_empty(),
        "after_id=1 should exclude all notifications"
    );
}

#[test]
fn notifications_persist_across_serde_roundtrip() {
    let mut sim = test_sim(42);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DebugNotification {
            message: "save me".to_string(),
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.notifications.iter_all().count(), 1);

    // Serialize and deserialize.
    let json = serde_json::to_string(&sim).unwrap();
    let mut restored: SimState = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.db.notifications.iter_all().count(), 1);
    let notif = restored.db.notifications.iter_all().next().unwrap();
    assert_eq!(notif.message, "save me");

    // Verify that auto-increment IDs don't collide after deserialization.
    restored.add_notification("post-load".to_string());
    let ids: Vec<_> = restored.db.notifications.iter_all().map(|n| n.id).collect();
    assert_eq!(ids.len(), 2);
    assert!(
        ids[1] > ids[0],
        "Post-load notification ID ({:?}) should be greater than pre-existing ({:?})",
        ids[1],
        ids[0]
    );
}

// ---------------------------------------------------------------------------
// Player identity tests
// ---------------------------------------------------------------------------

#[test]
fn register_player_creates_entry() {
    let mut sim = test_sim(42);
    assert_eq!(sim.db.players.iter_all().count(), 0);

    sim.register_player("alice");
    assert_eq!(sim.db.players.iter_all().count(), 1);

    let player = sim.db.players.get(&"alice".to_string()).unwrap();
    assert_eq!(player.name, "alice");
    assert_eq!(player.civ_id, sim.player_civ_id);
}

#[test]
fn register_player_is_idempotent() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    sim.register_player("alice");
    assert_eq!(sim.db.players.iter_all().count(), 1);
}

#[test]
fn register_player_multiple_players() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    sim.register_player("bob");
    assert_eq!(sim.db.players.iter_all().count(), 2);
    assert!(sim.db.players.get(&"alice".to_string()).is_some());
    assert!(sim.db.players.get(&"bob".to_string()).is_some());
}

#[test]
fn player_persists_across_serde_roundtrip() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    sim.register_player("bob");

    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    assert_eq!(restored.db.players.iter_all().count(), 2);
    let alice = restored.db.players.get(&"alice".to_string()).unwrap();
    assert_eq!(alice.civ_id, sim.player_civ_id);
    let bob = restored.db.players.get(&"bob".to_string()).unwrap();
    assert_eq!(bob.civ_id, sim.player_civ_id);
}

#[test]
fn tree_owner_is_civ_id() {
    let sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    assert_eq!(tree.owner, sim.player_civ_id);
}

// ---------------------------------------------------------------------------
// Selection groups (F-selection-groups)
// ---------------------------------------------------------------------------

#[test]
fn set_selection_group_creates_new_group() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    let elf_id = spawn_elf(&mut sim);

    let tick = sim.tick + 1;
    let cmd = SimCommand {
        player_name: "alice".to_string(),
        tick,
        action: SimAction::SetSelectionGroup {
            group_number: 1,
            creature_ids: vec![elf_id],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd], tick + 1);

    let groups = sim.get_selection_groups("alice");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, 1);
    assert_eq!(groups[0].1, vec![elf_id]);
    assert!(groups[0].2.is_empty());
}

#[test]
fn set_selection_group_overwrites_existing() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    let elf1 = spawn_elf(&mut sim);
    let elf2 = spawn_elf(&mut sim);

    let t = sim.tick + 1;
    let cmd1 = SimCommand {
        player_name: "alice".to_string(),
        tick: t,
        action: SimAction::SetSelectionGroup {
            group_number: 3,
            creature_ids: vec![elf1],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd1], t + 1);

    // Overwrite group 3 with elf2.
    let t2 = sim.tick + 1;
    let cmd2 = SimCommand {
        player_name: "alice".to_string(),
        tick: t2,
        action: SimAction::SetSelectionGroup {
            group_number: 3,
            creature_ids: vec![elf2],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd2], t2 + 1);

    let groups = sim.get_selection_groups("alice");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].1, vec![elf2]);
}

#[test]
fn add_to_selection_group_merges_without_duplicates() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    let elf1 = spawn_elf(&mut sim);
    let elf2 = spawn_elf(&mut sim);

    // Create group 2 with elf1.
    let t = sim.tick + 1;
    let cmd1 = SimCommand {
        player_name: "alice".to_string(),
        tick: t,
        action: SimAction::SetSelectionGroup {
            group_number: 2,
            creature_ids: vec![elf1],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd1], t + 1);

    // Add elf1 (duplicate) and elf2 (new).
    let t2 = sim.tick + 1;
    let cmd2 = SimCommand {
        player_name: "alice".to_string(),
        tick: t2,
        action: SimAction::AddToSelectionGroup {
            group_number: 2,
            creature_ids: vec![elf1, elf2],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd2], t2 + 1);

    let groups = sim.get_selection_groups("alice");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].1.len(), 2);
    assert!(groups[0].1.contains(&elf1));
    assert!(groups[0].1.contains(&elf2));
}

#[test]
fn add_to_nonexistent_group_creates_it() {
    let mut sim = test_sim(42);
    sim.register_player("bob");
    let elf = spawn_elf(&mut sim);

    let t = sim.tick + 1;
    let cmd = SimCommand {
        player_name: "bob".to_string(),
        tick: t,
        action: SimAction::AddToSelectionGroup {
            group_number: 5,
            creature_ids: vec![elf],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd], t + 1);

    let groups = sim.get_selection_groups("bob");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, 5);
    assert_eq!(groups[0].1, vec![elf]);
}

#[test]
fn selection_groups_are_per_player() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    sim.register_player("bob");
    let elf1 = spawn_elf(&mut sim);
    let elf2 = spawn_elf(&mut sim);

    let t = sim.tick + 1;
    let cmd1 = SimCommand {
        player_name: "alice".to_string(),
        tick: t,
        action: SimAction::SetSelectionGroup {
            group_number: 1,
            creature_ids: vec![elf1],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd1], t + 1);

    let t2 = sim.tick + 1;
    let cmd2 = SimCommand {
        player_name: "bob".to_string(),
        tick: t2,
        action: SimAction::SetSelectionGroup {
            group_number: 1,
            creature_ids: vec![elf2],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd2], t2 + 1);

    let alice_groups = sim.get_selection_groups("alice");
    assert_eq!(alice_groups.len(), 1);
    assert_eq!(alice_groups[0].1, vec![elf1]);

    let bob_groups = sim.get_selection_groups("bob");
    assert_eq!(bob_groups.len(), 1);
    assert_eq!(bob_groups[0].1, vec![elf2]);
}

#[test]
fn selection_groups_survive_serde_roundtrip() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    let elf = spawn_elf(&mut sim);

    let t = sim.tick + 1;
    let cmd = SimCommand {
        player_name: "alice".to_string(),
        tick: t,
        action: SimAction::SetSelectionGroup {
            group_number: 7,
            creature_ids: vec![elf],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd], t + 1);

    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    let groups = restored.get_selection_groups("alice");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, 7);
    assert_eq!(groups[0].1, vec![elf]);
}

#[test]
fn set_selection_group_with_structures() {
    let mut sim = test_sim(42);
    sim.register_player("alice");

    let t = sim.tick + 1;
    let cmd = SimCommand {
        player_name: "alice".to_string(),
        tick: t,
        action: SimAction::SetSelectionGroup {
            group_number: 4,
            creature_ids: vec![],
            structure_ids: vec![StructureId(42), StructureId(99)],
        },
    };
    sim.step(&[cmd], t + 1);

    let groups = sim.get_selection_groups("alice");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].2, vec![StructureId(42), StructureId(99)]);
}

#[test]
fn selection_group_command_serde_roundtrip() {
    let cmd = SimCommand {
        player_name: "player1".to_string(),
        tick: 10,
        action: SimAction::SetSelectionGroup {
            group_number: 3,
            creature_ids: vec![],
            structure_ids: vec![StructureId(1)],
        },
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());

    let cmd2 = SimCommand {
        player_name: "player1".to_string(),
        tick: 10,
        action: SimAction::AddToSelectionGroup {
            group_number: 5,
            creature_ids: vec![],
            structure_ids: vec![],
        },
    };
    let json2 = serde_json::to_string(&cmd2).unwrap();
    let restored2: SimCommand = serde_json::from_str(&json2).unwrap();
    assert_eq!(json2, serde_json::to_string(&restored2).unwrap());
}
