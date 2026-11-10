mod actions;
mod fa_icons;
mod key_repeat;
mod knob_touch;
mod shared;

use actions::{
    AppButtonAction, AppKnobAction, AppSliderAction, ButtonAction, Deck, KnobAction,
    PhysicalSourceSwitchAction, RouteToggleAction, SliderAction, SourceSwitchAction,
};
use openaction::*;

#[tokio::main]
async fn main() -> OpenActionResult<()> {
    {
        use simplelog::*;
        if let Err(error) = TermLogger::init(
            LevelFilter::Debug,
            Config::default(),
            TerminalMode::Stdout,
            ColorChoice::Never,
        ) {
            eprintln!("Logger initialization failed: {error}");
        }
    }

    register_action(Deck(KnobAction)).await;
    register_action(Deck(ButtonAction)).await;
    register_action(Deck(SourceSwitchAction)).await;
    register_action(Deck(PhysicalSourceSwitchAction)).await;
    register_action(Deck(RouteToggleAction)).await;
    register_action(Deck(SliderAction)).await;
    register_action(Deck(AppKnobAction)).await;
    register_action(Deck(AppButtonAction)).await;
    register_action(Deck(AppSliderAction)).await;

    run(std::env::args().collect()).await
}
