fn register_crates(commands: &mut Vec<crate::game::native_server::NativeCommand>) {
    crate::game::crates::register(commands);
}
