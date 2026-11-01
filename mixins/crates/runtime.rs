use std::{
    collections::HashMap,
    fs::{self, File},
    hash::{BuildHasher, Hash, Hasher, RandomState},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::game::{
    Game,
    crates_core::{
        CrateType, CratesData, KeyMode, MAX_CRATE_TYPES, MAX_ITEM_COUNT, MAX_REWARDS_PER_CRATE,
        MAX_WEIGHT, Placement, Reward, valid_item_id, valid_name, validate_data, weighted_index,
    },
    native_lifecycle::ServerTickPhase,
    native_server::{NativeCommand, NativeServerPlayer},
};
use horizon_client::{items::ItemStack, text::Text};

const KEY_ITEM_DEFAULT: &str = "minecraft:tripwire_hook";
const ANIMATION_TICKS: u16 = 60;

#[derive(Clone)]
enum MenuState {
    AdminHome {
        window: i32,
    },
    CrateDetail {
        window: i32,
        crate_type: String,
    },
    Opening {
        window: i32,
        crate_type: String,
        rewards: Vec<Reward>,
        prize: Reward,
        ticks: u16,
    },
}

static MENUS: OnceLock<Mutex<HashMap<u128, MenuState>>> = OnceLock::new();
static DRAW_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn menus() -> &'static Mutex<HashMap<u128, MenuState>> {
    MENUS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn data_path(game: &Game<'_>) -> Result<PathBuf, String> {
    let root = game
        .native_world_dir()
        .ok_or_else(|| "CRATES requires an open server world.".to_owned())?;
    Ok(root.join("horizon-crates").join("config.json"))
}

fn load_data(path: &Path) -> Result<CratesData, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let data: CratesData = serde_json::from_slice(&bytes)
                .map_err(|error| format!("Invalid CRATES data: {error}"))?;
            validate_data(&data)?;
            Ok(data)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(CratesData::default()),
        Err(error) => Err(format!("Could not read CRATES data: {error}")),
    }
}

fn save_data(path: &Path, data: &CratesData) -> Result<(), String> {
    validate_data(data)?;
    let parent = path
        .parent()
        .ok_or_else(|| "CRATES data path has no parent directory.".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create CRATES data directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(data)
        .map_err(|error| format!("Could not encode CRATES data: {error}"))?;
    let temporary = path.with_extension("json.tmp");
    let mut file = File::create(&temporary)
        .map_err(|error| format!("Could not write CRATES data: {error}"))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Could not flush CRATES data: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("Could not replace CRATES data: {error}"))
}

fn draw_roll(uuid: u128, total: u64) -> Result<u64, String> {
    if total == 0 {
        return Err("This crate has no weighted rewards.".into());
    }
    let rejection_threshold = total.wrapping_neg() % total;
    loop {
        let mut hasher = RandomState::new().build_hasher();
        uuid.hash(&mut hasher);
        DRAW_SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .hash(&mut hasher);
        let value = hasher.finish();
        if value >= rejection_threshold {
            return Ok(value % total);
        }
    }
}

fn uuid_key(uuid: u128) -> String {
    format!("{uuid:032x}")
}

fn item_stack(item: &str, count: u32) -> Option<ItemStack> {
    (count > 0).then(|| ItemStack {
        item: item.into(),
        count,
        damage: 0,
        extra: None,
    })
}

fn feedback(game: &mut Game<'_>, message: impl Into<String>) {
    game.native_feedback(Text::literal(message.into()));
}

fn tell(game: &Game<'_>, uuid: u128, message: impl Into<String>) {
    game.native_tell(uuid, Text::literal(message.into()));
}

fn action_bar(game: &mut Game<'_>, uuid: u128, message: impl Into<String>) {
    game.native_action_bar(uuid, Text::literal(message.into()));
}

pub(crate) fn register(commands: &mut Vec<NativeCommand>) {
    for usage in ["crates", "crates claim", "crates keys [type:word]"] {
        commands.push(NativeCommand::new(usage));
    }
    for usage in [
        "crates admin",
        "crates create <type:word>",
        "crates bind <type:word> <x:integer> <y:integer> <z:integer>",
        "crates unbind <x:integer> <y:integer> <z:integer>",
        "crates reward add <type:word> <item:item> <count:integer> <weight:integer>",
        "crates reward clear <type:word>",
        "crates reward remove <type:word> <index:integer>",
        "crates delete <type:word>",
        "crates key virtual <type:word>",
        "crates key item <type:word> <item:item>",
        "crates keys give <player:player> <type:word> <amount:integer>",
    ] {
        commands.push(NativeCommand::op(usage, 3));
    }
}

pub(crate) fn perform_command(
    game: &mut Game<'_>,
    player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    let words: Vec<&str> = command.split_whitespace().collect();
    if words.first() != Some(&"crates") {
        return;
    }
    *ran = true;
    match run_command(game, player, &words) {
        Ok(Some(message)) => feedback(game, message),
        Ok(None) => {}
        Err(error) => feedback(game, format!("CRATES: {error}")),
    }
}

fn run_command(
    game: &mut Game<'_>,
    player: &NativeServerPlayer,
    words: &[&str],
) -> Result<Option<String>, String> {
    match words.get(1).copied() {
        None | Some("help") => Ok(Some(help_text())),
        Some("admin") => {
            require_operator(player)?;
            open_admin_home(game, player.uuid)?;
            Ok(None)
        }
        Some("create") if words.len() == 3 => {
            require_operator(player)?;
            let name = words[2];
            if !valid_name(name) {
                return Err("Type names use lowercase letters, digits, `_` or `-`.".into());
            }
            let path = data_path(game)?;
            let mut data = load_data(&path)?;
            if data.crates.len() >= MAX_CRATE_TYPES {
                return Err(format!("Only {MAX_CRATE_TYPES} crate types are allowed."));
            }
            if data.crates.contains_key(name) {
                return Err(format!("Crate type `{name}` already exists."));
            }
            data.crates.insert(
                name.to_owned(),
                CrateType {
                    key: KeyMode::Virtual,
                    rewards: Vec::new(),
                },
            );
            save_data(&path, &data)?;
            Ok(Some(format!(
                "Created crate type `{name}` with virtual keys."
            )))
        }
        Some("delete") if words.len() == 3 => {
            require_operator(player)?;
            let path = data_path(game)?;
            let mut data = load_data(&path)?;
            if data.crates.remove(words[2]).is_none() {
                return Err(format!("Unknown crate type `{}`.", words[2]));
            }
            data.placements
                .retain(|placement| placement.crate_type != words[2]);
            for balances in data.virtual_keys.values_mut() {
                balances.remove(words[2]);
            }
            save_data(&path, &data)?;
            Ok(Some(format!(
                "Deleted crate type `{}` and its chest bindings.",
                words[2]
            )))
        }
        Some("bind") if words.len() == 6 => {
            require_operator(player)?;
            let name = words[2];
            let pos = parse_position(&words[3..6])?;
            let path = data_path(game)?;
            let mut data = load_data(&path)?;
            if !data.crates.contains_key(name) {
                return Err(format!("Unknown crate type `{name}`."));
            }
            data.placements
                .retain(|entry| entry.dimension != player.dimension || entry.pos != pos);
            data.placements.push(Placement {
                dimension: player.dimension.clone(),
                pos,
                crate_type: name.to_owned(),
            });
            save_data(&path, &data)?;
            Ok(Some(format!(
                "Bound `{name}` to {}, {}, {} in {}.",
                pos[0], pos[1], pos[2], player.dimension
            )))
        }
        Some("unbind") if words.len() == 5 => {
            require_operator(player)?;
            let pos = parse_position(&words[2..5])?;
            let path = data_path(game)?;
            let mut data = load_data(&path)?;
            let before = data.placements.len();
            data.placements
                .retain(|entry| entry.dimension != player.dimension || entry.pos != pos);
            if before == data.placements.len() {
                return Err("No CRATES binding exists at those coordinates.".into());
            }
            save_data(&path, &data)?;
            Ok(Some("Removed the CRATES binding.".into()))
        }
        Some("reward") if words.get(2) == Some(&"add") && words.len() == 7 => {
            require_operator(player)?;
            let name = words[3];
            let item = words[4];
            let count = parse_bounded(words[5], 1, MAX_ITEM_COUNT, "count")?;
            let weight = parse_bounded(words[6], 1, MAX_WEIGHT, "weight")?;
            ensure_known_item(game, item)?;
            let path = data_path(game)?;
            let mut data = load_data(&path)?;
            let crate_type = data
                .crates
                .get_mut(name)
                .ok_or_else(|| format!("Unknown crate type `{name}`."))?;
            if crate_type.rewards.len() >= MAX_REWARDS_PER_CRATE {
                return Err(format!(
                    "A crate can have at most {MAX_REWARDS_PER_CRATE} rewards."
                ));
            }
            crate_type.rewards.push(Reward {
                item: item.to_owned(),
                count,
                weight,
            });
            validate_data(&data)?;
            save_data(&path, &data)?;
            Ok(Some(format!(
                "Added {count}x {item} to `{name}` with weight {weight}."
            )))
        }
        Some("reward") if words.get(2) == Some(&"clear") && words.len() == 4 => {
            require_operator(player)?;
            let path = data_path(game)?;
            let mut data = load_data(&path)?;
            let crate_type = data
                .crates
                .get_mut(words[3])
                .ok_or_else(|| format!("Unknown crate type `{}`.", words[3]))?;
            crate_type.rewards.clear();
            save_data(&path, &data)?;
            Ok(Some(format!("Cleared rewards for `{}`.", words[3])))
        }
        Some("reward") if words.get(2) == Some(&"remove") && words.len() == 5 => {
            require_operator(player)?;
            let index = parse_bounded(words[4], 1, MAX_REWARDS_PER_CRATE as u32, "reward index")?;
            let path = data_path(game)?;
            let mut data = load_data(&path)?;
            let crate_type = data
                .crates
                .get_mut(words[3])
                .ok_or_else(|| format!("Unknown crate type `{}`.", words[3]))?;
            if index as usize > crate_type.rewards.len() {
                return Err(format!("Crate `{}` has no reward #{index}.", words[3]));
            }
            crate_type.rewards.remove(index as usize - 1);
            save_data(&path, &data)?;
            Ok(Some(format!(
                "Removed reward #{index} from `{}`.",
                words[3]
            )))
        }
        Some("key") if words.get(2) == Some(&"virtual") && words.len() == 4 => {
            require_operator(player)?;
            set_key_mode(game, words[3], KeyMode::Virtual)?;
            Ok(Some(format!("`{}` now uses virtual keys.", words[3])))
        }
        Some("key") if words.get(2) == Some(&"item") && words.len() == 5 => {
            require_operator(player)?;
            ensure_known_item(game, words[4])?;
            set_key_mode(game, words[3], KeyMode::Item(words[4].to_owned()))?;
            Ok(Some(format!(
                "`{}` now uses plain {} keys.",
                words[3], words[4]
            )))
        }
        Some("keys") if words.get(2) == Some(&"give") && words.len() == 6 => {
            require_operator(player)?;
            let amount = parse_bounded(words[5], 1, 1_000_000, "amount")?;
            let (target_uuid, target_name) = if player.name.eq_ignore_ascii_case(words[3]) {
                (player.uuid, player.name.clone())
            } else {
                let target = game
                    .native_server_players()
                    .into_iter()
                    .find(|candidate| candidate.name.eq_ignore_ascii_case(words[3]))
                    .ok_or_else(|| format!("Player `{}` is not online.", words[3]))?;
                (target.uuid, target.name)
            };
            let path = data_path(game)?;
            let mut data = load_data(&path)?;
            let uses_virtual_key = data
                .crates
                .get(words[4])
                .map(|crate_type| matches!(&crate_type.key, KeyMode::Virtual))
                .ok_or_else(|| format!("Unknown crate type `{}`.", words[4]))?;
            if !uses_virtual_key {
                return Err(format!(
                    "Crate `{}` uses item keys, not virtual keys.",
                    words[4]
                ));
            }
            let balances = data.virtual_keys.entry(uuid_key(target_uuid)).or_default();
            let balance = balances.entry(words[4].to_owned()).or_default();
            *balance = balance
                .checked_add(amount)
                .ok_or_else(|| "Virtual key balance limit reached.".to_owned())?;
            save_data(&path, &data)?;
            Ok(Some(format!(
                "Gave {amount} virtual `{}` key(s) to {}.",
                words[4], target_name
            )))
        }
        Some("keys") if words.len() <= 3 => {
            let path = data_path(game)?;
            let data = load_data(&path)?;
            let balances = data.virtual_keys.get(&uuid_key(player.uuid));
            let text = if let Some(name) = words.get(2) {
                format!(
                    "Virtual `{name}` keys: {}.",
                    balances
                        .and_then(|items| items.get(*name))
                        .copied()
                        .unwrap_or(0)
                )
            } else if let Some(balances) = balances {
                let summary = balances
                    .iter()
                    .map(|(name, amount)| format!("{name}={amount}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                if summary.is_empty() {
                    "You have no virtual keys.".into()
                } else {
                    format!("Your virtual keys: {summary}.")
                }
            } else {
                "You have no virtual keys.".into()
            };
            Ok(Some(text))
        }
        Some("claim") if words.len() == 2 => claim_rewards(game, player.uuid),
        _ => Ok(Some(help_text())),
    }
}

fn help_text() -> String {
    "CRATES: /crates keys [type], /crates claim; operators: /crates admin, create/delete, bind/unbind, reward add/remove/clear, key, keys give.".into()
}

fn require_operator(player: &NativeServerPlayer) -> Result<(), String> {
    if player.permission >= 3 {
        Ok(())
    } else {
        Err("Operator permission level 3 is required.".into())
    }
}

fn parse_position(words: &[&str]) -> Result<[i32; 3], String> {
    if words.len() != 3 {
        return Err("Expected three block coordinates.".into());
    }
    let mut pos = [0; 3];
    for (index, word) in words.iter().enumerate() {
        pos[index] = word
            .parse()
            .map_err(|_| format!("Invalid block coordinate `{word}`."))?;
    }
    Ok(pos)
}

fn parse_bounded<T>(value: &str, min: T, max: T, label: &str) -> Result<T, String>
where
    T: std::str::FromStr + PartialOrd + Copy + std::fmt::Display,
{
    let parsed = value
        .parse::<T>()
        .map_err(|_| format!("Invalid {label} `{value}`."))?;
    if parsed < min || parsed > max {
        return Err(format!("{label} must be between {min} and {max}."));
    }
    Ok(parsed)
}

fn ensure_known_item(game: &mut Game<'_>, item: &str) -> Result<(), String> {
    if !valid_item_id(item) {
        return Err(format!("`{item}` is not a plain namespaced item ID."));
    }
    let known = game
        .native_recipes()
        .is_some_and(|recipes| recipes.items.iter().any(|entry| entry.id == item));
    if known {
        Ok(())
    } else {
        Err(format!("Unknown item ID `{item}` in this world."))
    }
}

fn set_key_mode(game: &mut Game<'_>, name: &str, mode: KeyMode) -> Result<(), String> {
    let path = data_path(game)?;
    let mut data = load_data(&path)?;
    let crate_type = data
        .crates
        .get_mut(name)
        .ok_or_else(|| format!("Unknown crate type `{name}`."))?;
    crate_type.key = mode;
    save_data(&path, &data)
}

fn open_admin_home(game: &mut Game<'_>, uuid: u128) -> Result<(), String> {
    let data = load_data(&data_path(game)?)?;
    let mut items = vec![None; 27];
    items[4] = item_stack("minecraft:chest", 1);
    items[22] = item_stack("minecraft:emerald", 1);
    for index in 0..data.crates.len().min(8) {
        items[9 + index] = item_stack("minecraft:barrel", (index + 1) as u32);
    }
    let window = game
        .native_open_menu(uuid, 3, Text::literal("CRATES Admin"), items)
        .ok_or_else(|| "Could not open the CRATES dashboard.".to_owned())?;
    menus()
        .lock()
        .map_err(|_| "CRATES menu state is unavailable.".to_owned())?
        .insert(uuid, MenuState::AdminHome { window });
    let names = data
        .crates
        .keys()
        .take(8)
        .enumerate()
        .map(|(index, name)| format!("{}={name}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    tell(
        game,
        uuid,
        format!(
            "CRATES dashboard opened. Crate buttons: {names}. Click the emerald button to create one."
        ),
    );
    Ok(())
}

fn open_crate_detail(game: &mut Game<'_>, uuid: u128, crate_name: &str) -> Result<(), String> {
    let data = load_data(&data_path(game)?)?;
    let crate_type = data
        .crates
        .get(crate_name)
        .ok_or_else(|| format!("Unknown crate type `{crate_name}`."))?;
    let mut items = vec![None; 27];
    items[0] = item_stack("minecraft:arrow", 1);
    items[4] = item_stack("minecraft:chest", 1);
    items[11] = item_stack(
        match &crate_type.key {
            KeyMode::Virtual => "minecraft:emerald",
            KeyMode::Item(_) => "minecraft:tripwire_hook",
        },
        1,
    );
    items[13] = item_stack("minecraft:book", 1);
    for (index, reward) in crate_type.rewards.iter().take(8).enumerate() {
        items[18 + index] = item_stack(&reward.item, reward.count);
    }
    items[26] = item_stack("minecraft:paper", 1);
    let window = game
        .native_open_menu(
            uuid,
            3,
            Text::literal(format!("CRATES: {crate_name}")),
            items,
        )
        .ok_or_else(|| "Could not open the crate details.".to_owned())?;
    menus()
        .lock()
        .map_err(|_| "CRATES menu state is unavailable.".to_owned())?
        .insert(
            uuid,
            MenuState::CrateDetail {
                window,
                crate_type: crate_name.to_owned(),
            },
        );
    let reward_summary = if crate_type.rewards.is_empty() {
        "no rewards configured".to_owned()
    } else {
        crate_type
            .rewards
            .iter()
            .map(|reward| {
                format!(
                    "{}x {} (weight {})",
                    reward.count, reward.item, reward.weight
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let key_summary = match &crate_type.key {
        KeyMode::Virtual => "virtual key".to_owned(),
        KeyMode::Item(item) => format!("plain {item} item key"),
    };
    tell(
        game,
        uuid,
        format!(
            "`{crate_name}` uses a {key_summary}. Rewards: {reward_summary}. Click the key icon to switch key mode."
        ),
    );
    Ok(())
}

fn create_crate_from_dashboard(game: &mut Game<'_>, uuid: u128) -> Result<String, String> {
    let path = data_path(game)?;
    let mut data = load_data(&path)?;
    if data.crates.len() >= MAX_CRATE_TYPES {
        return Err(format!("Only {MAX_CRATE_TYPES} crate types are allowed."));
    }
    let mut suffix = 1;
    let name = loop {
        let candidate = format!("crate_{suffix}");
        if !data.crates.contains_key(&candidate) {
            break candidate;
        }
        suffix += 1;
    };
    data.crates.insert(
        name.clone(),
        CrateType {
            key: KeyMode::Virtual,
            rewards: Vec::new(),
        },
    );
    save_data(&path, &data)?;
    open_crate_detail(game, uuid, &name)?;
    Ok(name)
}

pub(crate) fn menu_clicked(game: &mut Game<'_>, uuid: u128, window: i32, slot: i32) {
    let state = match menus().lock() {
        Ok(menus) => menus.get(&uuid).cloned(),
        Err(_) => {
            tell(game, uuid, "CRATES menu state is unavailable.");
            return;
        }
    };
    let Some(state) = state else {
        return;
    };
    match state {
        MenuState::AdminHome { window: expected } if expected == window => {
            if slot == 22 {
                match create_crate_from_dashboard(game, uuid) {
                    Ok(name) => tell(
                        game,
                        uuid,
                        format!(
                            "Created `{name}`. Add rewards and bind a chest with /crates commands."
                        ),
                    ),
                    Err(error) => tell(game, uuid, format!("CRATES: {error}")),
                }
            } else if (9..17).contains(&slot) {
                let result = data_path(game).and_then(|path| load_data(&path));
                match result {
                    Ok(data) => {
                        if let Some(name) = data.crates.keys().nth((slot - 9) as usize)
                            && let Err(error) = open_crate_detail(game, uuid, name)
                        {
                            tell(game, uuid, format!("CRATES: {error}"));
                        }
                    }
                    Err(error) => tell(game, uuid, format!("CRATES: {error}")),
                }
            }
        }
        MenuState::CrateDetail {
            window: expected,
            crate_type,
        } if expected == window => match slot {
            0 => {
                if let Err(error) = open_admin_home(game, uuid) {
                    tell(game, uuid, format!("CRATES: {error}"));
                }
            }
            11 => {
                let path = match data_path(game) {
                    Ok(path) => path,
                    Err(error) => {
                        tell(game, uuid, format!("CRATES: {error}"));
                        return;
                    }
                };
                let result = load_data(&path).and_then(|mut data| {
                    let entry = data
                        .crates
                        .get_mut(&crate_type)
                        .ok_or_else(|| format!("Unknown crate type `{crate_type}`."))?;
                    entry.key = match &entry.key {
                        KeyMode::Virtual => KeyMode::Item(KEY_ITEM_DEFAULT.into()),
                        KeyMode::Item(_) => KeyMode::Virtual,
                    };
                    save_data(&path, &data)
                });
                match result {
                    Ok(()) => {
                        if let Err(error) = open_crate_detail(game, uuid, &crate_type) {
                            tell(game, uuid, format!("CRATES: {error}"));
                        }
                    }
                    Err(error) => tell(game, uuid, format!("CRATES: {error}")),
                }
            }
            13 | 26 => tell(
                game,
                uuid,
                format!(
                    "Edit `{crate_type}` with /crates reward add, /crates reward clear, and /crates bind. Item-key changes in the dashboard use {KEY_ITEM_DEFAULT}."
                ),
            ),
            _ => {}
        },
        _ => {}
    }
}

pub(crate) fn menu_closed(uuid: u128, window: i32) {
    if let Ok(mut menus) = menus().lock()
        && menus
            .get(&uuid)
            .is_some_and(|state| menu_window(state) == window)
    {
        menus.remove(&uuid);
    }
}

fn menu_window(state: &MenuState) -> i32 {
    match state {
        MenuState::AdminHome { window }
        | MenuState::CrateDetail { window, .. }
        | MenuState::Opening { window, .. } => *window,
    }
}

pub(crate) fn right_click_block(game: &mut Game<'_>, pos: [i32; 3], canceled: &mut bool) {
    let Some(player) = game.native_acting_player() else {
        return;
    };
    let path = match data_path(game) {
        Ok(path) => path,
        Err(error) => {
            action_bar(game, player.uuid, format!("CRATES: {error}"));
            return;
        }
    };
    let mut data = match load_data(&path) {
        Ok(data) => data,
        Err(error) => {
            action_bar(game, player.uuid, format!("CRATES: {error}"));
            return;
        }
    };
    let Some(crate_name) = data
        .placements
        .iter()
        .find(|entry| entry.dimension == player.dimension && entry.pos == pos)
        .map(|entry| entry.crate_type.clone())
    else {
        return;
    };
    *canceled = true;
    let opening_in_progress = match menus().lock() {
        Ok(menus) => matches!(menus.get(&player.uuid), Some(MenuState::Opening { .. })),
        Err(_) => {
            action_bar(game, player.uuid, "CRATES menu state is unavailable.");
            return;
        }
    };
    if opening_in_progress {
        action_bar(game, player.uuid, "A CRATES draw is already in progress.");
        return;
    }
    let Some(crate_type) = data.crates.get(&crate_name).cloned() else {
        action_bar(
            game,
            player.uuid,
            format!("CRATES type `{crate_name}` is missing."),
        );
        return;
    };
    if crate_type.rewards.is_empty() {
        action_bar(
            game,
            player.uuid,
            format!("Crate `{crate_name}` has no rewards yet."),
        );
        return;
    }
    let total = match crate_type
        .rewards
        .iter()
        .try_fold(0_u64, |sum, reward| sum.checked_add(reward.weight))
    {
        Some(total) if total > 0 => total,
        _ => {
            action_bar(game, player.uuid, "This crate has invalid reward weights.");
            return;
        }
    };
    let roll = match draw_roll(player.uuid, total) {
        Ok(roll) => roll,
        Err(error) => {
            action_bar(game, player.uuid, format!("CRATES: {error}"));
            return;
        }
    };
    let Some(index) = weighted_index(&crate_type.rewards, roll) else {
        action_bar(game, player.uuid, "CRATES could not select a reward.");
        return;
    };
    let prize = crate_type.rewards[index].clone();
    let key_id = uuid_key(player.uuid);
    match &crate_type.key {
        KeyMode::Virtual => {
            let balance = data
                .virtual_keys
                .get_mut(&key_id)
                .and_then(|keys| keys.get_mut(&crate_name));
            let Some(balance) = balance else {
                action_bar(
                    game,
                    player.uuid,
                    format!("You do not have a `{crate_name}` virtual key."),
                );
                return;
            };
            if *balance == 0 {
                action_bar(
                    game,
                    player.uuid,
                    format!("You do not have a `{crate_name}` virtual key."),
                );
                return;
            }
            *balance -= 1;
        }
        KeyMode::Item(item) => {
            if game.native_count_items(item) == 0 {
                action_bar(game, player.uuid, format!("You need a plain {item} key."));
                return;
            }
            if game.native_take_items(item, 1) != 1 {
                action_bar(
                    game,
                    player.uuid,
                    "CRATES could not consume the item key; no draw started.",
                );
                return;
            }
        }
    }
    data.pending_rewards
        .entry(key_id)
        .or_default()
        .push(prize.clone());
    if let Err(error) = save_data(&path, &data) {
        if let KeyMode::Item(item) = &crate_type.key
            && game.native_give_items(item, 1) != 1
        {
            action_bar(
                game,
                player.uuid,
                format!("CRATES save failed and the consumed key could not be restored: {error}"),
            );
            return;
        }
        action_bar(game, player.uuid, format!("CRATES: {error}"));
        return;
    }
    let window = match game.native_open_menu(
        player.uuid,
        3,
        Text::literal(format!("Opening: {crate_name}")),
        animation_items(&crate_type.rewards, None),
    ) {
        Some(window) => window,
        None => {
            action_bar(
                game,
                player.uuid,
                "The draw was saved, but its animation could not open. Use /crates claim.",
            );
            return;
        }
    };
    match menus().lock() {
        Ok(mut menus) => {
            menus.insert(
                player.uuid,
                MenuState::Opening {
                    window,
                    crate_type: crate_name,
                    rewards: crate_type.rewards,
                    prize,
                    ticks: 0,
                },
            );
        }
        Err(_) => action_bar(
            game,
            player.uuid,
            "The reward is saved. Use /crates claim after the menu closes.",
        ),
    }
}

fn animation_items(rewards: &[Reward], reveal: Option<&Reward>) -> Vec<Option<ItemStack>> {
    let mut items = vec![None; 27];
    items[4] = item_stack("minecraft:purple_stained_glass_pane", 1);
    items[12] = reveal
        .and_then(|reward| item_stack(&reward.item, reward.count))
        .or_else(|| {
            rewards
                .first()
                .and_then(|reward| item_stack(&reward.item, reward.count))
        });
    items[14] = item_stack("minecraft:purple_stained_glass_pane", 1);
    items
}

pub(crate) fn server_tick(game: &mut Game<'_>, phase: ServerTickPhase) {
    if !matches!(phase, ServerTickPhase::ServerTickEnd { .. }) {
        return;
    }
    let updates = match menus().lock() {
        Ok(mut menus) => {
            let mut updates = Vec::new();
            for (&uuid, state) in menus.iter_mut() {
                if let MenuState::Opening {
                    crate_type,
                    rewards,
                    prize,
                    ticks,
                    ..
                } = state
                {
                    *ticks = ticks.saturating_add(1);
                    if *ticks % 3 == 0 || *ticks == ANIMATION_TICKS {
                        let reveal = (*ticks >= ANIMATION_TICKS).then_some(prize);
                        let display = if let Some(reward) = reveal {
                            animation_items(rewards, Some(reward))
                        } else {
                            let index = usize::from(*ticks / 3) % rewards.len();
                            let mut frame = animation_items(rewards, None);
                            frame[12] = item_stack(&rewards[index].item, rewards[index].count);
                            frame
                        };
                        updates.push((
                            uuid,
                            display,
                            crate_type.clone(),
                            *ticks == ANIMATION_TICKS,
                        ));
                    }
                }
            }
            updates
        }
        Err(_) => return,
    };
    for (uuid, items, crate_type, revealed) in updates {
        game.native_set_menu_items(uuid, items);
        if revealed {
            tell(
                game,
                uuid,
                format!(
                    "CRATES: draw complete for `{crate_type}`. Claim your saved reward with /crates claim."
                ),
            );
        }
    }
    let finished = match menus().lock() {
        Ok(mut menus) => {
            let finished = menus
                .iter()
                .filter_map(|(&uuid, state)| match state {
                    MenuState::Opening { ticks, .. } if *ticks >= ANIMATION_TICKS + 20 => {
                        Some((uuid, menu_window(state)))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            for (uuid, _) in &finished {
                menus.remove(uuid);
            }
            finished
        }
        Err(_) => return,
    };
    for (uuid, _) in finished {
        game.native_close_menu(uuid);
    }
}

fn claim_rewards(game: &mut Game<'_>, uuid: u128) -> Result<Option<String>, String> {
    if menus()
        .lock()
        .map_err(|_| "CRATES menu state is unavailable.".to_owned())?
        .get(&uuid)
        .is_some_and(|state| matches!(state, MenuState::Opening { .. }))
    {
        return Err("Wait for the crate animation to finish before claiming.".into());
    }
    let path = data_path(game)?;
    let mut data = load_data(&path)?;
    let key = uuid_key(uuid);
    let rewards = data.pending_rewards.remove(&key).unwrap_or_default();
    if rewards.is_empty() {
        return Ok(Some("You have no pending CRATES rewards.".into()));
    }
    let mut remaining = Vec::new();
    let mut delivered = 0_u32;
    for mut reward in rewards {
        let given = game.native_give_items(&reward.item, reward.count);
        reward.count = reward.count.saturating_sub(given);
        delivered = delivered.saturating_add(given);
        if reward.count > 0 {
            remaining.push(reward);
        }
    }
    if !remaining.is_empty() {
        data.pending_rewards.insert(key, remaining);
    }
    save_data(&path, &data)?;
    if delivered == 0 {
        Ok(Some(
            "Your inventory is full; rewards remain pending.".into(),
        ))
    } else {
        Ok(Some(format!(
            "Claimed {delivered} reward item(s). Any remainder stays pending."
        )))
    }
}
