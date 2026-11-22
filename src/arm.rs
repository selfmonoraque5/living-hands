//! Your own arm, with your skin, under each held item, as the game draws it
//! for an empty hand (`renderPlayerArm`). The mod API's `render_hand` can draw
//! a model part, but doesn't say which texture is the player's skin: the game
//! patch in `src/mixins/skin.rs` hands that over each frame.
use std::sync::{Mutex, OnceLock};

use mod_api::client::first_person::Hand;
use mod_api::{BlockEntityDrawContext, ClientHost, HandRender, ModelPart};

/// What the game patch reads before the hands are drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Own {
    /// The skin's sprite in the game's atlas, or None for the default skin.
    pub skin: Option<String>,
    pub slim: bool,
    pub main_left: bool,
    pub invisible: bool,
}

static OWN: Mutex<Option<Own>> = Mutex::new(None);

/// The player model's right and left arms, wide then slim.
static ARMS: OnceLock<[[Option<ModelPart>; 2]; 2]> = OnceLock::new();

const STEVE: &str = "entity/player/wide/steve";

/// Called by the game patch each frame the hands are drawn.
pub fn set_own(own: Option<Own>) {
    if let Ok(mut slot) = OWN.lock() {
        *slot = own;
    }
}

fn own() -> Option<Own> {
    OWN.lock().ok().and_then(|o| o.clone())
}

/// Bakes the player model's arms once, at client setup.
pub fn bake(host: &dyn ClientHost) {
    let arms = |layer: &str| {
        let root = host.bake_layer(layer);
        ["right_arm", "left_arm"].map(|name| root.as_ref().and_then(|r| r.get_child(name)).cloned())
    };
    let _ = ARMS.set([arms("minecraft:player#main"), arms("minecraft:player_slim#main")]);
}

/// True for items the game already draws with arms (maps).
pub fn has_own_arms(id: Option<&str>) -> bool {
    id == Some("minecraft:filled_map")
}

/// Draws the arm that holds `render`'s item, at the hand's pose (with every
/// mod's offset applied), before the game draws the item over it.
pub fn draw(render: &HandRender<'_>, draw: &mut dyn BlockEntityDrawContext) {
    let Some(own) = own() else { return };
    if own.invisible {
        return;
    }
    let left = own.main_left != (render.hand == Hand::Off);
    let Some(arms) = ARMS.get() else { return };
    let set = &arms[usize::from(own.slim)];
    let Some(arm) = set[usize::from(left)].as_ref().or(arms[0][usize::from(left)].as_ref()) else {
        return;
    };
    let mut arm = arm.clone();
    // renderPlayerHand: the arm at rest, turned 0.1 radians outwards.
    arm.x_rot = 0.0;
    arm.y_rot = 0.0;
    arm.z_rot = if left { -0.1 } else { 0.1 };
    // The sleeve's end caps, see-through in most skins, would draw here as a
    // tinted square: the sleeve keeps only its four sides.
    for name in ["right_sleeve", "left_sleeve"] {
        if let Some(sleeve) = arm.get_child_mut(name) {
            sleeve.polygons.retain(|p| !p.vertices.iter().all(|v| v.pos[1] == p.vertices[0].pos[1]));
        }
    }

    draw.push();
    // The hand's pose, as the game applies it to the item (`hand_pose_matrix`).
    let [x, y, z] = render.translation;
    let [pitch, yaw, roll] = render.rotation;
    draw.translate([x, y, z]);
    draw.rotate_about([1.0, 0.0, 0.0], pitch, [0.0; 3]);
    draw.rotate_about([0.0, 1.0, 0.0], yaw, [0.0; 3]);
    draw.rotate_about([0.0, 0.0, 1.0], roll, [0.0; 3]);
    if render.scale.is_finite() && render.scale > 0.0 {
        draw.scale([render.scale; 3]);
    }
    arm_pose(draw, left, render.equip, render.swing);
    let sprite = own.skin.as_deref().unwrap_or(STEVE);
    if !draw.model_part(&arm, sprite, 0xFF_FFFF) {
        draw.model_part(&arm, STEVE, 0xFF_FFFF);
    }
    draw.pop();
}

/// `renderPlayerArm` up to `renderPlayerHand` (Horizon's `player_arm_pose`),
/// a little lower and further out so the item sits in the hand.
fn arm_pose(draw: &mut dyn BlockEntityDrawContext, left: bool, equip: f32, attack: f32) {
    use std::f32::consts::{PI, TAU};
    let f = if left { -1.0 } else { 1.0 };
    let root = attack.max(0.0).sqrt();
    let x = -0.3 * (root * PI).sin();
    let y = 0.4 * (root * TAU).sin();
    let z = -0.4 * (attack * PI).sin();
    let z_rot = (attack * attack * PI).sin();
    let y_rot = (root * PI).sin();
    let o = [0.0; 3];
    draw.translate([f * (x + 0.64 + HOLD[0]), y - 0.6 + equip * -0.6 + HOLD[1], z - 0.72 + HOLD[2]]);
    draw.rotate_about([0.0, 1.0, 0.0], f * 45.0, o);
    draw.rotate_about([0.0, 1.0, 0.0], f * y_rot * 70.0, o);
    draw.rotate_about([0.0, 0.0, 1.0], f * z_rot * -20.0, o);
    draw.translate([-f, 3.6, 3.5]);
    draw.rotate_about([0.0, 0.0, 1.0], f * 120.0, o);
    draw.rotate_about([1.0, 0.0, 0.0], 200.0, o);
    draw.rotate_about([0.0, 1.0, 0.0], f * -135.0, o);
    draw.translate([f * 5.6, 0.0, 0.0]);
}

/// How far the arm moves from the empty-hand pose, in blocks (x outwards).
const HOLD: [f32; 3] = [0.16, -0.06, 0.0];
