//! The icons GitHub shows on its alerts, drawn as pictures: reading mode's
//! text has no symbols font to take them from.
//!
//! The Octicons are GitHub's, under the MIT licence:
//!
//! > Copyright (c) 2023 GitHub Inc.
//! >
//! > Permission is hereby granted, free of charge, to any person obtaining
//! > a copy of this software and associated documentation files (the
//! > "Software"), to deal in the Software without restriction, including
//! > without limitation the rights to use, copy, modify, merge, publish,
//! > distribute, sublicense, and/or sell copies of the Software, and to
//! > permit persons to whom the Software is furnished to do so, subject to
//! > the following conditions:
//! >
//! > The above copyright notice and this permission notice shall be
//! > included in all copies or substantial portions of the Software.
//! >
//! > THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
//! > EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//! > MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
//! > NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
//! > LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
//! > OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
//! > WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//!
//! The outlines below are those of the `info`, `light-bulb`, `report`,
//! `alert` and `stop` icons (16 pixels) as the symbols font that kitty
//! bundles holds them, in that font's units (2048 to the icon).

use image::RgbaImage;
use resvg::{tiny_skia, usvg};

use crate::model::AlertKind;

use super::text::Brush;

/// The outline of the icon of `kind`, in a box of 2048 units from `-1638`
/// down.
fn outline(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::Note => {
            "M81 -216.5Q0 -406 0 -614Q0 -822 81 -1011.5Q162 -1201 299.5 -1338.5Q437 -1476 626.5 -1557Q816 -1638 1024 -1638Q1232 -1638 1421.5 -1557Q1611 -1476 1748.5 -1338.5Q1886 -1201 1967 -1011.5Q2048 -822 2048 -614Q2048 -406 1967 -216.5Q1886 -27 1748.5 110.5Q1611 248 1421.5 329Q1232 410 1024 410Q816 410 626.5 329Q437 248 299.5 110.5Q162 -27 81 -216.5ZM1347 -1380Q1193 -1446 1024 -1446Q855 -1446 701 -1380Q547 -1314 435.5 -1202.5Q324 -1091 258 -937Q192 -783 192 -614Q192 -445 258 -291Q324 -137 435.5 -25.5Q547 86 701 152Q855 218 1024 218Q1193 218 1347 152Q1501 86 1612.5 -25.5Q1724 -137 1790 -291Q1856 -445 1856 -614Q1856 -783 1790 -937Q1724 -1091 1612.5 -1202.5Q1501 -1314 1347 -1380ZM860 -578Q832 -606 832 -646Q832 -686 860 -714Q888 -742 928 -742L1056 -742Q1096 -742 1124 -714Q1152 -686 1152 -646L1152 -294L1184 -294Q1224 -294 1252 -266Q1280 -238 1280 -198Q1280 -158 1252 -130Q1224 -102 1184 -102L928 -102Q888 -102 860 -130Q832 -158 832 -198Q832 -238 860 -266Q888 -294 928 -294L960 -294L960 -550L928 -550Q888 -550 860 -578ZM1114.5 -907.5Q1077 -870 1024 -870Q971 -870 933.5 -907.5Q896 -945 896 -998Q896 -1051 933.5 -1088.5Q971 -1126 1024 -1126Q1077 -1126 1114.5 -1088.5Q1152 -1051 1152 -998Q1152 -945 1114.5 -907.5Z"
        }
        AlertKind::Tip => {
            "M1391.5 -1306.5Q1247 -1446 1024 -1446Q801 -1446 656.5 -1306.5Q512 -1167 512 -966Q512 -879 543.5 -813Q575 -747 638 -671L665 -639Q726 -567 751 -530Q818 -435 831 -339Q831 -339 831 -329Q831 -290 803 -261.5Q775 -233 735 -233Q700 -233 673.5 -256Q647 -279 641 -312Q634 -363 594 -421Q561 -466 524 -508Q496 -542 490 -549Q454 -593 428.5 -629.5Q403 -666 376 -719Q349 -772 334.5 -834.5Q320 -897 320 -966Q320 -1105 374 -1229.5Q428 -1354 521 -1443.5Q614 -1533 745 -1585.5Q876 -1638 1024 -1638Q1172 -1638 1303 -1585.5Q1434 -1533 1527 -1443.5Q1620 -1354 1674 -1229.5Q1728 -1105 1728 -966Q1728 -897 1713.5 -834.5Q1699 -772 1672 -719Q1645 -666 1619.5 -629.5Q1594 -593 1558 -549Q1545 -534 1524 -508Q1475 -450 1454 -421Q1414 -363 1407 -312Q1401 -279 1374.5 -257Q1348 -235 1313 -235Q1273 -235 1245 -263Q1217 -291 1217 -331Q1217 -331 1217 -340Q1230 -435 1297 -530Q1322 -567 1383 -639Q1400 -659 1410 -671Q1473 -747 1504.5 -813Q1536 -879 1536 -966Q1536 -1167 1391.5 -1306.5ZM736 -102L1312 -102Q1352 -102 1380 -74Q1408 -46 1408 -6Q1408 34 1380 62Q1352 90 1312 90L736 90Q696 90 668 62Q640 34 640 -6Q640 -46 668 -74Q696 -102 736 -102ZM796 382Q768 354 768 314Q768 274 796 246Q824 218 864 218L1184 218Q1224 218 1252 246Q1280 274 1280 314Q1280 354 1252 382Q1224 410 1184 410L864 410Q824 410 796 382Z"
        }
        AlertKind::Important => {
            "M1 -1414Q1 -1507 66.5 -1572.5Q132 -1638 225 -1638L1825 -1638Q1918 -1638 1983.5 -1572.5Q2049 -1507 2049 -1414L2049 -198Q2049 -105 1983.5 -39.5Q1918 26 1825 26L1033 26L703 355Q649 410 572 410Q495 410 440 355.5Q385 301 385 224Q385 224 385 174L385 75L385 26L225 26Q132 26 66.5 -39.5Q1 -105 1 -198ZM225 -1446Q212 -1446 202.5 -1436.5Q193 -1427 193 -1414L193 -198Q193 -185 202.5 -175.5Q212 -166 225 -166L481 -166Q521 -166 549 -138Q577 -110 577 -70L577 210L925 -138Q953 -166 993 -166L1201 -166Q1201 -166 1617 -166L1825 -166Q1838 -166 1847.5 -175.5Q1857 -185 1857 -198L1857 -1414Q1857 -1427 1847.5 -1436.5Q1838 -1446 1825 -1446ZM1121 -1158L1121 -838Q1121 -798 1093 -770Q1065 -742 1025 -742Q985 -742 957 -770Q929 -798 929 -838L929 -1158Q929 -1198 957 -1226Q985 -1254 1025 -1254Q1065 -1254 1093 -1226Q1121 -1198 1121 -1158ZM1115.5 -576.5Q1153 -539 1153 -486Q1153 -433 1115.5 -395.5Q1078 -358 1025 -358Q972 -358 934.5 -395.5Q897 -433 897 -486Q897 -539 934.5 -576.5Q972 -614 1025 -614Q1078 -614 1115.5 -576.5Z"
        }
        AlertKind::Warning => {
            "M822 -1466Q847 -1513 888.5 -1544Q930 -1575 977 -1583Q1024 -1591 1071 -1583Q1118 -1575 1159.5 -1544Q1201 -1513 1226 -1466L2021 22Q2048 73 2048 130Q2048 225 1981 292Q1914 359 1819 359L229 359Q134 359 67 292Q0 225 0 130Q0 73 27 22Q27 23 225.5 -349.5Q424 -722 623 -1094ZM1053 -1373Q1043 -1391 1024 -1391Q1005 -1391 995 -1373L199 115Q196 122 196 130Q196 143 205.5 153Q215 163 229 163L1819 163Q1833 163 1842.5 153Q1852 143 1852 130Q1852 122 1849 115Q1849 115 1649 -257Q1450 -630 1252 -1001ZM1122 -851L1122 -524Q1122 -483 1093.5 -454.5Q1065 -426 1024 -426Q983 -426 954.5 -454.5Q926 -483 926 -524L926 -851Q926 -892 954.5 -920.5Q983 -949 1024 -949Q1065 -949 1093.5 -920.5Q1122 -892 1122 -851ZM1116.5 -257Q1155 -219 1155 -164.5Q1155 -110 1116.5 -72Q1078 -34 1024 -34Q970 -34 931.5 -72Q893 -110 893 -164.5Q893 -219 931.5 -257Q970 -295 1024 -295Q1078 -295 1116.5 -257Z"
        }
        AlertKind::Caution => {
            "M573 -1610Q601 -1638 641 -1638L833 -1638Q833 -1638 1217 -1638L1409 -1638Q1449 -1638 1477 -1610L2021 -1066Q2049 -1038 2049 -998L2049 -230Q2049 -190 2021 -162L1477 382Q1449 410 1409 410L1217 410Q1217 410 833 410L641 410Q601 410 573 382L29 -162Q1 -190 1 -230L1 -422Q1 -422 1 -806L1 -998Q1 -1038 29 -1066ZM681 -1446L193 -958L193 -270L681 218L1369 218L1857 -270L1857 -958L1369 -1446ZM957 -1098Q985 -1126 1025 -1126Q1065 -1126 1093 -1098Q1121 -1070 1121 -1030L1121 -582Q1121 -542 1093 -514Q1065 -486 1025 -486Q985 -486 957 -514Q929 -542 929 -582L929 -1030Q929 -1070 957 -1098ZM1115.5 -139.5Q1078 -102 1025 -102Q972 -102 934.5 -139.5Q897 -177 897 -230Q897 -283 934.5 -320.5Q972 -358 1025 -358Q1078 -358 1115.5 -320.5Q1153 -283 1153 -230Q1153 -177 1115.5 -139.5Z"
        }
    }
}

/// The icon of `kind` in `color`, `size` pixels wide and high.
pub fn icon(kind: AlertKind, size: u32, color: Brush) -> RgbaImage {
    let [red, green, blue] = color;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 -1638 2048 2048\">\
         <path fill=\"#{red:02x}{green:02x}{blue:02x}\" d=\"{}\"/></svg>",
        outline(kind)
    );
    let mut image = RgbaImage::new(size, size);
    let Ok(tree) = usvg::Tree::from_str(&svg, &usvg::Options::default()) else {
        return image;
    };
    let Some(mut pixmap) = tiny_skia::Pixmap::new(size, size) else {
        return image;
    };
    let scale = size as f32 / 2048.0;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // The renderer works in premultiplied alpha, the picture is not.
    for (pixel, drawn) in image.pixels_mut().zip(pixmap.pixels()) {
        let colour = drawn.demultiply();
        pixel.0 = [colour.red(), colour.green(), colour.blue(), colour.alpha()];
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many pixels of `image` have any colour.
    fn inked(image: &RgbaImage) -> usize {
        image.pixels().filter(|pixel| pixel.0[3] > 0).count()
    }

    #[test]
    fn each_kind_has_an_icon_of_its_own_in_the_colour_asked_for() {
        let kinds = [
            AlertKind::Note,
            AlertKind::Tip,
            AlertKind::Important,
            AlertKind::Warning,
            AlertKind::Caution,
        ];
        let icons: Vec<RgbaImage> = (kinds.iter())
            .map(|&kind| icon(kind, 16, [0x12, 0x34, 0x56]))
            .collect();
        for (kind, image) in kinds.iter().zip(&icons) {
            assert_eq!(image.dimensions(), (16, 16), "{kind:?}");
            // Something is drawn, not all of the square, and always in the
            // colour, with only its opacity varying.
            let ink = inked(image);
            assert!(ink > 30 && ink < 16 * 16, "{kind:?}: {ink} pixels");
            assert!(
                image
                    .pixels()
                    .filter(|pixel| pixel.0[3] == 255)
                    .all(|pixel| pixel.0[..3] == [0x12, 0x34, 0x56]),
                "{kind:?}"
            );
        }
        for (a, b) in [(0, 1), (0, 2), (1, 2), (2, 3), (3, 4)] {
            assert_ne!(icons[a], icons[b], "{a} and {b}");
        }
    }

    #[test]
    fn an_icon_is_drawn_at_the_size_asked_for() {
        let large = icon(AlertKind::Note, 32, [255; 3]);
        assert_eq!(large.dimensions(), (32, 32));
        assert!(inked(&large) > 2 * inked(&icon(AlertKind::Note, 16, [255; 3])));
    }
}
