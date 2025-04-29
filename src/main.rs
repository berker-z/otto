mod app;

use app::OttoApp;
use iced::{Application, Settings};

fn main() -> iced::Result {
    OttoApp::run(Settings::default())
}
