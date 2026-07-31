mod connector_page;

use connector_page::ConnectorPage;

pub fn main() -> iced::Result {
    iced::application(
        ConnectorPage::default,
        ConnectorPage::update,
        ConnectorPage::view,
    )
    .title("PV Meet Connector")
    .run()
}
