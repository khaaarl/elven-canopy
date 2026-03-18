// Raid triggering — spawns hostile raiding parties from enemy civilizations.
//
// The `trigger_raid()` method picks a random hostile civ, spawns a species-
// appropriate number of raiders at the forest floor perimeter, and gives each
// an attack-move task toward the tree. Currently debug-only (no periodic
// trigger or detection gating).
//
// See also: `creature.rs` for `spawn_creature_with_civ()`, `combat.rs` for
// `command_attack_move()`, `worldgen.rs` for civ/relationship generation,
// `types.rs` for `CivSpecies::to_species()`.

use super::*;
use crate::event::SimEvent;

impl SimState {
    /// Trigger a raid from a random hostile civilization.
    ///
    /// 1. Finds all civs hostile to the player.
    /// 2. Picks one at random.
    /// 3. Spawns `raid_size` creatures at the forest floor perimeter.
    /// 4. Attack-moves each toward a wood-adjacent nav node near the tree.
    /// 5. Fires a notification.
    pub(crate) fn trigger_raid(&mut self, events: &mut Vec<SimEvent>) {
        let player_civ = match self.player_civ_id {
            Some(id) => id,
            None => return,
        };

        // Find civs that consider the player hostile (they hate us → they raid).
        // A civ we hate but that doesn't hate us wouldn't send a raiding party.
        let hostile_civs: Vec<CivId> = self
            .db
            .civ_relationships
            .by_to_civ(&player_civ, tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|rel| rel.opinion == CivOpinion::Hostile)
            .map(|rel| rel.from_civ)
            .collect();

        if hostile_civs.is_empty() {
            self.add_notification(
                "No hostile civilizations known — no raid triggered.".to_string(),
            );
            return;
        }

        // Pick a random hostile civ.
        let idx = (self.rng.next_u64() % hostile_civs.len() as u64) as usize;
        let raiding_civ_id = hostile_civs[idx];

        let raiding_civ = match self.db.civilizations.get(&raiding_civ_id) {
            Some(c) => c.clone(),
            None => return,
        };

        // A raid is an overt act — the player civ now knows about and hates
        // the attackers. This ensures elves treat raiders as hostile (red on
        // minimap, combat triggers). discover_civ is a no-op if already aware;
        // set_civ_opinion upgrades any existing non-hostile opinion.
        self.discover_civ(player_civ, raiding_civ_id, CivOpinion::Hostile);
        self.set_civ_opinion(player_civ, raiding_civ_id, CivOpinion::Hostile);

        let species = match raiding_civ.primary_species.to_species() {
            Some(s) => s,
            None => {
                self.add_notification(format!(
                    "The {} have no creature type yet — raid aborted.",
                    raiding_civ.name
                ));
                return;
            }
        };

        let raid_size = self.species_table[&species].raid_size;
        if raid_size == 0 {
            return;
        }

        // Pick a random cardinal direction (0=North, 1=South, 2=East, 3=West).
        let direction = (self.rng.next_u64() % 4) as u8;

        // Find perimeter spawn positions.
        let spawn_positions = self.find_perimeter_positions(direction, raid_size as usize);

        if spawn_positions.is_empty() {
            self.add_notification(
                "Raid triggered but no valid perimeter positions found.".to_string(),
            );
            return;
        }

        // Find attack-move targets near wood.
        let attack_targets = self.find_wood_adjacent_nodes(species);

        // Spawn raiders.
        let mut raider_ids = Vec::new();
        for &spawn_pos in &spawn_positions {
            if let Some(creature_id) =
                self.spawn_creature_with_civ(species, spawn_pos, Some(raiding_civ_id), events)
            {
                raider_ids.push(creature_id);
            }
        }

        if raider_ids.is_empty() {
            self.add_notification("Raid triggered but no raiders could be spawned.".to_string());
            return;
        }

        // Assign attack-move to each raider. Cancel the spawn-scheduled
        // activation first — command_attack_move schedules its own, and
        // having both causes duplicate activations (jerky double-step movement).
        for &raider_id in &raider_ids {
            self.event_queue.cancel_creature_activations(raider_id);
            let target = if attack_targets.is_empty() {
                // Fallback: attack-move toward world center at ground level.
                let cx = self.config.world_size.0 as i32 / 2;
                let cz = self.config.world_size.2 as i32 / 2;
                VoxelCoord::new(cx, 1, cz)
            } else {
                let target_idx = (self.rng.next_u64() % attack_targets.len() as u64) as usize;
                attack_targets[target_idx]
            };
            self.command_attack_move(raider_id, target, events);
        }

        let direction_name = match direction {
            0 => "north",
            1 => "south",
            2 => "east",
            _ => "west",
        };

        let species_name = raiding_civ.primary_species.display_str();
        self.add_notification(format!(
            "A {} raiding party ({} raiders) approaches from the {}!",
            species_name,
            raider_ids.len(),
            direction_name,
        ));
    }

    /// Find ground-level nav node positions at the forest floor perimeter in the
    /// given cardinal direction. Returns up to `count` positions.
    ///
    /// Note: uses `self.nav_graph` (1x1x1 graph). All current raiding species
    /// (Goblin, Orc, Troll) have 1x1x1 footprints. If a large-footprint species
    /// ever raids, this should switch to `graph_for_species(species)`.
    fn find_perimeter_positions(&self, direction: u8, count: usize) -> Vec<VoxelCoord> {
        let cx = self.config.world_size.0 as i32 / 2;
        let cz = self.config.world_size.2 as i32 / 2;
        let extent = self.config.floor_extent;

        // The perimeter band: positions within 2 voxels of the edge.
        let ground_nodes = self.nav_graph.ground_node_ids();

        let mut candidates: Vec<VoxelCoord> = ground_nodes
            .iter()
            .map(|&nid| self.nav_graph.node(nid).position)
            .filter(|pos| {
                match direction {
                    // North: low Z edge
                    0 => pos.z >= cz - extent && pos.z <= cz - extent + 2,
                    // South: high Z edge
                    1 => pos.z <= cz + extent && pos.z >= cz + extent - 2,
                    // East: high X edge
                    2 => pos.x <= cx + extent && pos.x >= cx + extent - 2,
                    // West: low X edge
                    _ => pos.x >= cx - extent && pos.x <= cx - extent + 2,
                }
            })
            .collect();

        // Sort for determinism, then pick up to `count` spread across the edge.
        candidates.sort();

        if candidates.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(count);
        if candidates.len() <= count {
            return candidates;
        }

        // Spread evenly across available positions.
        let step = candidates.len() / count;
        for i in 0..count {
            let idx = (i * step) % candidates.len();
            result.push(candidates[idx]);
        }
        result
    }

    /// Find nav node positions adjacent to wood voxels (Trunk, Branch, Root,
    /// GrownPlatform, GrownWall, GrownStairs, Bridge, Strut). Used as
    /// attack-move destinations for raiders.
    fn find_wood_adjacent_nodes(&self, species: Species) -> Vec<VoxelCoord> {
        let ground_only = self.species_table[&species].ground_only;
        let graph = self.graph_for_species(species);

        let mut targets = Vec::new();

        for node in graph.live_nodes() {
            // If species is ground_only, only consider ForestFloor nodes.
            if ground_only && node.surface_type != VoxelType::ForestFloor {
                continue;
            }

            // Check 6-connected neighbors for wood.
            let pos = node.position;
            let neighbors = [
                VoxelCoord::new(pos.x + 1, pos.y, pos.z),
                VoxelCoord::new(pos.x - 1, pos.y, pos.z),
                VoxelCoord::new(pos.x, pos.y + 1, pos.z),
                VoxelCoord::new(pos.x, pos.y - 1, pos.z),
                VoxelCoord::new(pos.x, pos.y, pos.z + 1),
                VoxelCoord::new(pos.x, pos.y, pos.z - 1),
            ];

            let near_wood = neighbors.iter().any(|&n| {
                matches!(
                    self.world.get(n),
                    VoxelType::Trunk
                        | VoxelType::Branch
                        | VoxelType::Root
                        | VoxelType::GrownPlatform
                        | VoxelType::GrownWall
                        | VoxelType::GrownStairs
                        | VoxelType::Bridge
                        | VoxelType::Strut
                )
            });

            if near_wood {
                targets.push(pos);
            }
        }

        targets
    }
}
