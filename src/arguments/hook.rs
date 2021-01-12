// SPDX-License-Identifier: GPL-2.0-or-later

//! This module contains utilities that are useful for parsing hooks.
//! This module can be seen as an extension of crate::argument_parser.

use crate::error::ArgumentError;
use crate::utils;
use crate::state::{State, ToggleIndex};
use crate::hook::Effect;
use crate::key::{Key, KeyParser};
use crate::event::Namespace;
use crate::arguments::lib::ComplexArgGroup;
use std::collections::HashMap;

/// Represents a --hook argument.
pub(super) struct HookArg {
    pub exec_shell: Vec<String>,
    pub hold_keys: Vec<Key>,
    pub toggle_action: HookToggleAction,
}

impl HookArg {
	pub fn parse(args: Vec<String>) -> Result<HookArg, ArgumentError> {
        let arg_group = ComplexArgGroup::parse(args,
            &["toggle"],
            &["exec-shell", "toggle"],
            false,
            true,
        )?;

        let toggle_action = HookToggleAction::parse(arg_group.has_flag("toggle"), arg_group.get_clauses("toggle"))?;
        let hold_keys = KeyParser {
            allow_transitions: false,
            allow_ranges: true,
            default_value: "1~",
            namespace: Namespace::User,
        }.parse_all(&arg_group.keys)?;

        if arg_group.keys.is_empty() {
            Err(ArgumentError::new("A --hook argument requires at least one key."))
        } else {
            Ok(HookArg {
                exec_shell: arg_group.get_clauses("exec-shell"),
                hold_keys, toggle_action,
            })
        }
    }
}

/// Represents how a single toggle clause on a hook should modify some toggle.
#[derive(Clone, Copy)]
enum HookToggleShift {
    /// Move the active index to the next one, wrapping around.
    Next,
    /// Set the active index to a specific index.
    ToIndex(usize),
}

/// Represents the aggregate effect of all toggle= clauses on a single --hook.
/// This is used to track arguments, this is not the implementation of such an effect.
pub struct HookToggleAction {
    /// The action based on a toggle flag or a toggle= without id.
    global_action: Option<HookToggleShift>,
    /// The set of specific toggle=id:index specified.
    by_id_actions: HashMap<String, HookToggleShift>,
}

impl HookToggleAction {
    fn new() -> HookToggleAction {
        HookToggleAction {
            global_action: None,
            by_id_actions: HashMap::new(),
        }
    }

    pub fn parse(has_toggle_flag: bool, toggle_clauses: Vec<String>) -> Result<HookToggleAction, ArgumentError> {
        let mut toggle_action = HookToggleAction::new();
        if has_toggle_flag {
            toggle_action.global_action = Some(HookToggleShift::Next);
        }
        for clause in toggle_clauses {
            let (id, index_str_opt) = utils::split_once(&clause, ":");
            let index: HookToggleShift = match index_str_opt {
                None => HookToggleShift::Next,
                Some(index_str) => HookToggleShift::ToIndex(
                    match index_str.parse::<usize>() {
                        Ok(value) => match value {
                            0 => return Err(ArgumentError::new("Cannot use toggle index 0: toggle indices start at 1.")),
                            _ => value - 1,
                        },
                        Err(error) => return Err(ArgumentError::new(format!("Cannot interpret {} as an integer: {}.", index_str, error))),
                    }
                ),
            };
            match id {
                "" => match toggle_action.global_action {
                    None => { toggle_action.global_action = Some(index); },
                    Some(_) => return Err(ArgumentError::new("A --hook cannot have multiple unspecified toggle clauses.")),
                },
                _ => {
                    match toggle_action.by_id_actions.get(id) {
                        None => { toggle_action.by_id_actions.insert(id.to_owned(), index); },
                        Some(_) => return Err(ArgumentError::new(format!("A toggle={} clause has been specified multiple times.", {id}))),
                    }
                }
            }
        }

        Ok(toggle_action)
    }

    /// Returns a list of all toggle effects that a hook needs to implement this HookToggleAction.
    /// Requires a map mapping toggle's id to their index. This map must contain all toggles which
    /// have an ID, but does not need to contain toggles that don't have any ID.
    pub fn implement(&self, toggle_index_by_id: &HashMap<String, ToggleIndex>) -> Result<Vec<Effect>, ArgumentError> {
        let mut effects: Vec<Effect> = Vec::new();
        let mut specified_indices: Vec<ToggleIndex> = Vec::new();
        for (toggle_id, &shift) in &self.by_id_actions {
            let toggle_index = *toggle_index_by_id.get(toggle_id).ok_or_else(|| {
                ArgumentError::new(format!("No toggle with the id \"{}\" exists.", toggle_id))
            })?;
            specified_indices.push(toggle_index);
            effects.push(Box::new(move |state: &mut State| {
                match shift {
                    HookToggleShift::Next => state[toggle_index].advance(),
                    HookToggleShift::ToIndex(value) => state[toggle_index].set_value_wrapped(value),
                }
            }));
        }
        if let Some(shift) = self.global_action {
            effects.push(Box::new(move |state: &mut State| {
                let toggles_affected = state.get_toggles_except(&specified_indices);
                for toggle in toggles_affected {
                    match shift {
                        HookToggleShift::Next => toggle.advance(),
                        HookToggleShift::ToIndex(value) => toggle.set_value_wrapped(value),
                    }
                }
            }));
        }

        Ok(effects)
    }
}
