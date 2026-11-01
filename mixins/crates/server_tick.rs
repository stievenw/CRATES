fn crates_server_tick(
    game: &mut crate::game::Game<'_>,
    phase: crate::game::native_lifecycle::ServerTickPhase,
) {
    crate::game::crates::server_tick(game, phase);
}
