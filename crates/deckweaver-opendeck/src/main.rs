mod actions;
mod fa_icons;
mod knob_touch;
mod shared;

use actions::{
    ButtonAction, KnobAction, PhysicalSourceSwitchAction, SliderAction, SourceSwitchAction,
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

    register_action(KnobAction).await;
    register_action(ButtonAction).await;
    register_action(SourceSwitchAction).await;
    register_action(PhysicalSourceSwitchAction).await;
    register_action(SliderAction).await;

    run(std::env::args().collect()).await
}
