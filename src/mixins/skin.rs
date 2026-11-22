// `hand_geometry` starts: which skin the own arm wears, for `arm.rs`. The
// game draws the arm itself only for an empty hand; this lets Living Hands
// draw it under held items too.
fn living_hands_skin(game: &Game<'_>) {
    let own = game.player_model.as_ref().map(|m| mod_living_hands::arm::Own {
        skin: m.skin.as_deref().map(str::to_owned),
        slim: m.slim,
        main_left: game.settings.main_hand_left,
        invisible: game.own_invisible(),
    });
    mod_living_hands::arm::set_own(own);
}
