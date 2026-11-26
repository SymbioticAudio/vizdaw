//! The strip colours by name (Swift `StripPalette`'s swatch list and
//! titles). The colours themselves are theme tokens, drawn by the UI.

use vizdaw_ffi::mixer::SwatchRow;

/// The strip colour names and their order.
pub struct StripPalette;

impl StripPalette {
    /// The eight strip colours, in the Colour menu's order.
    pub const SWATCHES: [SwatchRow; 8] = [
        SwatchRow::Red,
        SwatchRow::Orange,
        SwatchRow::Yellow,
        SwatchRow::Green,
        SwatchRow::Teal,
        SwatchRow::Blue,
        SwatchRow::Purple,
        SwatchRow::Pink,
    ];

    /// The Colour menu's first item: no colour of its own (the kind's).
    pub const NO_COLOUR_TITLE: &'static str = "None";

    /// A swatch's name in the Colour menu.
    pub fn title(c: SwatchRow) -> &'static str {
        match c {
            SwatchRow::Red => "Red",
            SwatchRow::Orange => "Orange",
            SwatchRow::Yellow => "Yellow",
            SwatchRow::Green => "Green",
            SwatchRow::Teal => "Teal",
            SwatchRow::Blue => "Blue",
            SwatchRow::Purple => "Purple",
            SwatchRow::Pink => "Pink",
        }
    }

    /// The swatch a channel call names ("red" … "pink", the title in
    /// lower case); `None` for any other word, `none` included.
    pub fn named(name: &str) -> Option<SwatchRow> {
        Self::SWATCHES
            .into_iter()
            .find(|c| Self::title(*c).to_lowercase() == name)
    }
}
