mod api_service;
mod common;
mod connector_page;

use connector_page::ConnectorPage;
use iced::{Element, Task};

use crate::connector_page::{ConnectorPageAction, ConnectorPageMessage};

#[derive(Debug, Clone)]
enum Message {
    Connector(ConnectorPageMessage),
    Connected(Result<(), String>),
}

fn update(page: &mut ConnectorPage, message: Message) -> Task<Message> {
    match message {
        Message::Connector(message) => match page.update(message) {
            ConnectorPageAction::Connect(api_key) => Task::perform(
                async move {
                    let token = api_service::get_access_token(&api_key)?;
                    api_service::connect_to_room(token, api_key.api_url).await
                },
                Message::Connected,
            ),
            ConnectorPageAction::None => Task::none(),
        },
        Message::Connected(result) => {
            println!("Connected to room: {result:?}");
            Task::none()
        }
    }
}

fn view(page: &ConnectorPage) -> Element<'_, Message> {
    page.view().map(Message::Connector)
}

pub fn main() -> iced::Result {
    iced::application(ConnectorPage::default, update, view)
        .title("PV Meet Connector")
        .run()
}
