extern crate self as horizon_client;

pub mod items {
    #[derive(Clone)]
    pub struct ItemStack {
        pub item: String,
        pub count: u32,
        pub damage: u32,
        pub extra: Option<()>,
    }
}

pub mod text {
    pub struct Text(pub String);

    impl Text {
        pub fn literal(value: impl Into<String>) -> Self {
            Self(value.into())
        }
    }
}

mod game {
    use std::{marker::PhantomData, path::PathBuf, sync::Arc};

    pub use horizon_crates as crates_core;

    pub mod native_lifecycle {
        #[derive(Clone, Copy)]
        #[allow(dead_code)]
        pub enum ServerTickPhase {
            ServerTickStart { world_running: bool },
            ServerTickEnd { world_running: bool },
        }
    }

    pub mod native_server {
        #[derive(Clone)]
        pub struct NativeServerPlayer {
            pub uuid: u128,
            pub name: String,
            pub permission: u8,
            pub dimension: String,
        }

        pub struct NativeCommand {
            pub usage: String,
            pub level: u8,
        }

        impl NativeCommand {
            pub fn new(usage: &str) -> Self {
                Self {
                    usage: usage.into(),
                    level: 0,
                }
            }

            pub fn op(usage: &str, level: u8) -> Self {
                Self {
                    usage: usage.into(),
                    level,
                }
            }
        }
    }

    pub struct NativeItem {
        pub id: String,
    }

    pub struct NativeRecipes {
        pub items: Vec<NativeItem>,
    }

    pub struct Game<'a> {
        _lifetime: PhantomData<&'a ()>,
    }

    impl Game<'_> {
        pub fn native_world_dir(&self) -> Option<PathBuf> {
            None
        }

        pub fn native_feedback(&mut self, _line: crate::text::Text) {}

        pub fn native_tell(&self, _uuid: u128, _line: crate::text::Text) -> bool {
            true
        }

        pub fn native_action_bar(&mut self, _uuid: u128, _line: crate::text::Text) -> bool {
            true
        }

        pub fn native_server_players(&self) -> Vec<native_server::NativeServerPlayer> {
            Vec::new()
        }

        pub fn native_recipes(&mut self) -> Option<Arc<NativeRecipes>> {
            Some(Arc::new(NativeRecipes { items: Vec::new() }))
        }

        pub fn native_open_menu(
            &mut self,
            _uuid: u128,
            _rows: u8,
            _title: crate::text::Text,
            _items: Vec<Option<crate::items::ItemStack>>,
        ) -> Option<i32> {
            Some(1)
        }

        pub fn native_set_menu_items(
            &mut self,
            _uuid: u128,
            _items: Vec<Option<crate::items::ItemStack>>,
        ) {
        }

        pub fn native_close_menu(&mut self, _uuid: u128) {}

        pub fn native_count_items(&mut self, _item: &str) -> u32 {
            0
        }

        pub fn native_take_items(&mut self, _item: &str, _count: u32) -> u32 {
            0
        }

        pub fn native_give_items(&mut self, _item: &str, _count: u32) -> u32 {
            0
        }

        pub fn native_acting_player(&self) -> Option<native_server::NativeServerPlayer> {
            None
        }
    }
}

#[allow(dead_code)]
#[path = "../mixins/crates/runtime.rs"]
mod runtime;

#[test]
fn runtime_registers_player_and_operator_commands() {
    let mut commands = Vec::new();
    runtime::register(&mut commands);

    assert!(
        commands
            .iter()
            .any(|command| command.usage == "crates claim")
    );
    assert!(
        commands
            .iter()
            .any(|command| command.usage == "crates admin" && command.level == 3)
    );
}
