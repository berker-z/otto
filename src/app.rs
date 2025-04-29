// src/app.rs

pub mod style;

use iced::{executor, Application, Command, Element, Theme, Color};
use iced::widget::{column, container, text};
use crate::app::style::{TITLE_COLOR, SUBTITLE_COLOR};

#[derive(Default)]
pub struct OttoApp;

#[derive(Debug, Clone)]
pub enum Message {}

impl Application for OttoApp {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        (Self::default(), Command::none())
    }

    fn title(&self) -> String {
        "Otto — sovereign chat client".into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn update(&mut self, _message: Message) -> Command<Message> {
        Command::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let content = column![
            text("hello otto.").style(TITLE_COLOR),
            text("this is your sovereign chat window.").style(SUBTITLE_COLOR),
        ];

        container(content)
            .padding(40)
            .center_x()
            .center_y()
            .into()
    }
}