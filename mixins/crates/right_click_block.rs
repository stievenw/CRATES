fn crates_right_click_block(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    canceled: &mut bool,
) {
    crate::game::crates::right_click_block(game, pos, canceled);
}
