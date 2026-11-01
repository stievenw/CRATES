# CRATES

CRATES is a server-side native Horizon mod for operator-managed, weighted
reward crates. It uses Horizon's server hooks and works with unmodified
Horizon clients.

Horizon uses `.hmod` for content-only mods, which cannot run CRATES' server
logic. This mod is packaged as `.hnmod`, Horizon's native-code mod format.
Native mods run with the user's account permissions and are experimental.

## Features

- Operators configure crate types, reward item IDs, counts, and relative
  weights with `/crates` commands.
- Bind a crate type to a chest by dimension and block coordinates.
- Choose a virtual key balance or a plain vanilla item key per crate type.
- Players open bound crates by right-clicking their chest. A vanilla chest
  menu animates the reward draw; the selected reward is saved to the world's
  pending-reward mailbox and claimed with `/crates claim`.
- `/crates admin` opens an operator dashboard with item-button navigation.
  The dashboard can create crate types, inspect their settings, and switch
  between virtual and item keys. Reward editing and block binding remain
  available through commands. It uses Horizon's vanilla chest menu rather
  than custom textures; item names and reward weights are also shown in chat.
- Configuration and pending rewards live in `world/horizon-crates/`, so
  world backups include them.

CRATES does not sell keys or random rewards for real money or for currency
that can be bought with real money. Keys are issued in-game by an operator.

## Operator commands

The command tree only exposes management actions to permission level 3 or
higher.

```text
/crates admin
/crates create <type>
/crates delete <type>
/crates bind <type> <x> <y> <z>
/crates reward add <type> <item-id> <count> <weight>
/crates reward remove <type> <index>
/crates reward clear <type>
/crates key virtual <type>
/crates key item <type> <item-id>
/crates keys give <player> <type> <amount>
/crates keys [type]
/crates unbind <x> <y> <z>
/crates claim
```

Coordinates in `/crates bind` are absolute block coordinates in the
operator's current dimension. Reward weights are positive integers; the
chance for a reward is its weight divided by the total weight for that crate.
For example, weights 1 and 9 mean 10% and 90%.

Item keys and rewards use plain item IDs such as `minecraft:tripwire_hook`
and `minecraft:diamond`. Item components and named items are not accepted.
When an inventory cannot fit a reward, it stays in the player's mailbox
until there is room and `/crates claim` is run.

## Build and validate

This source bundle is pinned to Horizon commit
`7066b93c9668f221369cbac44310b97fd4b5ffaa` and hook API 22:

```sh
cargo fmt --check
cargo test
python tools/package_hnmod.py
cargo run --manifest-path "G:\path\to\horizon-main\crates\cargo-horizon-mod\Cargo.toml" -- check target/horizon-mod/crates.hnmod
horizon-launcher native-build "G:\path\to\CRATE" --trust-native-code
```

The package command creates `target/horizon-mod/crates.hnmod`. The `check`
command verifies its bundle metadata and contents. The native build compiles
the hooks against the pinned Horizon source and selects the resulting game
version for the launcher's Native mods instance; it requires Horizon's
launcher, a prepared game version, Rust, and explicit trust in the native
code. Native server hooks run in single-player and LAN-hosted worlds; Horizon
does not currently build native mods for dedicated servers.

The native mod executes with the player's account permissions. Review the
source and trust it only if appropriate before building or installing it.
