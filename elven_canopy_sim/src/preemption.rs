// Task preemption priority system.
//
// Assigns a numeric priority level to each (TaskKindTag, TaskOrigin) pair and
// determines whether a new task can preempt a creature's current task. This
// replaces the ad-hoc `mope_can_interrupt_task` config flag with a general,
// exhaustive priority scheme.
//
// The `PreemptionLevel` enum has 8 levels from Idle(0) to Flee(7). The mapping
// function `preemption_level()` uses exhaustive matches on both `TaskKindTag`
// and `TaskOrigin` — adding a new variant to either enum causes a compile error
// here, forcing the developer to assign the correct level.
//
// Preemption rules are encoded in `can_preempt()`:
// - Standard: new level > current level → preempt.
// - Hardcoded exception: Mood never preempts Survival (prevents starvation spiral).
// - PlayerDirected override: PlayerDirected commands can preempt AutonomousCombat.
// - Same-level: no preemption, except PlayerDirected replaces PlayerDirected and
//   PlayerCombat replaces PlayerCombat (player changed their mind).
//
// See docs/drafts/combat_military.md §8 for the full design.
//
// **Critical constraint: determinism.** This module is pure computation with no
// state, randomness, or I/O.

use crate::db::TaskKindTag;
use crate::task::TaskOrigin;
use serde::{Deserialize, Serialize};

/// Priority level for task preemption. Higher numeric value = higher priority.
///
/// Does NOT derive `Ord` — comparisons use the explicit `level()` method to
/// avoid subtle bugs from variant declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PreemptionLevel {
    /// 0 — no task, wandering.
    Idle,
    /// 1 — background work: haul, cook, craft, harvest.
    Autonomous,
    /// 2 — player-directed non-combat: GoTo, Build, Furnish.
    PlayerDirected,
    /// 3 — self-care: eat, sleep, acquire items.
    Survival,
    /// 4 — mood crisis: mope.
    Mood,
    /// 5 — autonomous combat engagement (group behavior = Fight).
    AutonomousCombat,
    /// 6 — player-directed attack/attack-move.
    PlayerCombat,
    /// 7 — emergency flee.
    Flee,
}

impl PreemptionLevel {
    /// Numeric priority. Higher = more important.
    pub fn level(&self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Autonomous => 1,
            Self::PlayerDirected => 2,
            Self::Survival => 3,
            Self::Mood => 4,
            Self::AutonomousCombat => 5,
            Self::PlayerCombat => 6,
            Self::Flee => 7,
        }
    }
}

/// Compute the preemption level for a task with the given kind and origin.
///
/// Uses exhaustive matches (no wildcards) so that adding a new `TaskKindTag`
/// or `TaskOrigin` variant causes a compile error here.
pub fn preemption_level(kind: TaskKindTag, origin: TaskOrigin) -> PreemptionLevel {
    match kind {
        // Background work — Autonomous(1) regardless of origin.
        TaskKindTag::Haul
        | TaskKindTag::Cook
        | TaskKindTag::Craft
        | TaskKindTag::Harvest
        | TaskKindTag::ExtractFruit => match origin {
            TaskOrigin::PlayerDirected | TaskOrigin::Autonomous | TaskOrigin::Automated => {
                PreemptionLevel::Autonomous
            }
        },

        // Player-directed non-combat — PlayerDirected(2).
        TaskKindTag::GoTo | TaskKindTag::Build | TaskKindTag::Furnish => match origin {
            TaskOrigin::PlayerDirected | TaskOrigin::Autonomous | TaskOrigin::Automated => {
                PreemptionLevel::PlayerDirected
            }
        },

        // Self-care — Survival(3).
        TaskKindTag::EatBread
        | TaskKindTag::EatFruit
        | TaskKindTag::Sleep
        | TaskKindTag::AcquireItem => match origin {
            TaskOrigin::PlayerDirected | TaskOrigin::Autonomous | TaskOrigin::Automated => {
                PreemptionLevel::Survival
            }
        },

        // Mood crisis — Mood(4).
        TaskKindTag::Mope => match origin {
            TaskOrigin::PlayerDirected | TaskOrigin::Autonomous | TaskOrigin::Automated => {
                PreemptionLevel::Mood
            }
        },

        // Player-directed combat — PlayerCombat(6).
        TaskKindTag::AttackTarget => match origin {
            TaskOrigin::PlayerDirected => PreemptionLevel::PlayerCombat,
            TaskOrigin::Autonomous | TaskOrigin::Automated => PreemptionLevel::AutonomousCombat,
        },
    }
}

/// Determine whether a new task (with the given level and origin) can preempt
/// the creature's current task (with the given level and origin).
///
/// Rules:
/// 1. Standard: new level > current level → preempt.
/// 2. Hardcoded exception: Mood never preempts Survival.
/// 3. PlayerDirected override: any PlayerDirected task can preempt
///    AutonomousCombat (player is ultimate authority).
/// 4. Same-level: no preemption by default. Exceptions:
///    - PlayerDirected replaces PlayerDirected (player changed their mind).
///    - PlayerCombat replaces PlayerCombat (retargeting).
pub fn can_preempt(
    current_level: PreemptionLevel,
    current_origin: TaskOrigin,
    new_level: PreemptionLevel,
    new_origin: TaskOrigin,
) -> bool {
    let cur = current_level.level();
    let new = new_level.level();

    // Hardcoded exception: Mood never preempts Survival.
    if new_level == PreemptionLevel::Mood && current_level == PreemptionLevel::Survival {
        return false;
    }

    // PlayerDirected override: can preempt AutonomousCombat.
    if current_level == PreemptionLevel::AutonomousCombat
        && new_origin == TaskOrigin::PlayerDirected
    {
        return true;
    }

    // Standard rule: strictly higher level preempts.
    if new > cur {
        return true;
    }

    // Same-level replacement for explicit player commands.
    if new == cur {
        // PlayerDirected replaces PlayerDirected.
        if current_origin == TaskOrigin::PlayerDirected
            && new_origin == TaskOrigin::PlayerDirected
            && current_level == PreemptionLevel::PlayerDirected
        {
            return true;
        }
        // PlayerCombat replaces PlayerCombat.
        if current_origin == TaskOrigin::PlayerDirected
            && new_origin == TaskOrigin::PlayerDirected
            && current_level == PreemptionLevel::PlayerCombat
        {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // PreemptionLevel::level() ordering
    // -----------------------------------------------------------------------

    #[test]
    fn level_ordering_is_monotonic() {
        let levels = [
            PreemptionLevel::Idle,
            PreemptionLevel::Autonomous,
            PreemptionLevel::PlayerDirected,
            PreemptionLevel::Survival,
            PreemptionLevel::Mood,
            PreemptionLevel::AutonomousCombat,
            PreemptionLevel::PlayerCombat,
            PreemptionLevel::Flee,
        ];
        for pair in levels.windows(2) {
            assert!(
                pair[0].level() < pair[1].level(),
                "{:?} ({}) should be < {:?} ({})",
                pair[0],
                pair[0].level(),
                pair[1],
                pair[1].level()
            );
        }
    }

    // -----------------------------------------------------------------------
    // preemption_level() mapping
    // -----------------------------------------------------------------------

    #[test]
    fn background_work_maps_to_autonomous() {
        for kind in [
            TaskKindTag::Haul,
            TaskKindTag::Cook,
            TaskKindTag::Craft,
            TaskKindTag::Harvest,
        ] {
            for origin in [
                TaskOrigin::Autonomous,
                TaskOrigin::Automated,
                TaskOrigin::PlayerDirected,
            ] {
                assert_eq!(
                    preemption_level(kind, origin),
                    PreemptionLevel::Autonomous,
                    "{kind:?}/{origin:?} should be Autonomous"
                );
            }
        }
    }

    #[test]
    fn player_commands_map_to_player_directed() {
        for kind in [TaskKindTag::GoTo, TaskKindTag::Build, TaskKindTag::Furnish] {
            assert_eq!(
                preemption_level(kind, TaskOrigin::PlayerDirected),
                PreemptionLevel::PlayerDirected,
            );
        }
    }

    #[test]
    fn self_care_maps_to_survival() {
        for kind in [
            TaskKindTag::EatBread,
            TaskKindTag::EatFruit,
            TaskKindTag::Sleep,
            TaskKindTag::AcquireItem,
        ] {
            assert_eq!(
                preemption_level(kind, TaskOrigin::Autonomous),
                PreemptionLevel::Survival,
            );
        }
    }

    #[test]
    fn mope_maps_to_mood() {
        assert_eq!(
            preemption_level(TaskKindTag::Mope, TaskOrigin::Autonomous),
            PreemptionLevel::Mood,
        );
    }

    // -----------------------------------------------------------------------
    // can_preempt() — standard rule
    // -----------------------------------------------------------------------

    #[test]
    fn higher_level_preempts_lower() {
        // Mood(4) preempts Autonomous(1)
        assert!(can_preempt(
            PreemptionLevel::Autonomous,
            TaskOrigin::Autonomous,
            PreemptionLevel::Mood,
            TaskOrigin::Autonomous,
        ));
    }

    #[test]
    fn lower_level_does_not_preempt_higher() {
        // Autonomous(1) cannot preempt PlayerDirected(2)
        assert!(!can_preempt(
            PreemptionLevel::PlayerDirected,
            TaskOrigin::PlayerDirected,
            PreemptionLevel::Autonomous,
            TaskOrigin::Autonomous,
        ));
    }

    #[test]
    fn same_level_does_not_preempt_by_default() {
        // Autonomous vs Autonomous — no preemption
        assert!(!can_preempt(
            PreemptionLevel::Autonomous,
            TaskOrigin::Autonomous,
            PreemptionLevel::Autonomous,
            TaskOrigin::Autonomous,
        ));
    }

    // -----------------------------------------------------------------------
    // can_preempt() — Mood vs Survival exception
    // -----------------------------------------------------------------------

    #[test]
    fn mood_does_not_preempt_survival() {
        // Mood(4) > Survival(3) numerically, but hardcoded exception prevents it.
        assert!(!can_preempt(
            PreemptionLevel::Survival,
            TaskOrigin::Autonomous,
            PreemptionLevel::Mood,
            TaskOrigin::Autonomous,
        ));
    }

    #[test]
    fn survival_does_not_preempt_mood() {
        // Survival(3) < Mood(4), so standard rule already prevents this.
        assert!(!can_preempt(
            PreemptionLevel::Mood,
            TaskOrigin::Autonomous,
            PreemptionLevel::Survival,
            TaskOrigin::Autonomous,
        ));
    }

    // -----------------------------------------------------------------------
    // can_preempt() — Mood preempts lower levels
    // -----------------------------------------------------------------------

    #[test]
    fn mood_preempts_autonomous() {
        assert!(can_preempt(
            PreemptionLevel::Autonomous,
            TaskOrigin::Autonomous,
            PreemptionLevel::Mood,
            TaskOrigin::Autonomous,
        ));
    }

    #[test]
    fn mood_preempts_player_directed() {
        assert!(can_preempt(
            PreemptionLevel::PlayerDirected,
            TaskOrigin::PlayerDirected,
            PreemptionLevel::Mood,
            TaskOrigin::Autonomous,
        ));
    }

    #[test]
    fn mood_preempts_idle() {
        assert!(can_preempt(
            PreemptionLevel::Idle,
            TaskOrigin::Autonomous,
            PreemptionLevel::Mood,
            TaskOrigin::Autonomous,
        ));
    }

    // -----------------------------------------------------------------------
    // can_preempt() — PlayerDirected override of AutonomousCombat
    // -----------------------------------------------------------------------

    #[test]
    fn player_directed_preempts_autonomous_combat() {
        // PlayerDirected(2) < AutonomousCombat(5) numerically, but override.
        assert!(can_preempt(
            PreemptionLevel::AutonomousCombat,
            TaskOrigin::Autonomous,
            PreemptionLevel::PlayerDirected,
            TaskOrigin::PlayerDirected,
        ));
    }

    #[test]
    fn player_directed_does_not_preempt_player_combat() {
        // PlayerDirected(2) does NOT override PlayerCombat(6).
        assert!(!can_preempt(
            PreemptionLevel::PlayerCombat,
            TaskOrigin::PlayerDirected,
            PreemptionLevel::PlayerDirected,
            TaskOrigin::PlayerDirected,
        ));
    }

    // -----------------------------------------------------------------------
    // can_preempt() — same-level replacement
    // -----------------------------------------------------------------------

    #[test]
    fn player_directed_replaces_player_directed() {
        assert!(can_preempt(
            PreemptionLevel::PlayerDirected,
            TaskOrigin::PlayerDirected,
            PreemptionLevel::PlayerDirected,
            TaskOrigin::PlayerDirected,
        ));
    }

    #[test]
    fn player_combat_replaces_player_combat() {
        assert!(can_preempt(
            PreemptionLevel::PlayerCombat,
            TaskOrigin::PlayerDirected,
            PreemptionLevel::PlayerCombat,
            TaskOrigin::PlayerDirected,
        ));
    }

    #[test]
    fn autonomous_combat_does_not_replace_autonomous_combat() {
        assert!(!can_preempt(
            PreemptionLevel::AutonomousCombat,
            TaskOrigin::Autonomous,
            PreemptionLevel::AutonomousCombat,
            TaskOrigin::Autonomous,
        ));
    }

    // -----------------------------------------------------------------------
    // can_preempt() — combat preempts mood
    // -----------------------------------------------------------------------

    #[test]
    fn autonomous_combat_preempts_mood() {
        assert!(can_preempt(
            PreemptionLevel::Mood,
            TaskOrigin::Autonomous,
            PreemptionLevel::AutonomousCombat,
            TaskOrigin::Autonomous,
        ));
    }

    #[test]
    fn flee_preempts_mood() {
        assert!(can_preempt(
            PreemptionLevel::Mood,
            TaskOrigin::Autonomous,
            PreemptionLevel::Flee,
            TaskOrigin::Autonomous,
        ));
    }

    #[test]
    fn flee_preempts_player_combat() {
        assert!(can_preempt(
            PreemptionLevel::PlayerCombat,
            TaskOrigin::PlayerDirected,
            PreemptionLevel::Flee,
            TaskOrigin::Autonomous,
        ));
    }

    // -----------------------------------------------------------------------
    // Serde roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn preemption_level_serde_roundtrip() {
        for level in [
            PreemptionLevel::Idle,
            PreemptionLevel::Autonomous,
            PreemptionLevel::PlayerDirected,
            PreemptionLevel::Survival,
            PreemptionLevel::Mood,
            PreemptionLevel::AutonomousCombat,
            PreemptionLevel::PlayerCombat,
            PreemptionLevel::Flee,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let restored: PreemptionLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, restored);
        }
    }
}
