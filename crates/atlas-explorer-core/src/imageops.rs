//! Quick actions on pictures (More Actions): which files take them, what the
//! new files are called, and how a JPEG can be turned without being decoded.
//!
//! The pictures themselves are read and written by Qt on a worker (the app's
//! `ImageWork`); what is decided here has no Qt in it: the kinds of file, the
//! names (the original is never touched: every action makes a new file), the
//! limits that keep a hostile picture from using up the machine, and the
//! lossless JPEG turn (EXIF orientation read, written down as upright, and the
//! transform that does what the user asked on what is shown).

/// Most files one action takes.
pub const MAX_ITEMS: usize = 500;
/// The largest file one action reads.
pub const MAX_INPUT_BYTES: u64 = 256 << 20;
/// The most pixels of one picture (a 12000 x 10000 photo is 120 million).
pub const MAX_PIXELS: u64 = 150_000_000;
/// The longest file name the file systems of Linux take.
const MAX_NAME_BYTES: usize = 255;

/// What a file is, for these actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Jpeg,
    Png,
    Webp,
    Bmp,
    /// A picture Qt reads with a plug-in that may not be there (gif, tiff,
    /// avif, jpeg 2000): the app asks Qt before it offers an action.
    OtherRaster,
    Pdf,
}

/// The kind of a file from its MIME type; `None` when no action applies
/// (vector pictures such as SVG are not turned into pixels here).
pub fn kind_of_mime(mime: &str) -> Option<Kind> {
    Some(match mime {
        "image/jpeg" | "image/jpg" | "image/pjpeg" => Kind::Jpeg,
        "image/png" => Kind::Png,
        "image/webp" => Kind::Webp,
        "image/bmp" | "image/x-ms-bmp" | "image/x-bmp" => Kind::Bmp,
        "image/gif" | "image/tiff" | "image/avif" | "image/jp2" | "image/jpeg2000" => {
            Kind::OtherRaster
        }
        "application/pdf" => Kind::Pdf,
        _ => return None,
    })
}

/// What the app does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    RotateLeft,
    RotateRight,
    ToPng,
    ToJpeg,
    ToWebp,
    CombinePdf,
}

impl Action {
    /// The number the app uses for it.
    pub fn from_code(code: u32) -> Option<Action> {
        Some(match code {
            0 => Action::RotateLeft,
            1 => Action::RotateRight,
            2 => Action::ToPng,
            3 => Action::ToJpeg,
            4 => Action::ToWebp,
            5 => Action::CombinePdf,
            _ => return None,
        })
    }

    /// Whether `kind` can be the input of this action.
    pub fn accepts(self, kind: Kind) -> bool {
        match self {
            Action::CombinePdf => true,
            _ => kind != Kind::Pdf,
        }
    }

    /// Whether the action leaves a file of this kind as it is (a picture that
    /// already is a PNG is not converted to PNG).
    pub fn skips(self, kind: Kind) -> bool {
        matches!(
            (self, kind),
            (Action::ToPng, Kind::Png)
                | (Action::ToJpeg, Kind::Jpeg)
                | (Action::ToWebp, Kind::Webp)
        )
    }
}

/// The format a turned picture is written in: its own where Qt can write it,
/// else PNG (nothing is lost by it).
pub fn rotate_output_ext(kind: Kind) -> &'static str {
    match kind {
        Kind::Jpeg => "jpg",
        Kind::Webp => "webp",
        Kind::Bmp => "bmp",
        _ => "png",
    }
}

/// The extension of the file an action writes for a picture of `kind`.
fn output_ext(action: Action, kind: Kind) -> &'static str {
    match action {
        Action::RotateLeft | Action::RotateRight => rotate_output_ext(kind),
        Action::ToPng => "png",
        Action::ToJpeg => "jpg",
        Action::ToWebp => "webp",
        Action::CombinePdf => "pdf",
    }
}

/// Splits `name` into the part before the last dot and the dot's extension.
/// A leading dot is part of the stem (".png" has no extension).
fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    }
}

/// `stem` and `ext` joined, the stem cut at a character boundary so that the
/// whole name fits a file system.
fn fit(stem: &str, suffix: &str, ext: &str) -> String {
    let tail = if ext.is_empty() {
        suffix.to_string()
    } else {
        format!("{suffix}.{ext}")
    };
    let room = MAX_NAME_BYTES.saturating_sub(tail.len());
    let mut end = stem.len().min(room);
    while end > 0 && !stem.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &stem[..end], tail)
}

/// The name of the file an action makes from `name`, before two files of one
/// batch are told apart. A turned picture is "photo (rotated).jpg"; a
/// conversion changes the extension ("photo.png"), and when that is the
/// file's own name, "photo (converted).png"; a PDF is "Combined.pdf".
pub fn output_name(action: Action, kind: Kind, name: &str) -> String {
    let (stem, ext) = split_name(name);
    let new_ext = output_ext(action, kind);
    match action {
        Action::RotateLeft | Action::RotateRight => fit(stem, " (rotated)", new_ext),
        Action::ToPng | Action::ToJpeg | Action::ToWebp => {
            let plain = fit(stem, "", new_ext);
            // jpg and jpeg are one format: "photo.jpeg" to JPEG would be itself.
            let same = plain == name
                || (new_ext == "jpg"
                    && ext.eq_ignore_ascii_case("jpeg")
                    && stem == &plain[..plain.len() - 4]);
            if same {
                fit(stem, " (converted)", new_ext)
            } else {
                plain
            }
        }
        Action::CombinePdf => "Combined.pdf".to_string(),
    }
}

/// Names for a batch: each is made as `output_name` makes it, and a name that
/// an earlier file of the batch has is told apart with " (2)", " (3)" ...
/// before the extension. `inputs` are (kind, file name) in the order they are
/// worked on.
pub fn output_names(action: Action, inputs: &[(Kind, String)]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(inputs.len());
    for (kind, name) in inputs {
        let first = output_name(action, *kind, name);
        let mut candidate = first.clone();
        let mut n = 2u32;
        while !seen.insert(candidate.clone()) {
            let (stem, ext) = split_name(&first);
            candidate = fit(stem, &format!(" ({n})"), ext);
            n += 1;
        }
        out.push(candidate);
    }
    out
}

/// Whether a file of `size` bytes and `pixels` pixels (0 when not known yet)
/// may be read. The reason is plain words.
pub fn check_input(size: u64, pixels: u64) -> Result<(), String> {
    if size > MAX_INPUT_BYTES {
        return Err("it is bigger than 256 MB".to_string());
    }
    if pixels > MAX_PIXELS {
        return Err("it has more than 150 million pixels".to_string());
    }
    Ok(())
}

// ---- Turning a JPEG without decoding it ----

/// A turn or flip of a picture, with the numbers libjpeg-turbo gives them
/// (`TJXOP_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    None = 0,
    HFlip = 1,
    VFlip = 2,
    Transpose = 3,
    Transverse = 4,
    Rot90 = 5,
    Rot180 = 6,
    Rot270 = 7,
}

impl Transform {
    /// Where a point (x, y) goes, as a 2 x 2 matrix `[[a, b], [c, d]]` on
    /// coordinates with y pointing down: (x, y) -> (a x + b y, c x + d y).
    fn matrix(self) -> [[i8; 2]; 2] {
        match self {
            Transform::None => [[1, 0], [0, 1]],
            Transform::HFlip => [[-1, 0], [0, 1]],
            Transform::VFlip => [[1, 0], [0, -1]],
            Transform::Transpose => [[0, 1], [1, 0]],
            Transform::Transverse => [[0, -1], [-1, 0]],
            // Clockwise: the right edge goes to the bottom.
            Transform::Rot90 => [[0, -1], [1, 0]],
            Transform::Rot180 => [[-1, 0], [0, -1]],
            Transform::Rot270 => [[0, 1], [-1, 0]],
        }
    }

    const ALL: [Transform; 8] = [
        Transform::None,
        Transform::HFlip,
        Transform::VFlip,
        Transform::Transpose,
        Transform::Transverse,
        Transform::Rot90,
        Transform::Rot180,
        Transform::Rot270,
    ];

    /// `self` after `first`.
    fn after(self, first: Transform) -> Transform {
        let (a, b) = (self.matrix(), first.matrix());
        let mut m = [[0i8; 2]; 2];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = a[r][0] * b[0][c] + a[r][1] * b[1][c];
            }
        }
        Transform::ALL
            .into_iter()
            .find(|t| t.matrix() == m)
            .unwrap_or(Transform::None)
    }

    pub fn code(self) -> u32 {
        self as u32
    }
}

/// What an EXIF orientation (1 to 8) says is to be done to the stored pixels
/// to show the picture upright.
pub fn orientation_transform(orientation: u8) -> Transform {
    match orientation {
        2 => Transform::HFlip,
        3 => Transform::Rot180,
        4 => Transform::VFlip,
        5 => Transform::Transpose,
        6 => Transform::Rot90,
        7 => Transform::Transverse,
        8 => Transform::Rot270,
        _ => Transform::None,
    }
}

/// The transform of the stored pixels that turns the picture *as it is shown*
/// a quarter turn, and leaves it upright (orientation 1) afterwards.
pub fn jpeg_turn(orientation: u8, clockwise: bool) -> Transform {
    let turn = if clockwise {
        Transform::Rot90
    } else {
        Transform::Rot270
    };
    turn.after(orientation_transform(orientation))
}

/// Where the orientation value of a JPEG's EXIF data is: the offset of its two
/// bytes, and whether they are big-endian. `None` when there is no EXIF block
/// or it has no (readable) orientation.
fn orientation_at(jpeg: &[u8]) -> Option<(usize, bool)> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 4 <= jpeg.len() {
        if jpeg[i] != 0xFF {
            return None;
        }
        let marker = jpeg[i + 1];
        // Fill bytes, then markers without a length.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        // Start of scan or end of image: the headers are over.
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        if len < 2 || i + 2 + len > jpeg.len() {
            return None;
        }
        let body = &jpeg[i + 4..i + 2 + len];
        if marker == 0xE1 && body.len() >= 6 && &body[..6] == b"Exif\0\0" {
            let base = i + 4 + 6;
            let tiff = &jpeg[base..i + 2 + len];
            return orientation_in_tiff(tiff).map(|(off, big)| (base + off, big));
        }
        i += 2 + len;
    }
    None
}

fn orientation_in_tiff(tiff: &[u8]) -> Option<(usize, bool)> {
    if tiff.len() < 8 {
        return None;
    }
    let big = match &tiff[..2] {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let b = tiff.get(o..o + 2)?;
        Some(if big {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let b = tiff.get(o..o + 4)?;
        Some(if big {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    if u16_at(2)? != 42 {
        return None;
    }
    let ifd = u32_at(4)? as usize;
    let count = u16_at(ifd)? as usize;
    for n in 0..count.min(512) {
        let entry = ifd.checked_add(2)?.checked_add(n.checked_mul(12)?)?;
        if u16_at(entry)? == 0x0112 {
            // A SHORT (3), one value, kept in the entry itself.
            if u16_at(entry + 2)? != 3 || u32_at(entry + 4)? != 1 {
                return None;
            }
            tiff.get(entry + 8..entry + 10)?;
            return Some((entry + 8, big));
        }
    }
    None
}

/// The EXIF orientation of a JPEG, 1 to 8; 1 when it has none or a value that
/// means nothing.
pub fn exif_orientation(jpeg: &[u8]) -> u8 {
    match orientation_at(jpeg) {
        Some((at, big)) => {
            let v = if big {
                u16::from_be_bytes([jpeg[at], jpeg[at + 1]])
            } else {
                u16::from_le_bytes([jpeg[at], jpeg[at + 1]])
            };
            if (1..=8).contains(&v) { v as u8 } else { 1 }
        }
        None => 1,
    }
}

/// Writes orientation 1 (upright) into the EXIF data of a JPEG, in place.
/// Returns whether there was an orientation to write.
pub fn reset_exif_orientation(jpeg: &mut [u8]) -> bool {
    match orientation_at(jpeg) {
        Some((at, big)) => {
            let one = if big {
                1u16.to_be_bytes()
            } else {
                1u16.to_le_bytes()
            };
            jpeg[at] = one[0];
            jpeg[at + 1] = one[1];
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture as rows of labels, to see what a transform does.
    fn apply(t: Transform, g: &[Vec<char>]) -> Vec<Vec<char>> {
        let h = g.len();
        let w = g[0].len();
        let rot90 = |g: &[Vec<char>]| -> Vec<Vec<char>> {
            let (h, w) = (g.len(), g[0].len());
            (0..w)
                .map(|r| (0..h).map(|c| g[h - 1 - c][r]).collect())
                .collect()
        };
        match t {
            Transform::None => g.to_vec(),
            Transform::HFlip => g
                .iter()
                .map(|r| r.iter().rev().cloned().collect())
                .collect(),
            Transform::VFlip => g.iter().rev().cloned().collect(),
            Transform::Transpose => (0..w).map(|c| (0..h).map(|r| g[r][c]).collect()).collect(),
            Transform::Transverse => (0..w)
                .map(|c| (0..h).map(|r| g[h - 1 - r][w - 1 - c]).collect())
                .collect(),
            Transform::Rot90 => rot90(g),
            Transform::Rot180 => rot90(&rot90(g)),
            Transform::Rot270 => rot90(&rot90(&rot90(g))),
        }
    }

    fn grid() -> Vec<Vec<char>> {
        vec![vec!['a', 'b', 'c'], vec!['d', 'e', 'f']]
    }

    #[test]
    fn matrices_do_what_the_pixels_do() {
        // The definitions agree with literal pictures (y down, clockwise).
        let g = grid();
        assert_eq!(
            apply(Transform::Rot90, &g),
            vec![vec!['d', 'a'], vec!['e', 'b'], vec!['f', 'c']]
        );
        assert_eq!(
            apply(Transform::Transpose, &g),
            vec![vec!['a', 'd'], vec!['b', 'e'], vec!['c', 'f']]
        );
        assert_eq!(
            apply(Transform::Rot180, &g),
            vec![vec!['f', 'e', 'd'], vec!['c', 'b', 'a']]
        );
        // Composition by matrices equals composition of the pictures, for all 64 pairs.
        for first in Transform::ALL {
            for then in Transform::ALL {
                let by_matrix = then.after(first);
                assert_eq!(
                    apply(by_matrix, &g),
                    apply(then, &apply(first, &g)),
                    "{then:?} after {first:?} is {by_matrix:?}"
                );
            }
        }
    }

    #[test]
    fn a_turn_of_what_is_shown_accounts_for_the_orientation() {
        // Upright: a plain quarter turn.
        assert_eq!(jpeg_turn(1, true), Transform::Rot90);
        assert_eq!(jpeg_turn(1, false), Transform::Rot270);
        // Stored sideways (6: shown by turning it clockwise): turning the
        // shown picture clockwise is half a turn of the pixels, the other way
        // leaves them as they are.
        assert_eq!(jpeg_turn(6, true), Transform::Rot180);
        assert_eq!(jpeg_turn(6, false), Transform::None);
        assert_eq!(jpeg_turn(8, true), Transform::None);
        assert_eq!(jpeg_turn(8, false), Transform::Rot180);
        assert_eq!(jpeg_turn(3, true), Transform::Rot270);
        // Mirrored ones end in a flip or a transpose.
        assert_eq!(jpeg_turn(2, true), Transform::Transverse);
        // Whatever the orientation: shown picture of the result == shown
        // picture of the original, turned.
        let g = grid();
        for o in 1..=8u8 {
            for clockwise in [true, false] {
                let shown = apply(orientation_transform(o), &g);
                let wanted = apply(
                    if clockwise {
                        Transform::Rot90
                    } else {
                        Transform::Rot270
                    },
                    &shown,
                );
                assert_eq!(
                    apply(jpeg_turn(o, clockwise), &g),
                    wanted,
                    "orientation {o}, clockwise {clockwise}"
                );
            }
        }
    }

    /// A JPEG header with an EXIF block holding one orientation entry.
    fn jpeg_with(orientation: u16, big: bool) -> Vec<u8> {
        let mut tiff = Vec::new();
        let w16 = |v: u16| {
            if big {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let w32 = |v: u32| {
            if big {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        tiff.extend_from_slice(if big { b"MM" } else { b"II" });
        tiff.extend_from_slice(&w16(42));
        tiff.extend_from_slice(&w32(8));
        tiff.extend_from_slice(&w16(2));
        // Another entry first (Make, ASCII), then the orientation.
        tiff.extend_from_slice(&w16(0x010F));
        tiff.extend_from_slice(&w16(2));
        tiff.extend_from_slice(&w32(2));
        tiff.extend_from_slice(b"X\0\0\0");
        tiff.extend_from_slice(&w16(0x0112));
        tiff.extend_from_slice(&w16(3));
        tiff.extend_from_slice(&w32(1));
        tiff.extend_from_slice(&w16(orientation));
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&w32(0));
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
        out
    }

    #[test]
    fn orientation_is_read_and_reset_in_both_byte_orders() {
        for big in [false, true] {
            for o in 1..=8u16 {
                let mut j = jpeg_with(o, big);
                assert_eq!(exif_orientation(&j), o as u8);
                assert!(reset_exif_orientation(&mut j));
                assert_eq!(exif_orientation(&j), 1);
            }
        }
        // A value that means nothing is upright.
        assert_eq!(exif_orientation(&jpeg_with(9, false)), 1);
        assert_eq!(exif_orientation(&jpeg_with(0, true)), 1);
    }

    #[test]
    fn odd_jpeg_data_is_never_a_crash() {
        assert_eq!(exif_orientation(&[]), 1);
        assert_eq!(exif_orientation(&[0xFF, 0xD8]), 1);
        assert_eq!(exif_orientation(b"not a jpeg at all"), 1);
        let good = jpeg_with(6, false);
        // Cut anywhere, and with each byte broken: never a panic, never a wrong write.
        for n in 0..good.len() {
            let mut cut = good[..n].to_vec();
            let _ = exif_orientation(&cut);
            let _ = reset_exif_orientation(&mut cut);
        }
        for i in 0..good.len() {
            let mut bad = good.clone();
            bad[i] ^= 0xFF;
            let _ = exif_orientation(&bad);
            let mut again = bad.clone();
            let _ = reset_exif_orientation(&mut again);
            assert_eq!(bad.len(), again.len());
        }
        // An IFD offset that points past the end.
        let mut j = jpeg_with(6, false);
        let at = j.windows(6).position(|w| w == b"Exif\0\0").unwrap() + 6 + 4;
        j[at..at + 4].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
        assert_eq!(exif_orientation(&j), 1);
    }

    #[test]
    fn kinds_come_from_the_mime_type() {
        assert_eq!(kind_of_mime("image/jpeg"), Some(Kind::Jpeg));
        assert_eq!(kind_of_mime("image/png"), Some(Kind::Png));
        assert_eq!(kind_of_mime("application/pdf"), Some(Kind::Pdf));
        assert_eq!(kind_of_mime("image/svg+xml"), None);
        assert_eq!(kind_of_mime("text/plain"), None);
        assert_eq!(kind_of_mime(""), None);
        assert!(!Action::RotateLeft.accepts(Kind::Pdf));
        assert!(Action::CombinePdf.accepts(Kind::Pdf));
        assert!(Action::CombinePdf.accepts(Kind::Png));
        assert!(Action::ToPng.skips(Kind::Png));
        assert!(!Action::ToPng.skips(Kind::Jpeg));
    }

    #[test]
    fn new_files_get_new_names() {
        use Action::*;
        assert_eq!(
            output_name(RotateLeft, Kind::Jpeg, "photo.jpg"),
            "photo (rotated).jpg"
        );
        assert_eq!(
            output_name(RotateRight, Kind::Jpeg, "photo.JPEG"),
            "photo (rotated).jpg"
        );
        assert_eq!(
            output_name(RotateRight, Kind::Png, "a.b.png"),
            "a.b (rotated).png"
        );
        assert_eq!(
            output_name(RotateRight, Kind::OtherRaster, "scan.tiff"),
            "scan (rotated).png"
        );
        assert_eq!(
            output_name(RotateRight, Kind::Webp, "w.webp"),
            "w (rotated).webp"
        );
        assert_eq!(output_name(ToPng, Kind::Jpeg, "photo.jpg"), "photo.png");
        assert_eq!(output_name(ToJpeg, Kind::Png, "photo.png"), "photo.jpg");
        assert_eq!(output_name(ToWebp, Kind::Png, "photo"), "photo.webp");
        // A name with no extension, and one that is only a dot name.
        assert_eq!(
            output_name(RotateLeft, Kind::Png, "noext"),
            "noext (rotated).png"
        );
        assert_eq!(output_name(ToPng, Kind::Jpeg, ".hidden"), ".hidden.png");
        // The file's own name is never the result.
        assert_eq!(
            output_name(ToJpeg, Kind::Jpeg, "photo.jpg"),
            "photo (converted).jpg"
        );
        assert_eq!(
            output_name(ToJpeg, Kind::Jpeg, "photo.jpeg"),
            "photo (converted).jpg"
        );
        assert_eq!(
            output_name(ToPng, Kind::Png, "photo.png"),
            "photo (converted).png"
        );
        assert_eq!(output_name(CombinePdf, Kind::Png, "x.png"), "Combined.pdf");
        // Never a slash or a longer name than a file system takes.
        let long = format!("{}.jpg", "é".repeat(200));
        let made = output_name(RotateLeft, Kind::Jpeg, &long);
        assert!(made.len() <= 255, "{}", made.len());
        assert!(made.ends_with(" (rotated).jpg"));
    }

    #[test]
    fn a_batch_never_makes_two_files_of_one_name() {
        let inputs = vec![
            (Kind::Jpeg, "a.jpg".to_string()),
            (Kind::Jpeg, "a.jpeg".to_string()),
            (Kind::OtherRaster, "a.tiff".to_string()),
            (Kind::Jpeg, "b.jpg".to_string()),
        ];
        let names = output_names(Action::ToPng, &inputs);
        assert_eq!(names, vec!["a.png", "a (2).png", "a (3).png", "b.png"]);
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
        // Combine makes one file whatever the input.
        assert_eq!(
            output_names(Action::CombinePdf, &inputs[..1]),
            vec!["Combined.pdf"]
        );
    }

    #[test]
    fn input_limits_refuse_what_is_too_big() {
        assert!(check_input(1000, 1_000_000).is_ok());
        assert!(check_input(MAX_INPUT_BYTES, MAX_PIXELS).is_ok());
        assert!(check_input(MAX_INPUT_BYTES + 1, 0).is_err());
        assert!(check_input(0, MAX_PIXELS + 1).is_err());
    }
}
