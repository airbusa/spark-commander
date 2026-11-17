//! Help > About: what Spark Commander is, and what it is made with.

pub const NAME: &str = "Spark Commander";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const WEBSITE: &str = "https://github.com/airbusa/spark-commander";
/// Its licence's name, and its text.
pub const LICENCE: (&str, &str) = ("GNU General Public License 3.0 or later", "https://www.gnu.org/licenses/gpl-3.0.html");

/// Another project's work in it: its name, what it does here, its licence and site.
pub struct Part {
    pub name: &'static str,
    pub what: fn() -> String,
    pub licence: &'static str,
    pub website: &'static str,
}

const fn part(name: &'static str, what: fn() -> String, licence: &'static str, website: &'static str) -> Part {
    Part { name, what, licence, website }
}

/// What comes with it besides the add-ons (`addons::Builtin`) and the
/// libraries it is built on; Cargo.lock lists every crate.
pub const SHIPPED: &[Part] = &[
    part("Material Icon Theme", || crate::t!("about.part.material_icons"), "MIT", "https://github.com/material-extensions/vscode-material-icon-theme"),
    part("Lucide", || crate::t!("about.part.lucide"), "ISC", "https://lucide.dev"),
];

pub const LIBRARIES: &[Part] = &[
    part("GPUI", || crate::t!("about.part.gpui"), "Apache 2.0", "https://github.com/zed-industries/zed"),
    part("GPUI Kit", || crate::t!("about.part.gpui_kit"), "Apache 2.0", "https://github.com/longbridge/gpui-component"),
    part("tree-sitter", || crate::t!("about.part.tree_sitter"), "MIT", "https://tree-sitter.github.io"),
    part("alacritty_terminal", || crate::t!("about.part.alacritty_terminal"), "Apache 2.0", "https://github.com/alacritty/alacritty"),
    part("image", || crate::t!("about.part.image"), "MIT or Apache 2.0", "https://github.com/image-rs/image"),
    part("pdfium-render", || crate::t!("about.part.pdfium_render"), "MIT or Apache 2.0", "https://github.com/ajrcarey/pdfium-render"),
    part("russh, russh-sftp", || crate::t!("about.part.russh"), "Apache 2.0", "https://github.com/warp-tech/russh"),
    part("suppaftp", || crate::t!("about.part.suppaftp"), "MIT or Apache 2.0", "https://github.com/veeso/suppaftp"),
    part("rustls", || crate::t!("about.part.rustls"), "Apache 2.0, ISC or MIT", "https://github.com/rustls/rustls"),
    part("keyring", || crate::t!("about.part.keyring"), "MIT or Apache 2.0", "https://github.com/open-source-cooperative/keyring-rs"),
    part("zbus", || crate::t!("about.part.zbus"), "MIT", "https://github.com/z-galaxy/zbus"),
    part("trash", || crate::t!("about.part.trash"), "MIT", "https://github.com/ArturKovacs/trash"),
    part("html5ever", || crate::t!("about.part.html5ever"), "MIT or Apache 2.0", "https://github.com/servo/html5ever"),
    part("similar", || crate::t!("about.part.similar"), "Apache 2.0", "https://github.com/mitsuhiko/similar"),
    part("encoding_rs", || crate::t!("about.part.encoding_rs"), "MIT or Apache 2.0, BSD 3-Clause", "https://github.com/hsivonen/encoding_rs"),
    part("tokio", || crate::t!("about.part.tokio"), "MIT", "https://github.com/tokio-rs/tokio"),
    part("serde", || crate::t!("about.part.serde"), "MIT or Apache 2.0", "https://github.com/serde-rs/serde"),
    part("ratatui, ratatui-image", || crate::t!("about.part.ratatui"), "MIT", "https://github.com/ratatui/ratatui"),
];
