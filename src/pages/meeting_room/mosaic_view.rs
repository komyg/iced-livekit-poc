use std::collections::HashMap;

use iced::widget::{column, container, responsive, row, shader, space, stack, text};
use iced::{Background, Color, Element, Length, Size, border};

use super::data::Roster;
use crate::video::video_sink::{Frame, VideoSink};

pub const MAX_TILES: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct Tile<'a> {
    pub identity: &'a str,
    pub label: &'a str,
    pub frame: Option<&'a Frame>,
    pub is_local: bool,
}

pub fn ordered_tiles<'a>(
    local: Option<&'a (String, String)>,
    roster: &'a Roster,
    frames: &'a HashMap<String, Frame>,
) -> Vec<Tile<'a>> {
    let tile = |identity: &'a String, label: &'a String, is_local: bool| Tile {
        identity,
        label,
        frame: frames.get(identity),
        is_local,
    };

    let mut tiles: Vec<Tile<'a>> = local
        .iter()
        .map(|(identity, label)| tile(identity, label, true))
        .chain(
            roster
                .iter()
                .map(|(identity, label)| tile(identity, label, false)),
        )
        .collect();

    tiles.sort_by_cached_key(|tile| (tile.label.to_lowercase(), tile.identity.to_owned()));
    tiles.truncate(MAX_TILES);

    tiles
}

pub const fn grid_shape(count: usize, landscape: bool) -> (usize, usize) {
    let capacity = if count == 0 {
        1
    } else {
        count.next_power_of_two()
    };
    let capacity = if capacity > MAX_TILES {
        MAX_TILES
    } else {
        capacity
    };

    // Exact for powers of two, and `capacity` is one by construction.
    let exponent = capacity.trailing_zeros();
    let long = 1_usize << exponent.div_ceil(2);
    let short = capacity / long;

    if landscape {
        (long, short)
    } else {
        (short, long)
    }
}

pub fn view<'a, Message: 'a>(tiles: Vec<Tile<'a>>) -> Element<'a, Message> {
    responsive(move |size: Size| {
        let (columns, rows) = grid_shape(tiles.len(), size.width >= size.height);
        let mut cells = tiles.iter().copied().map(tile_view);

        column((0..rows).map(|_| {
            row((0..columns).map(|_| {
                cells
                    .next()
                    .unwrap_or_else(|| space().width(Length::Fill).height(Length::Fill).into())
            }))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    })
    .into()
}

fn tile_view<'a, Message: 'a>(tile: Tile<'a>) -> Element<'a, Message> {
    let body: Element<'a, Message> = match tile.frame {
        Some(frame) => shader(VideoSink::new(tile.identity, frame.clone()))
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        None => container(text(tile.label).size(20))
            .center(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.15, 0.15, 0.15))),
                border: border::rounded(8),
                ..container::Style::default()
            })
            .into(),
    };

    let name = if tile.is_local {
        format!("{} (you)", tile.label)
    } else {
        tile.label.to_owned()
    };

    let name_plate = container(
        text(name)
            .size(14)
            .wrapping(text::Wrapping::None)
            .color(Color::WHITE),
    )
    .padding([4, 8])
    .style(|_| container::Style {
        background: Some(Background::Color(Color::BLACK.scale_alpha(0.5))),
        border: border::rounded(6),
        ..container::Style::default()
    });

    // Clipped so a long name in a small cell cannot push the grid around.
    let name_plate = container(name_plate)
        .align_left(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(8)
        .clip(true);

    container(stack![body, name_plate])
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(4)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_grows_in_powers_of_two() {
        let landscape = [
            (0, (1, 1)),
            (1, (1, 1)),
            (2, (2, 1)),
            (3, (2, 2)),
            (4, (2, 2)),
            (5, (4, 2)),
            (8, (4, 2)),
            (9, (4, 4)),
            (16, (4, 4)),
            (17, (8, 4)),
            (32, (8, 4)),
            (33, (8, 4)),
        ];

        for (count, shape) in landscape {
            assert_eq!(grid_shape(count, true), shape, "landscape, {count} tiles");

            let (columns, rows) = shape;
            assert_eq!(
                grid_shape(count, false),
                (rows, columns),
                "portrait, {count} tiles"
            );
        }
    }

    #[test]
    fn every_count_fits_its_grid() {
        for count in 0..=MAX_TILES {
            let (columns, rows) = grid_shape(count, true);
            assert!(columns * rows >= count.max(1), "{count} tiles");
            assert!(columns * rows <= MAX_TILES, "{count} tiles");
        }
    }

    #[test]
    fn tiles_sort_by_name_then_identity_and_include_local() {
        let local = ("id-local".to_owned(), "bob".to_owned());
        let roster: Roster = [
            ("id-2".to_owned(), "alice".to_owned()),
            ("id-1".to_owned(), "Alice".to_owned()),
            ("id-3".to_owned(), "Zed".to_owned()),
        ]
        .into_iter()
        .collect();
        let frames = HashMap::new();

        let tiles = ordered_tiles(Some(&local), &roster, &frames);
        let identities: Vec<&str> = tiles.iter().map(|tile| tile.identity).collect();

        assert_eq!(identities, ["id-1", "id-2", "id-local", "id-3"]);
        assert_eq!(tiles.iter().filter(|tile| tile.is_local).count(), 1);
    }

    #[test]
    fn tiles_are_capped_after_sorting() {
        let roster: Roster = (0..40)
            .map(|n| (format!("id-{n:02}"), format!("name-{n:02}")))
            .collect();
        let frames = HashMap::new();

        let tiles = ordered_tiles(None, &roster, &frames);

        assert_eq!(tiles.len(), MAX_TILES);
        assert_eq!(tiles.first().map(|tile| tile.identity), Some("id-00"));
        assert_eq!(tiles.last().map(|tile| tile.identity), Some("id-31"));
    }
}
