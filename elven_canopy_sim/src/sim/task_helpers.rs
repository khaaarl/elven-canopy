// Task query helpers and insertion — extension table accessors for decomposed tasks.
//
// Tasks are stored across multiple tabulosity tables (base task + extension
// tables for haul data, sleep data, craft data, etc.). These helper methods
// abstract the multi-table lookups into single calls. `insert_task` is the
// canonical way to create a task, decomposing a `Task` DTO into the
// appropriate DB rows.
//
// See also: `task.rs` (Task DTO and TaskKind), `db.rs` (table definitions),
// `activation.rs` (task claiming and execution).
use super::*;
use crate::inventory;
use crate::task;

impl SimState {
    /// Get the TaskCraftData for a task, if it exists.
    pub(crate) fn task_craft_data(&self, task_id: TaskId) -> Option<crate::db::TaskCraftData> {
        self.db.task_craft_data.get(&task_id)
    }

    /// Get the AttackTarget extension data for a task.
    pub(crate) fn task_attack_target_data(
        &self,
        task_id: TaskId,
    ) -> Option<crate::db::TaskAttackTargetData> {
        self.db.task_attack_target_data.get(&task_id)
    }

    /// Get the AttackMove extension data for a task.
    pub(crate) fn task_attack_move_data(
        &self,
        task_id: TaskId,
    ) -> Option<crate::db::TaskAttackMoveData> {
        self.db.task_attack_move_data.get(&task_id)
    }

    /// Get the project_id for a Build task from the task_blueprint_refs table.
    pub(crate) fn task_project_id(&self, task_id: TaskId) -> Option<ProjectId> {
        self.db
            .task_blueprint_refs
            .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .map(|r| r.project_id)
    }

    /// Get a structure_id from a task's structure refs by role.
    pub(crate) fn task_structure_ref(
        &self,
        task_id: TaskId,
        role: crate::db::TaskStructureRole,
    ) -> Option<StructureId> {
        self.db
            .task_structure_refs
            .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|r| r.role == role)
            .map(|r| r.structure_id)
    }

    /// Get a voxel coord from a task's voxel refs by role.
    pub(crate) fn task_voxel_ref(
        &self,
        task_id: TaskId,
        role: crate::db::TaskVoxelRole,
    ) -> Option<VoxelCoord> {
        self.db
            .task_voxel_refs
            .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|r| r.role == role)
            .map(|r| r.coord)
    }

    /// Get the haul data for a Haul task.
    pub(crate) fn task_haul_data(&self, task_id: TaskId) -> Option<crate::db::TaskHaulData> {
        self.db.task_haul_data.get(&task_id)
    }

    /// Get the sleep data for a Sleep task.
    pub(crate) fn task_sleep_data(&self, task_id: TaskId) -> Option<crate::db::TaskSleepData> {
        self.db.task_sleep_data.get(&task_id)
    }

    /// Get the acquire data for an AcquireItem task.
    pub(crate) fn task_acquire_data(&self, task_id: TaskId) -> Option<crate::db::TaskAcquireData> {
        self.db.task_acquire_data.get(&task_id)
    }

    /// Reconstruct a HaulSource enum from the task's extension tables.
    pub(crate) fn task_haul_source(
        &self,
        task_id: TaskId,
        source_kind: crate::db::HaulSourceKind,
    ) -> Option<task::HaulSource> {
        match source_kind {
            crate::db::HaulSourceKind::Pile => {
                let pos = self.task_voxel_ref(task_id, crate::db::TaskVoxelRole::HaulSourcePile)?;
                Some(task::HaulSource::GroundPile(pos))
            }
            crate::db::HaulSourceKind::Building => {
                let sid = self.task_structure_ref(
                    task_id,
                    crate::db::TaskStructureRole::HaulSourceBuilding,
                )?;
                Some(task::HaulSource::Building(sid))
            }
        }
    }

    /// Reconstruct a HaulSource for an AcquireItem task.
    pub(crate) fn task_acquire_source(
        &self,
        task_id: TaskId,
        source_kind: crate::db::HaulSourceKind,
    ) -> Option<task::HaulSource> {
        match source_kind {
            crate::db::HaulSourceKind::Pile => {
                let pos =
                    self.task_voxel_ref(task_id, crate::db::TaskVoxelRole::AcquireSourcePile)?;
                Some(task::HaulSource::GroundPile(pos))
            }
            crate::db::HaulSourceKind::Building => {
                let sid = self.task_structure_ref(
                    task_id,
                    crate::db::TaskStructureRole::AcquireSourceBuilding,
                )?;
                Some(task::HaulSource::Building(sid))
            }
        }
    }

    /// Reconstruct a SleepLocation from the task's extension tables.
    pub(crate) fn task_sleep_location(&self, task_id: TaskId) -> Option<task::SleepLocation> {
        let sleep_data = self.task_sleep_data(task_id)?;
        match sleep_data.sleep_location {
            crate::db::SleepLocationType::Home => {
                let sid =
                    self.task_structure_ref(task_id, crate::db::TaskStructureRole::SleepAt)?;
                Some(task::SleepLocation::Home(sid))
            }
            crate::db::SleepLocationType::Dormitory => {
                let sid =
                    self.task_structure_ref(task_id, crate::db::TaskStructureRole::SleepAt)?;
                Some(task::SleepLocation::Dormitory(sid))
            }
            crate::db::SleepLocationType::Ground => Some(task::SleepLocation::Ground),
        }
    }

    /// Insert a task and populate its relationship/extension tables based on kind.
    pub(crate) fn insert_task(&mut self, zone_id: ZoneId, task: task::Task) {
        let task_id = task.id;
        let kind = &task.kind;

        // Insert the base task row first (extension tables have FK → tasks).
        let db_task = crate::db::Task {
            id: task.id,
            zone_id,
            kind_tag: crate::db::TaskKindTag::from_kind(kind),
            state: task.state,
            location: task.location,
            progress: task.progress,
            total_cost: task.total_cost,
            required_species: task.required_species,
            origin: task.origin,
            target_creature: task.target_creature,
            restrict_to_creature_id: task.restrict_to_creature_id,
            prerequisite_task_id: task.prerequisite_task_id,
            required_civ_id: task.required_civ_id,
        };
        self.db.insert_task(db_task).unwrap();

        // Populate relationship and extension tables.
        match kind {
            task::TaskKind::Build { project_id } => {
                let seq = self.db.task_blueprint_refs.next_seq();
                let _ = self
                    .db
                    .insert_task_blueprint_ref(crate::db::TaskBlueprintRef {
                        task_id,
                        seq,
                        project_id: *project_id,
                    });
            }
            task::TaskKind::DineAtHall { structure_id } => {
                let seq = self.db.task_structure_refs.next_seq();
                let _ = self
                    .db
                    .insert_task_structure_ref(crate::db::TaskStructureRef {
                        seq,
                        task_id,
                        structure_id: *structure_id,
                        role: crate::db::TaskStructureRole::DineAt,
                    });
                // DiningSeat voxel ref is inserted at task creation site (heartbeat),
                // since the table position is determined during find_nearest_dining_hall.
            }
            task::TaskKind::EatFruit { fruit_pos } | task::TaskKind::Harvest { fruit_pos } => {
                let seq = self.db.task_voxel_refs.next_seq();
                let _ = self.db.insert_task_voxel_ref(crate::db::TaskVoxelRef {
                    seq,
                    task_id,
                    coord: *fruit_pos,
                    role: crate::db::TaskVoxelRole::FruitTarget,
                });
            }
            task::TaskKind::Furnish { structure_id } => {
                let seq = self.db.task_structure_refs.next_seq();
                let _ = self
                    .db
                    .insert_task_structure_ref(crate::db::TaskStructureRef {
                        seq,
                        task_id,
                        structure_id: *structure_id,
                        role: crate::db::TaskStructureRole::FurnishTarget,
                    });
            }
            task::TaskKind::Sleep { bed_pos, location } => {
                if let Some(pos) = bed_pos {
                    let seq = self.db.task_voxel_refs.next_seq();
                    let _ = self.db.insert_task_voxel_ref(crate::db::TaskVoxelRef {
                        seq,
                        task_id,
                        coord: *pos,
                        role: crate::db::TaskVoxelRole::BedPosition,
                    });
                }
                let sleep_loc = match location {
                    task::SleepLocation::Home(sid) => {
                        let seq = self.db.task_structure_refs.next_seq();
                        let _ = self
                            .db
                            .insert_task_structure_ref(crate::db::TaskStructureRef {
                                seq,
                                task_id,
                                structure_id: *sid,
                                role: crate::db::TaskStructureRole::SleepAt,
                            });
                        crate::db::SleepLocationType::Home
                    }
                    task::SleepLocation::Dormitory(sid) => {
                        let seq = self.db.task_structure_refs.next_seq();
                        let _ = self
                            .db
                            .insert_task_structure_ref(crate::db::TaskStructureRef {
                                seq,
                                task_id,
                                structure_id: *sid,
                                role: crate::db::TaskStructureRole::SleepAt,
                            });
                        crate::db::SleepLocationType::Dormitory
                    }
                    task::SleepLocation::Ground => crate::db::SleepLocationType::Ground,
                };
                let _ = self.db.insert_task_sleep_data(crate::db::TaskSleepData {
                    task_id,
                    sleep_location: sleep_loc,
                });
            }
            task::TaskKind::Haul {
                item_kind,
                quantity,
                source,
                destination,
                phase,
                destination_coord,
            } => {
                // Destination structure ref.
                let seq = self.db.task_structure_refs.next_seq();
                let _ = self
                    .db
                    .insert_task_structure_ref(crate::db::TaskStructureRef {
                        seq,
                        task_id,
                        structure_id: *destination,
                        role: crate::db::TaskStructureRole::HaulDestination,
                    });
                // Source ref.
                let source_kind = match source {
                    task::HaulSource::GroundPile(pos) => {
                        let seq = self.db.task_voxel_refs.next_seq();
                        let _ = self.db.insert_task_voxel_ref(crate::db::TaskVoxelRef {
                            seq,
                            task_id,
                            coord: *pos,
                            role: crate::db::TaskVoxelRole::HaulSourcePile,
                        });
                        crate::db::HaulSourceKind::Pile
                    }
                    task::HaulSource::Building(sid) => {
                        let seq = self.db.task_structure_refs.next_seq();
                        let _ = self
                            .db
                            .insert_task_structure_ref(crate::db::TaskStructureRef {
                                seq,
                                task_id,
                                structure_id: *sid,
                                role: crate::db::TaskStructureRole::HaulSourceBuilding,
                            });
                        crate::db::HaulSourceKind::Building
                    }
                };
                let _ = self.db.insert_task_haul_data(crate::db::TaskHaulData {
                    task_id,
                    item_kind: *item_kind,
                    quantity: *quantity,
                    phase: *phase,
                    source_kind,
                    destination_coord: *destination_coord,
                    material_filter: inventory::MaterialFilter::Any,
                    hauled_material: None,
                });
            }
            task::TaskKind::AcquireItem {
                source,
                item_kind,
                quantity,
            } => {
                let source_kind = match source {
                    task::HaulSource::GroundPile(pos) => {
                        let seq = self.db.task_voxel_refs.next_seq();
                        let _ = self.db.insert_task_voxel_ref(crate::db::TaskVoxelRef {
                            seq,
                            task_id,
                            coord: *pos,
                            role: crate::db::TaskVoxelRole::AcquireSourcePile,
                        });
                        crate::db::HaulSourceKind::Pile
                    }
                    task::HaulSource::Building(sid) => {
                        let seq = self.db.task_structure_refs.next_seq();
                        let _ = self
                            .db
                            .insert_task_structure_ref(crate::db::TaskStructureRef {
                                seq,
                                task_id,
                                structure_id: *sid,
                                role: crate::db::TaskStructureRole::AcquireSourceBuilding,
                            });
                        crate::db::HaulSourceKind::Building
                    }
                };
                let _ = self
                    .db
                    .insert_task_acquire_data(crate::db::TaskAcquireData {
                        task_id,
                        item_kind: *item_kind,
                        quantity: *quantity,
                        source_kind,
                    });
            }
            task::TaskKind::Craft {
                structure_id,
                active_recipe_id,
            } => {
                let seq = self.db.task_structure_refs.next_seq();
                let _ = self
                    .db
                    .insert_task_structure_ref(crate::db::TaskStructureRef {
                        seq,
                        task_id,
                        structure_id: *structure_id,
                        role: crate::db::TaskStructureRole::CraftAt,
                    });
                // TaskCraftData is populated by the crafting monitor after
                // insert_task — it overwrites recipe/material/active_recipe_id.
                // We insert a placeholder here for task decomposition.
                let _ = self.db.insert_task_craft_data(crate::db::TaskCraftData {
                    task_id,
                    recipe: crate::recipe::Recipe::Extract,
                    material: None,
                    active_recipe_id: *active_recipe_id,
                });
            }
            task::TaskKind::AttackTarget { target } => {
                let _ = self
                    .db
                    .insert_task_attack_target_data(crate::db::TaskAttackTargetData {
                        task_id,
                        target: *target,
                        path_failures: 0,
                    });
            }
            task::TaskKind::AcquireMilitaryEquipment {
                source,
                item_kind,
                quantity,
            } => {
                let source_kind = match source {
                    task::HaulSource::GroundPile(pos) => {
                        let seq = self.db.task_voxel_refs.next_seq();
                        let _ = self.db.insert_task_voxel_ref(crate::db::TaskVoxelRef {
                            seq,
                            task_id,
                            coord: *pos,
                            role: crate::db::TaskVoxelRole::AcquireSourcePile,
                        });
                        crate::db::HaulSourceKind::Pile
                    }
                    task::HaulSource::Building(sid) => {
                        let seq = self.db.task_structure_refs.next_seq();
                        let _ = self
                            .db
                            .insert_task_structure_ref(crate::db::TaskStructureRef {
                                seq,
                                task_id,
                                structure_id: *sid,
                                role: crate::db::TaskStructureRole::AcquireSourceBuilding,
                            });
                        crate::db::HaulSourceKind::Building
                    }
                };
                let _ = self
                    .db
                    .insert_task_acquire_data(crate::db::TaskAcquireData {
                        task_id,
                        item_kind: *item_kind,
                        quantity: *quantity,
                        source_kind,
                    });
            }
            task::TaskKind::Tame { target } => {
                let _ = self.db.insert_task_tame_data(crate::db::TaskTameData {
                    task_id,
                    target: *target,
                });
            }
            // AttackMove — extension data inserted by the command handler
            // (command_attack_move) since the destination VoxelCoord is not
            // carried in the TaskKind variant.
            task::TaskKind::AttackMove => {}
            task::TaskKind::Graze { grass_pos } => {
                let seq = self.db.task_voxel_refs.next_seq();
                let _ = self.db.insert_task_voxel_ref(crate::db::TaskVoxelRef {
                    seq,
                    task_id,
                    coord: *grass_pos,
                    role: crate::db::TaskVoxelRole::GrazeTarget,
                });
            }
            // GoTo, EatBread, Mope — no extra data.
            task::TaskKind::GoTo | task::TaskKind::EatBread | task::TaskKind::Mope => {}
        }
    }
}
