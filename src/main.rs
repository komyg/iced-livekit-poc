mod api_service;
mod common;
mod connector_page;

use connector_page::ConnectorPage;

use crate::connector_page::{ConnectorPageAction, ConnectorPageMessage};

fn update(page: &mut ConnectorPage, message: ConnectorPageMessage) {
    match page.update(message) {
        ConnectorPageAction::Connect(api_key) => {
            let access_token = api_service::get_access_token(&api_key);
            println!("Access token: {:?}", access_token);
        }
        ConnectorPageAction::None => (),
    }
}

pub fn main() -> iced::Result {
    iced::application(ConnectorPage::default, update, ConnectorPage::view)
        .title("PV Meet Connector")
        .run()
}
