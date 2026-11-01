fn crates_menu_clicked(
    game: &mut crate::game::Game<'_>,
    uuid: u128,
    window: i32,
    slot: i32,
) {
    crate::game::crates::menu_clicked(game, uuid, window, slot);
}
