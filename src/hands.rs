//! The first-person hands' motion: how a held item bobs with your steps, drops
//! and tilts into a sprint, dips when you land, rises when you jump, sinks when
//! you crouch and breathes when you stand still; how it strokes when you swim,
//! climbs hand over hand up a ladder, trails behind on an elytra, flinches when
//! you're hurt and flips when you switch items; how food is chewed, a drawn bow
//! trembles and a shield is pushed out; and how a punch, a sword cut, a spear
//! thrust, a tool's chop and a mace slam each swing differently.
//!
//! Turning sway is off by default: Living Camera already makes the hand lag
//! behind the view, and the two would double. Without Living Camera, turn it on.
//!
//! `tick` hears the player 20 times a second, `frame` moves the springs, and
//! `pose` gives the offsets the `render_hand` hook adds to the hand.

use std::cell::RefCell;
use std::f32::consts::PI;

pub const ID: &str = "living_hands";

/// One option of the settings page: its key, English label, choices
/// (value, English text) and default value.
pub struct Opt {
    pub key: &'static str,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the English labels test")
    )]
    pub label: &'static str,
    pub choices: &'static [(u16, &'static str)],
    pub default: u16,
}

const ON_OFF: &[(u16, &str)] = &[(0, "OFF"), (1, "ON")];
const STRENGTH: &[(u16, &str)] = &[
    (0, "OFF"),
    (50, "50%"),
    (100, "100%"),
    (150, "150%"),
    (200, "200%"),
];

pub mod o {
    pub const ENABLED: usize = 0;
    pub const INTENSITY: usize = 1;
    pub const WALK: usize = 2;
    pub const SPRINT: usize = 3;
    pub const JUMP: usize = 4;
    pub const SWING: usize = 5;
    pub const IDLE: usize = 6;
    pub const ACTIONS: usize = 7;
    pub const MOVES: usize = 8;
    pub const HURT: usize = 9;
    pub const SWITCH: usize = 10;
    pub const TURN: usize = 11;
    pub const ARM: usize = 12;
}

const fn strength(key: &'static str, label: &'static str, default: u16) -> Opt {
    Opt {
        key,
        label,
        choices: STRENGTH,
        default,
    }
}

pub const OPTS: [Opt; 13] = [
    Opt {
        key: "enabled",
        label: "Living Hands",
        choices: ON_OFF,
        default: 1,
    },
    strength("intensity", "Overall intensity", 100),
    strength("walk", "Walking bob", 100),
    strength("sprint", "Sprint pose", 100),
    strength("jump", "Jump and landing", 100),
    strength("swing", "Punch and swing", 100),
    strength("idle", "Breathing", 100),
    strength("actions", "Eating, bows, shields", 100),
    strength("moves", "Swim, climb, glide", 100),
    strength("hurt", "Hurt flinch", 100),
    strength("switch", "Item switch flip", 100),
    strength("turn", "Turning sway", 0),
    Opt {
        key: "arm",
        label: "Arm with items",
        choices: ON_OFF,
        default: 1,
    },
];

pub fn label_key(key: &str) -> String {
    format!("{ID}.option.{key}")
}

pub fn choice_key(key: &str, value: u16) -> String {
    format!("{ID}.option.{key}.{value}")
}

/// The settings' values (each option's chosen value, not its index).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    values: [u16; OPTS.len()],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            values: OPTS.map(|opt| opt.default),
        }
    }
}

impl Settings {
    /// From each option's value as text (a missing or unknown one keeps its default).
    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let mut s = Self::default();
        for (i, opt) in OPTS.iter().enumerate() {
            if let Some(v) = get(opt.key).and_then(|v| v.parse::<u16>().ok())
                && opt.choices.iter().any(|c| c.0 == v)
            {
                s.values[i] = v;
            }
        }
        s
    }

    /// From the page's choice indexes.
    pub fn from_choices(index: impl Fn(&str) -> Option<u32>) -> Self {
        Self::read(|key| {
            let opt = OPTS.iter().find(|opt| opt.key == key)?;
            let at = usize::try_from(index(key)?).ok()?;
            opt.choices.get(at).map(|c| c.0.to_string())
        })
    }

    /// Option `i`'s choice index, as the page keeps it.
    pub fn choice_index(&self, i: usize) -> String {
        OPTS[i]
            .choices
            .iter()
            .position(|c| c.0 == self.values[i])
            .unwrap_or(0)
            .to_string()
    }

    pub fn enabled(&self) -> bool {
        self.values[o::ENABLED] != 0
    }

    /// Whether your arm is drawn under held items.
    pub fn arm(&self) -> bool {
        self.enabled() && self.values[o::ARM] != 0
    }

    /// Option `i` as a factor, times the overall intensity; 0 when the mod is off.
    pub fn factor(&self, i: usize) -> f32 {
        if !self.enabled() {
            return 0.0;
        }
        f32::from(self.values[i]) / 100.0 * f32::from(self.values[o::INTENSITY]) / 100.0
    }
}

/// What the hand holds, for how it swings, how it's used and how it's carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Fist,
    Blade,
    Spear,
    Tool,
    Mace,
    Bow,
    Shield,
    Light,
    /// Held to look at or aim (maps, a spyglass, a compass): carried steadier.
    Steady,
    Other,
}

/// The kind of an item by its id (`minecraft:iron_sword`).
pub fn kind_of(id: Option<&str>) -> Kind {
    let Some(id) = id else { return Kind::Fist };
    let path = id.rsplit(':').next().unwrap_or(id);
    let ends = |suffixes: &[&str]| suffixes.iter().any(|s| path.ends_with(s));
    if ends(&["_sword"]) {
        Kind::Blade
    } else if path == "trident" || ends(&["_spear"]) {
        Kind::Spear
    } else if path == "mace" {
        Kind::Mace
    } else if ends(&["_axe", "_pickaxe", "_shovel", "_hoe"]) {
        Kind::Tool
    } else if path == "bow" || path == "crossbow" {
        Kind::Bow
    } else if path == "shield" {
        Kind::Shield
    } else if ends(&["torch", "lantern"]) {
        Kind::Light
    } else if [
        "map",
        "filled_map",
        "spyglass",
        "compass",
        "recovery_compass",
        "clock",
        "brush",
        "goat_horn",
    ]
    .contains(&path)
    {
        Kind::Steady
    } else {
        Kind::Other
    }
}

/// Offsets added to the hand: `translation` in blocks (x right, y up, z back)
/// and `rotation` as pitch, yaw and roll in degrees. Horizon turns the hand
/// about the eye, not about the hand: a positive pitch lifts the item, a
/// positive yaw brings it toward the centre and a few degrees move it far, so
/// the turns here stay small.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub translation: [f32; 3],
    pub rotation: [f32; 3],
}

/// No added turn goes past this, whatever the strengths.
const MAX_TURN: f32 = 15.0;

/// The player as the last tick saw them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Player {
    pub pos: [f64; 3],
    pub on_ground: bool,
    pub sprinting: bool,
    pub crouching: bool,
    pub in_water: bool,
    pub climbing: bool,
    pub gliding: bool,
    pub blocking: bool,
    /// The hand using an item (eating, drawing a bow, raising a shield):
    /// `Some(true)` the main hand.
    pub using: Option<bool>,
    pub health: f32,
    pub slot: usize,
}

#[derive(Default)]
struct State {
    settings: Option<Settings>,
    last: Option<Player>,
    /// Horizontal speed in blocks per second, as the last tick measured it.
    speed: f32,
    /// Vertical speed in blocks per second.
    rise: f32,
    /// Smoothed speed, step phase, and the blends (0 to 1) of each state.
    speed_s: f32,
    phase: f32,
    sprint_s: f32,
    crouch_s: f32,
    swim_s: f32,
    climb_s: f32,
    glide_s: f32,
    block_s: f32,
    swim_phase: f32,
    climb_phase: f32,
    /// Seconds the current item has been used for.
    use_time: f32,
    /// The landing and jump spring: its offset (blocks, down negative) and speed.
    dip: f32,
    dip_v: f32,
    /// The hurt spring and the item switch spring, each with its speed.
    flinch: f32,
    flinch_v: f32,
    flip: f32,
    flip_v: f32,
    /// The view's yaw and pitch last frame, and the hand's lag behind turns.
    look: Option<[f32; 2]>,
    turn: [f32; 2],
    time: f32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// The settings, read from `get` the first time, then kept.
pub fn settings(get: impl Fn(&str) -> Option<String>) -> Settings {
    STATE.with_borrow_mut(|s| *s.settings.get_or_insert_with(|| Settings::read(get)))
}

pub fn set_settings(settings: Settings) {
    STATE.with_borrow_mut(|s| s.settings = Some(settings));
}

/// Walking is about 4.3 blocks a second: one bob cycle each 1.6 blocks.
const STRIDE: f32 = 1.6;
/// The springs: stiffness and damping per second.
const SPRING_K: f32 = 140.0;
const SPRING_C: f32 = 15.0;

/// A tick (20 a second): `None` without a player in a world.
pub fn tick(player: Option<Player>) {
    STATE.with_borrow_mut(|s| {
        let Some(p) = player else {
            s.last = None;
            s.speed = 0.0;
            s.rise = 0.0;
            s.use_time = 0.0;
            return;
        };
        if let Some(last) = s.last {
            let (dx, dy, dz) = (
                p.pos[0] - last.pos[0],
                p.pos[1] - last.pos[1],
                p.pos[2] - last.pos[2],
            );
            s.speed = (dx.hypot(dz) * 20.0) as f32;
            let rise = (dy * 20.0) as f32;
            // Landing: the harder the fall, the deeper the dip.
            if p.on_ground && !last.on_ground {
                s.dip_v -= landing_kick(s.rise);
            }
            // A jump lifts the hand a little as you leave the ground.
            if !p.on_ground && last.on_ground && rise > 2.0 {
                s.dip_v += 0.35;
            }
            s.rise = rise;
            // A teleport or a respawn isn't a step.
            if s.speed > 40.0 {
                s.speed = 0.0;
                s.rise = 0.0;
            }
            // Hurt: the hand is knocked down, harder for a bigger hit.
            s.flinch_v -= flinch_kick(last.health - p.health);
            // Another item: the hand flips it up as it comes in.
            if p.slot != last.slot {
                s.flip_v += 1.0;
            }
        }
        s.use_time = if p.using.is_some() && s.last.is_some_and(|l| l.using == p.using) {
            s.use_time + 0.05
        } else {
            0.0
        };
        s.last = Some(p);
    });
}

/// How hard a landing at `rise` blocks per second (negative falling) pushes
/// the spring: nothing for a step down, up to a hard thud.
pub fn landing_kick(rise: f32) -> f32 {
    ((-rise - 3.0) / 25.0).clamp(0.0, 0.8)
}

/// How hard losing `lost` health (half hearts) knocks the hand: nothing for
/// healing, up to a heavy hit.
pub fn flinch_kick(lost: f32) -> f32 {
    if lost < 0.5 {
        0.0
    } else {
        (0.3 + lost * 0.05).min(0.7)
    }
}

/// A frame: `dt` seconds since the last one, with the view's yaw and pitch.
pub fn frame(dt: f32, look: Option<[f32; 2]>) {
    let dt = dt.clamp(0.0, 0.1);
    STATE.with_borrow_mut(|s| {
        s.time += dt;
        let follow = |current: f32, target: f32, rate: f32| {
            current + (target - current) * (1.0 - (-rate * dt).exp())
        };
        let p = s.last.unwrap_or_default();
        let blend = |on: bool| if on { 1.0 } else { 0.0 };
        s.speed_s = follow(s.speed_s, s.speed.min(8.0), 10.0);
        s.sprint_s = follow(s.sprint_s, blend(p.sprinting), 7.0);
        s.crouch_s = follow(s.crouch_s, blend(p.crouching), 9.0);
        s.swim_s = follow(s.swim_s, blend(p.in_water && !p.on_ground), 4.0);
        s.climb_s = follow(s.climb_s, blend(p.climbing && !p.on_ground), 8.0);
        s.glide_s = follow(s.glide_s, blend(p.gliding), 5.0);
        s.block_s = follow(s.block_s, blend(p.blocking), 12.0);
        s.phase = (s.phase + s.speed_s * dt / STRIDE * 2.0 * PI) % (2.0 * PI * 64.0);
        // A stroke each second or so, quicker when swimming fast.
        let stroke = 3.0 + s.speed_s.min(6.0);
        s.swim_phase = (s.swim_phase + stroke * dt) % (2.0 * PI * 64.0);
        // Hand over hand: one reach each half block climbed.
        s.climb_phase = (s.climb_phase + s.rise.abs().min(6.0) * dt * 2.0 * PI) % (2.0 * PI * 64.0);
        // The springs settle back to rest, a little past it first.
        for (x, v) in [
            (&mut s.dip, &mut s.dip_v),
            (&mut s.flinch, &mut s.flinch_v),
            (&mut s.flip, &mut s.flip_v),
        ] {
            let accel = -SPRING_K * *x - SPRING_C * *v;
            *v += accel * dt;
            *x = (*x + *v * dt).clamp(-0.4, 0.4);
        }
        // Turning: the hand lags behind the view, then catches up.
        let mut target = [0.0; 2];
        if let (Some(now), Some(before)) = (look, s.look)
            && dt > 0.0
        {
            let yaw = (now[0] - before[0] + 540.0).rem_euclid(360.0) - 180.0;
            let pitch = now[1] - before[1];
            target = [
                (-yaw / dt * 0.0002).clamp(-0.06, 0.06),
                (pitch / dt * 0.0002).clamp(-0.05, 0.05),
            ];
        }
        s.turn = [follow(s.turn[0], target[0], 12.0), follow(s.turn[1], target[1], 12.0)];
        s.look = look;
    });
}

/// The motion inputs the pose reads, apart from the swing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub speed: f32,
    pub phase: f32,
    pub sprint: f32,
    pub crouch: f32,
    pub dip: f32,
    pub time: f32,
    pub swim: f32,
    pub swim_phase: f32,
    pub climb: f32,
    pub climb_phase: f32,
    pub glide: f32,
    pub block: f32,
    pub using: Option<bool>,
    pub use_time: f32,
    pub flinch: f32,
    pub flip: f32,
    pub turn: [f32; 2],
}

pub fn motion() -> Motion {
    STATE.with_borrow(|s| Motion {
        speed: s.speed_s,
        phase: s.phase,
        sprint: s.sprint_s,
        crouch: s.crouch_s,
        dip: s.dip,
        time: s.time,
        swim: s.swim_s,
        swim_phase: s.swim_phase,
        climb: s.climb_s,
        climb_phase: s.climb_phase,
        glide: s.glide_s,
        block: s.block_s,
        using: s.last.and_then(|p| p.using),
        use_time: s.use_time,
        flinch: s.flinch,
        flip: s.flip,
        turn: s.turn,
    })
}

pub fn current_settings() -> Settings {
    STATE.with_borrow(|s| s.settings.unwrap_or_default())
}

/// The offsets for one hand: `main` the main hand (the off hand mirrors it),
/// holding `kind`, at swing progress `swing` (0 to 1).
pub fn pose(settings: &Settings, m: Motion, kind: Kind, main: bool, swing: f32) -> Pose {
    let mut t = [0.0f32; 3];
    let mut r = [0.0f32; 3];
    let side = if main { 1.0 } else { -1.0 };
    let moves = settings.factor(o::MOVES);
    let actions = settings.factor(o::ACTIONS);
    // Swimming, climbing and gliding take over from the steps.
    let afoot = 1.0 - (m.swim.max(m.climb).max(m.glide) * moves.min(1.0));
    let steady = if kind == Kind::Steady { 0.5 } else { 1.0 };

    // Walking bob: side to side once a stride, down twice, with a slight roll.
    let walk = settings.factor(o::WALK) * (m.speed / 4.3).clamp(0.0, 1.6) * afoot * steady;
    t[0] += 0.02 * m.phase.sin() * walk;
    t[1] -= 0.014 * (1.0 - (2.0 * m.phase).cos()) / 2.0 * walk;
    r[2] += 2.5 * m.phase.sin() * walk;

    // Sprint: a weapon or tool is carried low and tilted; anything else just lowers.
    let sprint = settings.factor(o::SPRINT) * m.sprint * afoot;
    match kind {
        Kind::Blade | Kind::Tool | Kind::Mace | Kind::Spear => {
            r[0] -= 4.0 * sprint;
            r[2] -= 6.0 * sprint;
            t[0] += 0.03 * sprint;
            t[1] -= 0.05 * sprint;
        }
        _ => {
            r[0] -= 3.0 * sprint;
            t[1] -= 0.06 * sprint;
        }
    }

    // Crouching lowers the hand a little.
    t[1] -= 0.03 * m.crouch * settings.factor(o::WALK);

    // Jump and landing: the spring moves the hand, and tips it with the dip.
    let jump = settings.factor(o::JUMP);
    t[1] += m.dip * jump;
    r[0] += m.dip * 15.0 * jump;

    // Standing still, the hand breathes.
    let still = (1.0 - m.speed / 1.5).clamp(0.0, 1.0) * afoot;
    let idle = settings.factor(o::IDLE) * still * steady;
    t[1] += 0.005 * (m.time * 1.7).sin() * idle;
    r[0] += 0.6 * (m.time * 1.7).sin() * idle;

    // Swimming: a slow breaststroke, both hands sweeping out and back.
    let swim = m.swim * moves;
    t[0] += 0.035 * m.swim_phase.sin() * swim;
    t[2] += 0.03 * m.swim_phase.cos() * swim;
    t[1] -= 0.03 * swim;

    // Climbing: the hands come up and reach in turn.
    let climb = m.climb * moves;
    t[1] += (0.04 + 0.035 * m.climb_phase.sin() * side) * climb;
    r[0] += 3.0 * climb;

    // Gliding: the hands trail low and back.
    let glide = m.glide * moves;
    t[1] -= 0.06 * glide;
    t[2] += 0.08 * glide;
    r[0] -= 6.0 * glide;

    // A light is held up and out, to see by.
    if kind == Kind::Light {
        t[1] += 0.04 * settings.factor(o::INTENSITY).min(1.0);
        r[0] += 2.0 * settings.factor(o::INTENSITY).min(1.0);
    }

    // Using the item in this hand.
    if m.using == Some(main) {
        // Drawn back as it charges, then trembling once it's held full.
        let draw = (m.use_time / 1.0).clamp(0.0, 1.0);
        let shake = ((m.use_time - 1.0) / 1.5).clamp(0.0, 1.0) * actions;
        let tremble = |t: &mut [f32; 3]| {
            t[0] += 0.004 * (m.time * 41.0).sin() * shake;
            t[1] += 0.004 * (m.time * 57.0 + 1.0).sin() * shake;
        };
        match kind {
            Kind::Bow => {
                t[2] += 0.03 * draw * actions;
                tremble(&mut t);
            }
            Kind::Spear => {
                t[2] += 0.05 * draw * actions;
                t[1] += 0.03 * draw * actions;
                tremble(&mut t);
            }
            Kind::Shield | Kind::Steady => {}
            // Food and drink: a chewing bob.
            _ => {
                t[1] += 0.012 * (m.time * 11.0).sin() * actions;
                r[2] += 1.5 * (m.time * 11.0).sin() * actions;
            }
        }
    }

    // A raised shield is pushed out and in front.
    if kind == Kind::Shield {
        t[2] -= 0.05 * m.block * actions;
        t[0] -= 0.04 * m.block * actions;
    }

    // Hurt: knocked down and twisted, then springs back.
    let hurt = settings.factor(o::HURT);
    t[1] += m.flinch * 2.0 * hurt;
    r[0] += m.flinch * 150.0 * hurt;
    r[2] -= m.flinch * 150.0 * hurt;

    // Switching items: the new one comes in with a flip.
    let flip = settings.factor(o::SWITCH);
    t[1] -= m.flip * flip;
    r[2] += m.flip * 150.0 * flip;

    // The swing, on top of vanilla's: a punch jabs forward, a blade cuts across,
    // a spear thrusts, a tool winds up and chops down, a mace lifts high and slams.
    if main && swing > 0.0 {
        let s = settings.factor(o::SWING);
        let swing = swing.clamp(0.0, 1.0);
        let arc = (PI * swing).sin();
        // Up for the first half, down for the second.
        let chop = (2.0 * PI * swing).sin();
        match kind {
            Kind::Fist => {
                t[2] -= 0.12 * arc * s;
                r[0] -= 6.0 * arc * s;
            }
            Kind::Blade => {
                r[1] += 12.0 * arc * s;
                r[2] -= 6.0 * arc * s;
            }
            Kind::Spear => {
                t[2] -= 0.18 * arc * s;
                r[0] -= 3.0 * arc * s;
            }
            Kind::Tool => {
                r[0] += 8.0 * chop * s;
                t[2] -= 0.05 * arc * s;
            }
            Kind::Mace => {
                r[0] += 11.0 * chop * s;
                t[1] += 0.08 * chop * s;
            }
            Kind::Shield => {
                t[2] -= 0.08 * arc * s;
            }
            Kind::Bow | Kind::Light | Kind::Steady | Kind::Other => {}
        }
    }

    if !main {
        t[0] = -t[0];
        r[1] = -r[1];
        r[2] = -r[2];
    }

    // Turning sway: both hands lag the same way.
    let turn = settings.factor(o::TURN);
    t[0] += m.turn[0] * turn;
    t[1] += m.turn[1] * turn;
    r[2] += m.turn[0] * 60.0 * turn;

    Pose {
        translation: t,
        rotation: r.map(|a| a.clamp(-MAX_TURN, MAX_TURN)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest() -> Motion {
        Motion::default()
    }

    fn quiet() -> Settings {
        Settings::read(|k| (k == "idle").then(|| "0".into()))
    }

    #[test]
    fn standing_still_with_breathing_off_is_the_stock_hand() {
        assert_eq!(pose(&quiet(), rest(), Kind::Other, true, 0.0), Pose::default());
    }

    #[test]
    fn turning_the_mod_off_leaves_the_hand_alone() {
        let s = Settings::read(|k| (k == "enabled").then(|| "0".into()));
        let busy = Motion {
            speed: 5.6,
            phase: 1.0,
            sprint: 1.0,
            crouch: 1.0,
            dip: -0.2,
            time: 3.0,
            swim: 1.0,
            climb: 1.0,
            glide: 1.0,
            block: 1.0,
            using: Some(true),
            use_time: 3.0,
            flinch: -0.02,
            flip: 0.03,
            turn: [0.05, 0.02],
            ..rest()
        };
        assert_eq!(pose(&s, busy, Kind::Blade, true, 0.5), Pose::default());
        assert_eq!(pose(&s, busy, Kind::Bow, true, 0.0), Pose::default());
    }

    #[test]
    fn items_are_sorted_by_kind() {
        assert_eq!(kind_of(None), Kind::Fist);
        assert_eq!(kind_of(Some("minecraft:diamond_sword")), Kind::Blade);
        assert_eq!(kind_of(Some("minecraft:trident")), Kind::Spear);
        assert_eq!(kind_of(Some("minecraft:iron_spear")), Kind::Spear);
        assert_eq!(kind_of(Some("minecraft:netherite_pickaxe")), Kind::Tool);
        assert_eq!(kind_of(Some("minecraft:wooden_hoe")), Kind::Tool);
        assert_eq!(kind_of(Some("minecraft:mace")), Kind::Mace);
        assert_eq!(kind_of(Some("minecraft:crossbow")), Kind::Bow);
        assert_eq!(kind_of(Some("minecraft:shield")), Kind::Shield);
        assert_eq!(kind_of(Some("minecraft:soul_torch")), Kind::Light);
        assert_eq!(kind_of(Some("minecraft:lantern")), Kind::Light);
        assert_eq!(kind_of(Some("minecraft:filled_map")), Kind::Steady);
        assert_eq!(kind_of(Some("minecraft:grass_block")), Kind::Other);
        // Not a pickaxe: the suffix must be whole.
        assert_eq!(kind_of(Some("minecraft:waxe")), Kind::Other);
    }

    #[test]
    fn the_off_hand_mirrors_the_main_one() {
        let s = Settings::default();
        let m = Motion {
            speed: 4.3,
            phase: 1.2,
            sprint: 0.5,
            ..rest()
        };
        let main = pose(&s, m, Kind::Other, true, 0.0);
        let off = pose(&s, m, Kind::Other, false, 0.0);
        assert_eq!(off.translation[0], -main.translation[0]);
        assert_eq!(off.translation[1], main.translation[1]);
        assert_eq!(off.rotation[2], -main.rotation[2]);
    }

    #[test]
    fn each_kind_swings_its_own_way() {
        let s = quiet();
        let mid = |kind| pose(&s, rest(), kind, true, 0.5);
        assert!(mid(Kind::Fist).translation[2] < -0.1, "a punch jabs forward");
        assert!(mid(Kind::Blade).rotation[1] > 10.0, "a blade cuts across");
        assert!(mid(Kind::Spear).translation[2] < -0.15, "a spear thrusts");
        let at = |kind, swing| pose(&s, rest(), kind, true, swing);
        let (early, late) = (at(Kind::Tool, 0.25).rotation[0], at(Kind::Tool, 0.75).rotation[0]);
        assert!(early > 7.0 && late < -7.0, "a tool winds up, then strikes: {early} {late}");
        let (up, down) = (at(Kind::Mace, 0.25), at(Kind::Mace, 0.75));
        assert!(up.translation[1] > 0.07 && down.translation[1] < -0.07, "a mace lifts and slams");
        assert_eq!(mid(Kind::Other), Pose::default());
        // Every swing stays within a small turn about the eye.
        for kind in [Kind::Fist, Kind::Blade, Kind::Spear, Kind::Tool, Kind::Mace] {
            for step in 0..=10 {
                let p = pose(&s, rest(), kind, true, step as f32 / 10.0);
                assert!(p.rotation.iter().all(|a| a.abs() <= 12.0), "{kind:?} {p:?}");
            }
        }
        // Only the main hand swings, and nothing moves at rest or at the end.
        assert_eq!(pose(&s, rest(), Kind::Fist, false, 0.5), Pose::default());
        assert!(pose(&s, rest(), Kind::Fist, true, 1.0).translation[2].abs() < 1e-6);
    }

    #[test]
    fn sprinting_carries_a_weapon_low_and_tilted() {
        let s = Settings::default();
        let m = Motion {
            sprint: 1.0,
            ..rest()
        };
        let p = pose(&s, m, Kind::Blade, true, 0.0);
        assert!(p.rotation[0] < 0.0 && p.translation[1] < -0.04);
    }

    #[test]
    fn swimming_climbing_and_gliding_replace_the_steps() {
        let s = quiet();
        let walking = Motion {
            speed: 4.3,
            phase: PI / 2.0,
            ..rest()
        };
        let swimming = Motion { swim: 1.0, ..walking };
        // In water the step's sway is gone; the stroke moves the hand instead.
        let p = pose(&s, swimming, Kind::Other, true, 0.0);
        assert!(p.translation[1] < -0.02, "{p:?}");
        assert!(p.rotation[2].abs() < 1e-4, "no step roll in water: {p:?}");
        // Climbing: one hand up while the other is down.
        let climbing = Motion {
            climb: 1.0,
            climb_phase: PI / 2.0,
            ..rest()
        };
        let main = pose(&s, climbing, Kind::Fist, true, 0.0).translation[1];
        let off = pose(&s, climbing, Kind::Fist, false, 0.0).translation[1];
        assert!(main > 0.07 && off < 0.01, "{main} {off}");
        let gliding = Motion { glide: 1.0, ..rest() };
        let p = pose(&s, gliding, Kind::Fist, true, 0.0);
        assert!(p.translation[1] < -0.05 && p.translation[2] > 0.07);
    }

    #[test]
    fn using_an_item_moves_only_that_hand() {
        let s = quiet();
        let drawn = Motion {
            using: Some(true),
            use_time: 3.0,
            time: 0.3,
            ..rest()
        };
        let main = pose(&s, drawn, Kind::Bow, true, 0.0);
        assert!(main.translation[2] > 0.02, "pulled back: {main:?}");
        assert_eq!(pose(&s, drawn, Kind::Bow, false, 0.0), Pose::default());
        // A bow only trembles once it's been held drawn a while.
        let fresh = Motion { use_time: 0.5, ..drawn };
        let shaking = |m: Motion| pose(&s, m, Kind::Bow, true, 0.0).translation[0].abs();
        assert_eq!(shaking(fresh), 0.0);
        assert!(shaking(drawn) > 0.0);
        // Food is chewed.
        let eating = Motion { time: 0.15, ..drawn };
        assert!(pose(&s, eating, Kind::Other, true, 0.0).translation[1].abs() > 0.005);
        // A raised shield goes out in front.
        let blocking = Motion { block: 1.0, ..rest() };
        assert!(pose(&s, blocking, Kind::Shield, false, 0.0).translation[2] < -0.04);
    }

    #[test]
    fn hits_and_switches_kick_springs_that_settle() {
        assert_eq!(flinch_kick(-4.0), 0.0, "healing");
        assert_eq!(flinch_kick(0.2), 0.0);
        assert!(flinch_kick(2.0) > 0.3);
        assert_eq!(flinch_kick(40.0), 0.7, "capped");
        let base = Player {
            pos: [0.0, 64.0, 0.0],
            on_ground: true,
            health: 20.0,
            ..Player::default()
        };
        tick(Some(base));
        tick(Some(Player {
            health: 14.0,
            slot: 3,
            ..base
        }));
        let mut peak = (0.0f32, 0.0f32);
        for _ in 0..240 {
            frame(1.0 / 60.0, None);
            let m = motion();
            peak = (peak.0.min(m.flinch), peak.1.max(m.flip));
        }
        assert!(peak.0 < -0.01 && peak.1 > 0.02, "{peak:?}");
        assert!(motion().flinch.abs() < 0.001 && motion().flip.abs() < 0.001);
    }

    #[test]
    fn turning_sway_is_off_by_default_and_both_hands_lag_together() {
        let m = Motion {
            turn: [-0.04, 0.0],
            ..rest()
        };
        assert_eq!(pose(&quiet(), m, Kind::Other, true, 0.0), Pose::default());
        let on = Settings::read(|k| match k {
            "idle" => Some("0".into()),
            "turn" => Some("100".into()),
            _ => None,
        });
        let main = pose(&on, m, Kind::Other, true, 0.0).translation[0];
        let off = pose(&on, m, Kind::Other, false, 0.0).translation[0];
        assert!(main < -0.03 && main == off);
    }

    #[test]
    fn a_quick_turn_makes_the_hand_lag_then_catch_up() {
        frame(1.0 / 60.0, Some([0.0, 0.0]));
        for i in 1..=6 {
            frame(1.0 / 60.0, Some([i as f32 * 3.0, 0.0]));
        }
        assert!(motion().turn[0] < -0.02, "lags left on a right turn: {:?}", motion().turn);
        for _ in 0..60 {
            frame(1.0 / 60.0, Some([18.0, 0.0]));
        }
        assert!(motion().turn[0].abs() < 0.002);
        // Wrapping from 359° to 1° is a small turn, not a spin.
        frame(1.0 / 60.0, Some([359.0, 0.0]));
        frame(1.0 / 60.0, Some([1.0, 0.0]));
        assert!(motion().turn[0].abs() < 0.03);
    }

    #[test]
    fn landings_kick_harder_from_higher() {
        assert_eq!(landing_kick(-2.0), 0.0, "a step down");
        assert!(landing_kick(-10.0) > 0.2);
        assert_eq!(landing_kick(-80.0), 0.8, "capped");
    }

    #[test]
    fn the_landing_spring_settles() {
        tick(Some(Player {
            pos: [0.0, 70.0, 0.0],
            ..Player::default()
        }));
        tick(Some(Player {
            pos: [0.0, 69.0, 0.0],
            on_ground: true,
            ..Player::default()
        }));
        let start = motion().dip;
        for _ in 0..240 {
            frame(1.0 / 60.0, None);
        }
        assert!(motion().dip.abs() < 0.01, "{start} -> {}", motion().dip);
    }

    #[test]
    fn intensity_scales_every_effect() {
        let half = Settings::read(|k| (k == "intensity").then(|| "50".into()));
        let full = Settings::default();
        let m = Motion {
            speed: 4.3,
            phase: 1.0,
            ..rest()
        };
        let a = pose(&half, m, Kind::Other, true, 0.0).rotation[2];
        let b = pose(&full, m, Kind::Other, true, 0.0).rotation[2];
        assert!((a * 2.0 - b).abs() < 1e-4);
    }

    #[test]
    fn no_turn_goes_too_far() {
        let s = Settings::read(|k| (k != "enabled").then(|| "200".into()));
        let wild = Motion {
            speed: 8.0,
            phase: 1.0,
            sprint: 1.0,
            dip: -0.4,
            flinch: -0.4,
            flip: 0.4,
            turn: [0.06, 0.05],
            ..rest()
        };
        for kind in [Kind::Mace, Kind::Blade, Kind::Other] {
            let p = pose(&s, wild, kind, true, 0.3);
            assert!(p.rotation.iter().all(|a| a.abs() <= MAX_TURN), "{kind:?} {p:?}");
        }
    }

    #[test]
    fn the_english_labels_match_the_lang_file() {
        let lang = include_str!("../assets/living_hands/lang/en_us.json");
        let has = |key: String, text: &str| lang.contains(&format!("\"{key}\": \"{text}\""));
        for opt in OPTS {
            assert!(has(label_key(opt.key), opt.label), "{}", opt.key);
            for (v, text) in opt.choices {
                assert!(has(choice_key(opt.key, *v), text), "{} {v}", opt.key);
            }
        }
    }
}
