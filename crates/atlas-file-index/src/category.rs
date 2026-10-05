//! Categories (the `kinds` option) and MIME types, from the file name only: no
//! content sniffing.

/// What the `kinds` option filters on. The discriminant is the bit in a kinds
/// mask and the byte stored in the snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Category {
    Folder = 0,
    Document,
    Spreadsheet,
    Presentation,
    Pdf,
    Image,
    Audio,
    Video,
    Archive,
    Code,
    Text,
    Executable,
    Font,
    DiskImage,
    Other,
}

pub const CATEGORY_COUNT: u8 = 15;

impl Category {
    pub fn from_u8(v: u8) -> Option<Category> {
        use Category::*;
        const ALL: [Category; 15] = [
            Folder,
            Document,
            Spreadsheet,
            Presentation,
            Pdf,
            Image,
            Audio,
            Video,
            Archive,
            Code,
            Text,
            Executable,
            Font,
            DiskImage,
            Other,
        ];
        ALL.get(usize::from(v)).copied()
    }

    /// The name used by the `kinds` option.
    pub fn from_name(name: &str) -> Option<Category> {
        Some(match name {
            "folder" => Category::Folder,
            "document" => Category::Document,
            "spreadsheet" => Category::Spreadsheet,
            "presentation" => Category::Presentation,
            "pdf" => Category::Pdf,
            "image" => Category::Image,
            "audio" => Category::Audio,
            "video" => Category::Video,
            "archive" => Category::Archive,
            "code" => Category::Code,
            "text" => Category::Text,
            "executable" => Category::Executable,
            "font" => Category::Font,
            "disk-image" => Category::DiskImage,
            _ => return None,
        })
    }

    pub fn bit(self) -> u32 {
        1u32 << (self as u8)
    }
}

/// (extension, category, MIME type); extensions are lower case.
const TABLE: &[(&str, Category, &str)] = &[
    ("pdf", Category::Pdf, "application/pdf"),
    ("doc", Category::Document, "application/msword"),
    (
        "docx",
        Category::Document,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    (
        "odt",
        Category::Document,
        "application/vnd.oasis.opendocument.text",
    ),
    ("rtf", Category::Document, "application/rtf"),
    ("epub", Category::Document, "application/epub+zip"),
    ("xls", Category::Spreadsheet, "application/vnd.ms-excel"),
    (
        "xlsx",
        Category::Spreadsheet,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    (
        "ods",
        Category::Spreadsheet,
        "application/vnd.oasis.opendocument.spreadsheet",
    ),
    ("csv", Category::Spreadsheet, "text/csv"),
    (
        "ppt",
        Category::Presentation,
        "application/vnd.ms-powerpoint",
    ),
    (
        "pptx",
        Category::Presentation,
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    (
        "odp",
        Category::Presentation,
        "application/vnd.oasis.opendocument.presentation",
    ),
    ("png", Category::Image, "image/png"),
    ("jpg", Category::Image, "image/jpeg"),
    ("jpeg", Category::Image, "image/jpeg"),
    ("gif", Category::Image, "image/gif"),
    ("webp", Category::Image, "image/webp"),
    ("bmp", Category::Image, "image/bmp"),
    ("svg", Category::Image, "image/svg+xml"),
    ("tif", Category::Image, "image/tiff"),
    ("tiff", Category::Image, "image/tiff"),
    ("heic", Category::Image, "image/heic"),
    ("avif", Category::Image, "image/avif"),
    ("ico", Category::Image, "image/vnd.microsoft.icon"),
    ("mp3", Category::Audio, "audio/mpeg"),
    ("flac", Category::Audio, "audio/flac"),
    ("ogg", Category::Audio, "audio/ogg"),
    ("oga", Category::Audio, "audio/ogg"),
    ("opus", Category::Audio, "audio/opus"),
    ("wav", Category::Audio, "audio/x-wav"),
    ("m4a", Category::Audio, "audio/mp4"),
    ("aac", Category::Audio, "audio/aac"),
    ("mp4", Category::Video, "video/mp4"),
    ("mkv", Category::Video, "video/x-matroska"),
    ("webm", Category::Video, "video/webm"),
    ("avi", Category::Video, "video/x-msvideo"),
    ("mov", Category::Video, "video/quicktime"),
    ("m4v", Category::Video, "video/x-m4v"),
    ("zip", Category::Archive, "application/zip"),
    ("tar", Category::Archive, "application/x-tar"),
    ("gz", Category::Archive, "application/gzip"),
    ("tgz", Category::Archive, "application/gzip"),
    ("bz2", Category::Archive, "application/x-bzip2"),
    ("xz", Category::Archive, "application/x-xz"),
    ("zst", Category::Archive, "application/zstd"),
    ("7z", Category::Archive, "application/x-7z-compressed"),
    ("rar", Category::Archive, "application/vnd.rar"),
    ("rpm", Category::Archive, "application/x-rpm"),
    (
        "deb",
        Category::Archive,
        "application/vnd.debian.binary-package",
    ),
    ("rs", Category::Code, "text/rust"),
    ("c", Category::Code, "text/x-csrc"),
    ("h", Category::Code, "text/x-chdr"),
    ("cpp", Category::Code, "text/x-c++src"),
    ("cc", Category::Code, "text/x-c++src"),
    ("hpp", Category::Code, "text/x-c++hdr"),
    ("py", Category::Code, "text/x-python"),
    ("js", Category::Code, "text/javascript"),
    ("mjs", Category::Code, "text/javascript"),
    ("ts", Category::Code, "text/typescript"),
    ("tsx", Category::Code, "text/tsx"),
    ("jsx", Category::Code, "text/jsx"),
    ("go", Category::Code, "text/x-go"),
    ("java", Category::Code, "text/x-java"),
    ("kt", Category::Code, "text/x-kotlin"),
    ("qml", Category::Code, "text/x-qml"),
    ("sh", Category::Code, "application/x-shellscript"),
    ("html", Category::Code, "text/html"),
    ("htm", Category::Code, "text/html"),
    ("css", Category::Code, "text/css"),
    ("json", Category::Code, "application/json"),
    ("xml", Category::Code, "application/xml"),
    ("toml", Category::Code, "application/toml"),
    ("yaml", Category::Code, "application/yaml"),
    ("yml", Category::Code, "application/yaml"),
    ("txt", Category::Text, "text/plain"),
    ("md", Category::Text, "text/markdown"),
    ("log", Category::Text, "text/x-log"),
    ("ini", Category::Text, "text/plain"),
    ("conf", Category::Text, "text/plain"),
    ("appimage", Category::Executable, "application/vnd.appimage"),
    ("run", Category::Executable, "application/x-executable"),
    (
        "exe",
        Category::Executable,
        "application/vnd.microsoft.portable-executable",
    ),
    ("msi", Category::Executable, "application/x-msi"),
    (
        "flatpakref",
        Category::Executable,
        "application/vnd.flatpak.ref",
    ),
    ("ttf", Category::Font, "font/ttf"),
    ("otf", Category::Font, "font/otf"),
    ("woff", Category::Font, "font/woff"),
    ("woff2", Category::Font, "font/woff2"),
    ("iso", Category::DiskImage, "application/x-iso9660-image"),
    ("img", Category::DiskImage, "application/x-raw-disk-image"),
    ("qcow2", Category::DiskImage, "application/x-qemu-disk"),
    ("vhd", Category::DiskImage, "application/x-vhd"),
    ("vmdk", Category::DiskImage, "application/x-vmdk"),
];

/// The part after the last dot, lower-cased, if the name is ASCII-safe to
/// compare (any bytes work: a non-matching extension simply finds nothing).
fn extension(name: &[u8]) -> Option<String> {
    let dot = memchr::memrchr(b'.', name)?;
    if dot == 0 || dot + 1 >= name.len() || name.len() - dot > 12 {
        return None;
    }
    let ext = &name[dot + 1..];
    if !ext.is_ascii() {
        return None;
    }
    Some(String::from_utf8_lossy(ext).to_ascii_lowercase())
}

/// Category of a file from its name; `exec` is the exec permission bit, which
/// makes an otherwise unknown file an executable.
pub fn category_of(name: &[u8], is_dir: bool, exec: bool) -> Category {
    if is_dir {
        return Category::Folder;
    }
    if let Some(ext) = extension(name)
        && let Some(&(_, c, _)) = TABLE.iter().find(|(e, _, _)| *e == ext)
    {
        return c;
    }
    if exec {
        Category::Executable
    } else {
        Category::Other
    }
}

/// MIME type from the name.
pub fn mime_of(name: &[u8], is_dir: bool) -> &'static str {
    if is_dir {
        return "inode/directory";
    }
    if let Some(ext) = extension(name)
        && let Some(&(_, _, m)) = TABLE.iter().find(|(e, _, _)| *e == ext)
    {
        return m;
    }
    "application/octet-stream"
}

/// freedesktop icon name for a MIME type (`image/png` is `image-png`).
pub fn icon_of(mime: &str, is_dir: bool) -> String {
    if is_dir {
        return "folder".to_string();
    }
    if mime == "application/octet-stream" {
        return "application-x-generic".to_string();
    }
    mime.replace('/', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories() {
        assert_eq!(category_of(b"a.PDF", false, false), Category::Pdf);
        assert_eq!(category_of(b"a.tar.gz", false, false), Category::Archive);
        assert_eq!(category_of(b"Makefile", false, true), Category::Executable);
        assert_eq!(category_of(b"Makefile", false, false), Category::Other);
        assert_eq!(category_of(b".png", false, false), Category::Other);
        assert_eq!(category_of(b"x", true, false), Category::Folder);
        assert_eq!(category_of(b"x.\xFF", false, false), Category::Other);
    }

    #[test]
    fn mime_and_icon() {
        assert_eq!(mime_of(b"a.png", false), "image/png");
        assert_eq!(icon_of("image/png", false), "image-png");
        assert_eq!(mime_of(b"d", true), "inode/directory");
        assert_eq!(icon_of("inode/directory", true), "folder");
        assert_eq!(
            icon_of(mime_of(b"zzz", false), false),
            "application-x-generic"
        );
    }

    #[test]
    fn bits_and_names_round_trip() {
        for n in 0..CATEGORY_COUNT {
            let c = Category::from_u8(n).expect("valid");
            assert_eq!(c as u8, n);
        }
        assert!(Category::from_u8(CATEGORY_COUNT).is_none());
        assert_eq!(Category::from_name("disk-image"), Some(Category::DiskImage));
        assert_eq!(Category::from_name("nope"), None);
    }
}
