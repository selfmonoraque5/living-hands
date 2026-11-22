//! Living Hands on Horizon's mod API: the hooks that feed `hands.rs` (the
//! player's movement each tick, the frame time) and add the pose it gives to
//! each first-person hand.
use mod_api::client::first_person::Hand;
use mod_api::{
    self, ClientHost, ClientSetupEvent, ClientTickPostEvent, Flow, Hooks, Registrar,
    RegistrationContext, RenderFramePreEvent, RenderHandEvent, SettingChangedEvent, SettingSpec,
};

pub mod arm;
mod hands;

use hands::{OPTS, Player, Settings};

/// Setting `key` as the hands read it (its value, not the page's choice index).
fn value(host: &dyn ClientHost, key: &str) -> Option<String> {
    let opt = OPTS.iter().find(|opt| opt.key == key)?;
    let at = usize::try_from(host.setting_choice(key)?).ok()?;
    opt.choices.get(at).map(|c| c.0.to_string())
}

#[derive(Default)]
struct LivingHands;

#[mod_api::horizon_mod]
impl Hooks for LivingHands {
    /// The settings page: one choice per option.
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        let defaults = Settings::default();
        for (i, opt) in OPTS.iter().enumerate() {
            let options = opt
                .choices
                .iter()
                .map(|(v, _)| hands::choice_key(opt.key, *v));
            let default = defaults.choice_index(i).parse().unwrap_or(0);
            let _ = r.register_setting(SettingSpec::choice(
                opt.key,
                hands::label_key(opt.key),
                options,
                default,
            ));
        }
    }

    /// The player model's arms, baked once.
    fn client_setup(&self, host: &mut dyn ClientHost, _: &mut ClientSetupEvent) {
        arm::bake(&*host);
    }

    fn setting_changed(&self, host: &mut dyn ClientHost, _: &mut SettingChangedEvent<'_>) {
        hands::set_settings(Settings::from_choices(|key| host.setting_choice(key)));
    }

    /// Each tick: where the player is, how they move (standing, sprinting,
    /// crouching, swimming, climbing, gliding), what they use, their health
    /// and their hotbar slot.
    fn client_tick_post(&self, host: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        let reads: &dyn ClientHost = &*host;
        let player = reads.local_player().and_then(|p| {
            let e = p.entity();
            let l = reads.living(e)?;
            let using = (l.item_use_remaining(reads).unwrap_or(0) > 0)
                .then(|| l.used_item_hand(reads).map(|hand| hand == Hand::Main))
                .flatten();
            Some(Player {
                pos: e.position(reads)?,
                on_ground: e.on_ground(reads).unwrap_or(true),
                sprinting: e.is_sprinting(reads).unwrap_or(false),
                crouching: e.is_crouching(reads).unwrap_or(false),
                in_water: e.is_in_water(reads).unwrap_or(false),
                climbing: l.is_climbing(reads).unwrap_or(false),
                gliding: l.is_gliding(reads).unwrap_or(false),
                blocking: l.is_blocking(reads).unwrap_or(false),
                using,
                health: l.health(reads).unwrap_or(0.0),
                slot: p.selected_slot(reads).unwrap_or(0),
            })
        });
        hands::tick(player);
    }

    /// Each frame: the springs move on, and the view's turn is measured.
    fn render_frame_pre(&self, host: &mut dyn ClientHost, event: &mut RenderFramePreEvent) {
        hands::settings(|key| value(&*host, key));
        let reads: &dyn ClientHost = &*host;
        let look = reads.local_player().and_then(|p| p.entity().rotation(reads));
        hands::frame(event.elapsed.as_secs_f32(), look);
    }

    /// Each first-person hand: the pose for what it holds, added to whatever
    /// other mods (Living Camera's turning sway) already set.
    fn render_hand(&self, host: &dyn ClientHost, event: &mut RenderHandEvent<'_, '_>) -> Flow {
        let render = &mut *event.render;
        let id = render.stack.map(|stack| stack.item().id(host));
        let kind = hands::kind_of(id.as_deref());
        let main = render.hand == Hand::Main;
        let settings = hands::current_settings();
        let pose = hands::pose(
            &settings,
            hands::motion(),
            kind,
            main,
            render.swing,
        );
        for i in 0..3 {
            render.translation[i] += pose.translation[i];
            render.rotation[i] += pose.rotation[i];
        }
        if settings.arm() && render.stack.is_some() && !arm::has_own_arms(id.as_deref()) {
            arm::draw(render, &mut *event.draw);
        }
        Flow::Continue
    }
}
