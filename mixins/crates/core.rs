use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_CRATE_TYPES: usize = 32;
pub const MAX_REWARDS_PER_CRATE: usize = 27;
pub const MAX_ITEM_COUNT: u32 = 64;
pub const MAX_WEIGHT: u64 = 1_000_000;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CratesData {
    pub crates: BTreeMap<String, CrateType>,
    pub placements: Vec<Placement>,
    pub virtual_keys: BTreeMap<String, BTreeMap<String, u64>>,
    pub pending_rewards: BTreeMap<String, Vec<Reward>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CrateType {
    pub key: KeyMode,
    pub rewards: Vec<Reward>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum KeyMode {
    Virtual,
    Item(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Reward {
    pub item: String,
    pub count: u32,
    pub weight: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Placement {
    pub dimension: String,
    pub pos: [i32; 3],
    pub crate_type: String,
}

pub fn validate_data(data: &CratesData) -> Result<(), String> {
    if data.crates.len() > MAX_CRATE_TYPES {
        return Err("CRATES data contains too many crate types.".into());
    }
    for (name, crate_type) in &data.crates {
        if !valid_name(name) {
            return Err(format!("Invalid crate type name `{name}` in CRATES data."));
        }
        if crate_type.rewards.len() > MAX_REWARDS_PER_CRATE {
            return Err(format!("Crate `{name}` contains too many rewards."));
        }
        let total = crate_type.rewards.iter().try_fold(0_u64, |sum, reward| {
            if !valid_item_id(&reward.item)
                || reward.count == 0
                || reward.count > MAX_ITEM_COUNT
                || reward.weight == 0
                || reward.weight > MAX_WEIGHT
            {
                return None;
            }
            sum.checked_add(reward.weight)
        });
        if total.is_none() {
            return Err(format!(
                "Crate `{name}` has an invalid reward or total weight."
            ));
        }
        if let KeyMode::Item(item) = &crate_type.key
            && !valid_item_id(item)
        {
            return Err(format!("Crate `{name}` has an invalid item key."));
        }
    }
    for placement in &data.placements {
        if !data.crates.contains_key(&placement.crate_type) {
            return Err(format!(
                "CRATES placement points to missing type `{}`.",
                placement.crate_type
            ));
        }
    }
    Ok(())
}

pub fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
}

pub fn valid_item_id(value: &str) -> bool {
    let Some((namespace, path)) = value.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !path.is_empty()
        && namespace.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
        })
        && path.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_./-".contains(&byte)
        })
}

pub fn weighted_index(rewards: &[Reward], roll: u64) -> Option<usize> {
    let total = rewards
        .iter()
        .try_fold(0_u64, |sum, reward| sum.checked_add(reward.weight))?;
    if total == 0 || roll >= total || rewards.iter().any(|reward| reward.weight == 0) {
        return None;
    }
    let mut remaining = roll;
    rewards.iter().position(|reward| {
        if remaining < reward.weight {
            true
        } else {
            remaining -= reward.weight;
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reward(weight: u64) -> Reward {
        Reward {
            item: "minecraft:diamond".into(),
            count: 1,
            weight,
        }
    }

    #[test]
    fn weighted_index_covers_each_weight_interval() {
        let rewards = [reward(2), reward(3), reward(5)];
        assert_eq!(weighted_index(&rewards, 0), Some(0));
        assert_eq!(weighted_index(&rewards, 1), Some(0));
        assert_eq!(weighted_index(&rewards, 2), Some(1));
        assert_eq!(weighted_index(&rewards, 4), Some(1));
        assert_eq!(weighted_index(&rewards, 5), Some(2));
        assert_eq!(weighted_index(&rewards, 9), Some(2));
    }

    #[test]
    fn weighted_index_rejects_bad_rolls_and_weights() {
        assert_eq!(weighted_index(&[reward(1)], 1), None);
        assert_eq!(weighted_index(&[reward(0)], 0), None);
        assert_eq!(weighted_index(&[], 0), None);
    }

    #[test]
    fn validates_names_and_plain_item_ids() {
        assert!(valid_name("rare_crate-2"));
        assert!(!valid_name("Rare Crate"));
        assert!(valid_item_id("minecraft:tripwire_hook"));
        assert!(!valid_item_id("Tripwire Hook"));
        assert!(!valid_item_id("minecraft:diamond[custom_name=x]"));
    }

    #[test]
    fn data_round_trips_and_rejects_dangling_placements() {
        let data = CratesData {
            crates: BTreeMap::from([(
                "basic".into(),
                CrateType {
                    key: KeyMode::Virtual,
                    rewards: vec![reward(1)],
                },
            )]),
            placements: vec![Placement {
                dimension: "minecraft:overworld".into(),
                pos: [1, 64, -2],
                crate_type: "basic".into(),
            }],
            ..CratesData::default()
        };
        let encoded = serde_json::to_vec(&data).expect("serializes");
        let decoded: CratesData = serde_json::from_slice(&encoded).expect("deserializes");
        validate_data(&decoded).expect("valid data");
        let mut broken = decoded;
        broken.placements[0].crate_type = "missing".into();
        assert!(validate_data(&broken).is_err());
    }
}
