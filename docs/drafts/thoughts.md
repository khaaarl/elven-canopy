# Creature Thoughts — Design Plan

Dwarf Fortress-inspired thought system. Creatures accumulate thoughts in response
to events. Each thought has a kind (with associated data), a timestamp, and
per-kind properties for dedup cooldown and expiry. Thoughts are displayed on the
creature info panel and will later feed into the emotional dimension system
(`F-emotions`).

---

## 1. Data Structures (sim crate)

### ThoughtKind

Enum with data in variants. Derives `PartialEq` for dedup comparison.

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum ThoughtKind {
    SleptInOwnHome(StructureId),
    SleptOnGround,
    SleptInDormitory(StructureId),
    AteMeal,
    LowCeiling(StructureId),
    // Future: WentHungry, Exhausted, SawSculpture(StructureId), LostFriend(CreatureId), ...
}
```

Each variant carries only the IDs needed to distinguish meaningfully different
instances of the same thought. "Low ceiling in building A" and "low ceiling in
building B" are distinct thoughts.

### ThoughtKind methods

```rust
impl ThoughtKind {
    /// Ticks before an identical thought can be added again.
    /// The mood effect (when it exists) still applies on duplicates.
    pub fn dedup_cooldown_ticks(&self) -> u64 { ... }

    /// Ticks after which this thought expires and is removed.
    pub fn expiry_ticks(&self) -> u64 { ... }

    /// Human-readable description for UI display.
    pub fn description(&self) -> &'static str { ... }
}
```

Cooldown and expiry are per-kind. Examples (assuming 1000 ticks/sec):

| Kind              | Dedup cooldown | Expiry         | Rationale                                    |
|-------------------|----------------|----------------|----------------------------------------------|
| SleptInOwnHome    | 1 day cycle    | Medium (~days)  | Once per sleep, fades after a few days       |
| SleptOnGround     | 1 day cycle    | Medium (~days)  | Same cadence as home sleep                   |
| SleptInDormitory  | 1 day cycle    | Medium (~days)  | Same cadence as home sleep                   |
| AteMeal           | 1 day cycle    | Short (~1 day)  | Routine, fades fast                          |
| LowCeiling        | ~30 min        | Short (~1 day)  | Reminded each visit, fades quickly           |

Exact tick values will live in `GameConfig` so they're tunable without recompile.

### Thought

```rust
#[derive(Clone, Debug)]
pub struct Thought {
    pub kind: ThoughtKind,
    pub tick: u64,
}
```

### Creature changes

Add to `Creature`:

```rust
pub thoughts: Vec<Thought>,
```

Hard-capped at 200 entries. When adding beyond the cap, drop the oldest thought.

---

## 2. Thought Generation

Thoughts are generated at specific moments in the sim tick loop, not every tick.

### Trigger points

| Thought           | Trigger                                                             |
|-------------------|---------------------------------------------------------------------|
| SleptInOwnHome    | Sleep task completes, bed was in creature's `assigned_home`         |
| SleptInDormitory  | Sleep task completes, bed was in a dormitory (not assigned home)    |
| SleptOnGround     | Ground-sleep task completes                                        |
| AteMeal           | Food task completes                                                |
| LowCeiling        | Creature enters or starts an activity inside a 1-voxel-height structure |

### Adding a thought

```rust
impl Creature {
    pub fn add_thought(&mut self, kind: ThoughtKind, tick: u64) {
        // 1. Check dedup: scan backwards for identical kind within cooldown window
        let dominated = self.thoughts.iter().rev().any(|t| {
            t.kind == kind && tick.saturating_sub(t.tick) < kind.dedup_cooldown_ticks()
        });
        if dominated {
            return; // Skip add, but caller can still apply mood effects
        }

        // 2. Add the thought
        self.thoughts.push(Thought { kind, tick });

        // 3. Enforce hard cap (drop oldest)
        if self.thoughts.len() > THOUGHT_CAP {
            self.thoughts.remove(0);
        }
    }
}
```

Note: The `add_thought` method returns early on dedup. When mood effects exist,
the caller will apply them regardless — the dedup only controls whether the
thought appears in the UI list.

### Expiry cleanup

During the creature's heartbeat tick, remove expired thoughts:

```rust
self.thoughts.retain(|t| {
    tick.saturating_sub(t.tick) < t.kind.expiry_ticks()
});
```

This runs infrequently (once per heartbeat, not every tick), so the cost is
negligible.

---

## 3. Low Ceiling Detection

A structure has a "low ceiling" if its interior height is 1 voxel. This means
the structure's `BuildType` creates an enclosed space (walls + roof) and the
floor-to-ceiling gap is exactly 1.

Detection approach: when a creature starts an activity inside a completed
structure (sleep, eat, work), check the structure's interior height. If it's 1,
generate `LowCeiling(structure_id)`.

The interior height could be:
- Stored on `CompletedStructure` at build time (computed once from the blueprint
  dimensions minus wall/roof thickness).
- Or computed on the fly from the structure's bounding box.

Storing it is cleaner since it doesn't change after construction.

---

## 4. UI Display (GDScript)

### SimBridge additions

Expose thought data through `SimBridge`:

```rust
pub fn get_creature_thoughts(&self, creature_id: u64) -> PackedStringArray {
    // Return descriptions of active (non-expired) thoughts, most recent first
}
```

Could also return tick timestamps for "X time ago" display, but a simple
description list is a good starting point.

### Creature info panel

Add a "Thoughts" section to `creature_info_panel.gd` below the existing
creature info. Display as a scrollable list of thought descriptions, most recent
first. Each entry could show:

- The thought description text (from `ThoughtKind::description()`)
- Optionally, how long ago it occurred ("recently", "a while ago")
- Optionally, a positive/negative indicator (color or icon)

Keep it simple initially — just a list of strings.

---

## 5. Future Hooks

- **Mood effects:** When `F-emotions` lands, each `ThoughtKind` will carry a
  mood modifier (e.g., `SleptInOwnHome` → +joy, `LowCeiling` → +stress).
  The `add_thought` call site will apply the modifier regardless of dedup.
- **Personality interaction:** `F-personality` could modulate thought intensity
  (a claustrophobic elf hates low ceilings more).
- **Loose dedup:** For families of similar thoughts (e.g., seeing multiple
  sculptures), a future `ThoughtKind::dedup_group()` method could return an
  optional group ID, and dedup would also check against recent thoughts in the
  same group.

---

## 6. Determinism

All thought generation is triggered by sim events at specific ticks, using only
sim state. No randomness is involved in thought generation (though future
personality-based variance would use the deterministic PRNG). The `Vec<Thought>`
is ordered by insertion, which is deterministic. Safe for replays and
multiplayer.

---

## 7. Serialization

Thoughts must be included in save files. `Thought` and `ThoughtKind` will derive
`Serialize`/`Deserialize` (serde). The save format version will be bumped when
this is added.
