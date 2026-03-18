# War Magic Design (F-war-magic)

## Overview

Combat magic for elves (and potentially other magical creatures). Spells are
active abilities cast from a per-elf mana pool, controlled via an RTS-style
command card (F-ability-hotkeys). Three spells in the first pass: Mend
(healing), Rootbind (crowd control), and Ice Shard (ranged damage).

## Mana Model

Per-elf mana pools. The mana system (F-mana-system) already tracks `mp` and
`mp_max` on creatures. Spells consume mp directly. Tree-spirit mana transfer
(letting elves draw from the tree's pool) is a later feature — for now, elves
are limited to their personal regeneration.

Key stats for magic:

| Stat | Role in magic |
|------|---------------|
| WIL  | Mana pool size, mana regen rate |
| INT  | Spell effectiveness (damage, heal amount, duration) |
| CHA  | Future: singing/buff effectiveness |

These stats are already rolled and stored (F-creature-stats) but currently
inert. War magic is one of their first mechanical hooks.

## Command Card (F-ability-hotkeys)

When a creature with abilities is selected, an ability panel appears in the UI
(StarCraft-style command card). Each ability is a button with:

- **Icon** showing the spell
- **Hotkey letter** displayed on the button
- **Mana cost** shown on or near the button
- **Cooldown indicator** (if applicable)
- **Autocast ring** — glowing border when autocast is enabled

### Controls

| Action | Input |
|--------|-------|
| Manual cast | Left-click button (or press hotkey), then click target |
| Autocast toggle | Right-click button |
| Cancel targeting | Right-click ground / Escape |
| Group autocast | Select multiple elves, right-click button — all toggle |

When manual casting, the cursor changes to a targeting reticle. Left-click a
valid target to cast; right-click or Escape to cancel.

Group behavior: if you select 5 mage-elves and right-click Mend, all 5 toggle
autocast. If some already have it on, the toggle is per-elf (each flips).

## First-Pass Spells

### Mend (Healing)

Single-target heal-over-time on a friendly creature.

- **Cast type:** Channeled (caster stands still while healing)
- **Target:** Single friendly creature (not self? TBD)
- **Range:** Short (adjacent or 2-3 voxels)
- **Mana cost:** Per tick of healing (not upfront), so partial heals are
  possible if mana runs out mid-channel
- **Heal amount:** Based on caster's INT (exponential scaling like other stats)
- **Autocast:** Yes. When enabled, the elf behaves like a StarCraft Medic:
  - Follows nearby injured friendlies
  - Prioritizes lowest HP% ally within range
  - Stops to heal, resumes following when target is full or out of range
  - Will not initiate combat — a Mend-autocasting elf is a dedicated healer
- **Manual cast:** Click Mend, click injured ally. Caster walks to target if
  needed, then channels.

Design note: mana-per-tick (rather than upfront cost) means a low-mana elf
can still do *some* healing rather than being completely locked out. It also
means interrupting a heal (target dies, caster is attacked) doesn't waste a
big mana lump.

### Rootbind (Stun / Immobilize)

Single-target crowd control. Roots burst from the nearest wooden surface and
entangle the target.

- **Cast type:** Instant (short cast animation, then effect applies)
- **Target:** Single enemy creature
- **Range:** Medium (maybe 5-8 voxels)
- **Mana cost:** Flat upfront cost
- **Duration:** Contested — `f(caster INT) vs f(target STR)`. A strong target
  tears free faster. Minimum duration of ~1 second so it's never completely
  useless; maximum cap so it can't be oppressive.
  - Possible formula: `base_duration * int_multiplier(caster) / str_multiplier(target)`
  - A high-INT elf rootbinding a goblin (low STR): long root.
  - A low-INT elf rootbinding a troll (high STR): very brief root.
- **Effect:** Target cannot move. Can still attack if an enemy is in melee
  range. Cannot use movement abilities (no blinking out).
- **Autocast:** No. Stuns are too valuable to spend automatically — the player
  should choose which target to lock down. (Revisitable if we find players
  want it.)
- **Diminishing returns:** TBD. Possibly not needed if mana cost is the
  limiter, but worth considering if perma-root becomes a problem with
  multiple casters.

### Ice Shard (Ranged Magic Damage)

Mana-fueled ranged projectile. The magical equivalent of an arrow.

- **Cast type:** Projectile (creates a sim projectile like arrows do)
- **Target:** Single enemy creature (or target point? arrows are aimed at
  creatures)
- **Range:** Similar to bow range
- **Mana cost:** Flat per-shot
- **Damage:** Based on caster's INT (where arrows use STR for velocity and DEX
  for accuracy). Ice Shard accuracy could also use INT, or possibly WIL for
  "mental focus." TBD.
- **Projectile behavior:** Travels like an arrow but uses magic stats instead
  of physical stats. Possibly no friendly-fire (magic projectiles could be
  target-seeking), or possibly the same FF rules as arrows — design choice.
- **Autocast:** Yes. When enabled, the elf fires Ice Shards at enemies within
  range, like an archer with infinite ammo but limited mana. Useful for
  magical "turret" elves stationed at chokepoints.
- **Manual cast:** Click Ice Shard, click enemy. Elf turns and fires.

The key distinction from archery: no ammo (arrows, bowstring durability), but
costs mana. An elf with high INT and WIL but low DEX/STR might be a terrible
archer but a good ice shard caster.

## Spell Learning

Spells must be learned (not innate). The library building (F-bldg-library)
is where elves study and learn spells. Details of the learning/research
mechanic are deferred to that feature's design. For first-pass implementation,
spells could be granted via debug command to unblock testing.

## Future Spell Ideas

These are not part of the first pass but are documented for future reference.
Roughly grouped by theme.

### Conjuration
- **Summon Creature** — Conjure a temporary allied creature (elephant, giant
  hornet, bee swarm). Duration-limited, consumes significant mana. The
  creature fights independently with its own AI. Disappears when duration
  expires or when killed.

### Mind / Spirit
- **Mind Control** — Temporarily take control of an enemy creature. High mana
  cost, duration contested (caster INT vs target WIL). Controlled creature
  fights for you.
- **Berserk** — Buff an allied creature: increased damage and speed, but
  they attack the nearest creature (friend or foe) uncontrollably.

### Stealth
- **Cloak** — Invisibility on self, or AoE cloak on nearby allies. Enemies
  cannot detect cloaked creatures unless they get very close (PER-based
  detection). Breaks on attack or spell cast.

### Archery Enhancement
- **Enchanted Arrow** — A special arrow shot costing mana. Could add effects:
  fire damage over time, piercing (hits multiple targets), slowing, etc.
  Uses normal archery stats + mana. Could be autocastable (every Nth arrow
  is enchanted).

### Terrain Manipulation
- **Thornbriar** — Grow a patch of thorny bushes in a target area. Creatures
  moving through the area take damage and move slower. Duration-limited
  (bushes wither after a time). Could block a chokepoint or slow a charge.
  Area-of-effect targeting (click a point, affects a radius).
- **Gust** — AoE knockback in a cone. Pushes creatures away from the caster.
  Most interesting for pushing enemies off platforms (falling damage). Requires
  creature gravity to be implemented first.

### Movement
- **Blink** — Short-range teleport. Instant. High mana cost, moderate
  cooldown. Useful for repositioning healers, escaping melee, or reaching
  elevated platforms. Range maybe 5-10 voxels, line-of-sight not required
  (it's teleportation), but destination must be a valid walkable voxel.

### Combat Singing (far future)
- Elves singing in combat to buff allies — attack speed, damage, morale,
  mana regen. Could evolve into a musical instrument / band system where
  different instruments provide different buffs, and having a full ensemble
  produces harmony bonuses. Mechanically distinct from construction choir
  singing (F-choir-build / F-choir-harmony). The music crate
  (elven_canopy_music) and lang crate (elven_canopy_lang) already generate
  Vaelith lyrics and polyphonic music — this could eventually tie in.

## Implementation Considerations

### Sim-Side

- New `SpellId` enum (Mend, Rootbind, IceShard).
- New `SimCommand` variants: `CastSpell { caster, target, spell }`,
  `SetAutocast { creature, spell, enabled }`.
- New task type for channeled spells (Mend). Rootbind and Ice Shard are
  instant/projectile — might not need a full task, just an action within the
  combat/activation system.
- Autocast state stored per-creature per-spell (bitfield or small vec).
- Mend autocast drives a new AI behavior: scan for injured friendlies, walk
  to them, heal. Similar to existing task assignment but spell-driven.
- Ice Shard autocast integrates with the existing ranged combat system — the
  creature "shoots" but with a magic projectile instead of an arrow.
- Rootbind applies a status effect (new concept): immobilized for N ticks.
  Status effects are a new system — even if Rootbind is the only one at
  first, the system should be generic enough for future spells.

### GDScript-Side (F-ability-hotkeys)

- Command card UI panel (bottom-right, like SC).
- Buttons generated from the selected creature's known spells.
- Autocast toggle visual state (glowing border).
- Targeting mode: cursor changes, valid target highlighting.
- Spell effect visuals (ice shard projectile sprite, root tendrils growing
  around target, healing glow).

### Prerequisites and Ordering

The first-pass implementation has these dependencies:

1. **F-elf-mana-pool** — Wire WIL/INT to mana pool size and regen rate.
   Prereq: F-mana-system (done), F-creature-stats (done).
2. **F-status-effects** — Generic status effect system (at minimum:
   immobilized). Prereq: none beyond existing sim infrastructure.
3. **F-spell-system** — Core spell casting infrastructure: SpellId, cast
   commands, mana costs, cooldowns, spell knowledge per creature.
   Prereq: F-elf-mana-pool, F-status-effects.
4. **F-spell-mend** — Mend spell implementation + autocast healer AI.
   Prereq: F-spell-system.
5. **F-spell-rootbind** — Rootbind spell implementation.
   Prereq: F-spell-system, F-status-effects.
6. **F-spell-ice-shard** — Ice Shard spell implementation + autocast.
   Prereq: F-spell-system, F-projectiles (done).
7. **F-ability-hotkeys** — Command card UI, targeting mode, autocast toggle.
   Prereq: F-spell-system (needs spell data to display).

These can be partially parallelized: F-elf-mana-pool and F-status-effects
have no mutual dependency. F-ability-hotkeys could be developed alongside
the spells with stub data.

## Open Questions

- **Self-heal with Mend?** Allowing self-cast makes healers more survivable
  but reduces the need for positioning and protection. SC Medics can't
  self-heal. Leaning toward no self-cast.
- **Ice Shard friendly fire?** Arrows have FF avoidance. Magic projectiles
  could be "smarter" (no FF) as a perk of using mana. Or same FF rules for
  consistency.
- **Cooldowns?** SC spells generally don't have cooldowns (just energy cost).
  Mana cost alone might be sufficient rate-limiting. Rootbind might need a
  per-target cooldown to prevent chain-stunning.
- **Cast interruption?** If a caster takes damage while channeling Mend,
  does the channel break? Adds counterplay but might be frustrating. Could
  be a threshold (damage > X% of HP interrupts).
- **Spell range scaling?** Fixed range, or does INT increase range? Leaning
  toward fixed (simpler, more predictable for the player).
