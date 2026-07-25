use deckweaver_core::svg_data_to_png_bytes;
use fontawesome_free_pack::get_icon;

pub fn fa_icon_to_png(slug: &str) -> Option<Vec<u8>> {
    let icon = get_icon(slug)?;
    let white_svg = icon.svg.replace("currentColor", "#ffffff");
    svg_data_to_png_bytes(white_svg.as_bytes())
}
