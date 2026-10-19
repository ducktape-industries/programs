//! The product's visual system: bundled fonts, the type scale, and the one
//! palette both the native shell (through the kit's theme) and the WASM
//! views (through `view::design`) paint with.

/// Font identity: the family names a view writes on the wire. The app does
/// not draw these two faces: its host (app `crates/host/src/fonts.rs`,
/// `app_family`) draws [`FAMILY_UI`] as Instrument Sans and [`FAMILY_MONO`]
/// as IBM Plex Mono, the Expo app's faces, and bundles Regular, Italic,
/// Medium, Bold and Bold Italic of each. So a weight of 500
/// (`FontWeight::MEDIUM`) draws Medium.
pub mod fonts {
    /// the UI face — every sans role (default, medium, display). The app
    /// draws it as Instrument Sans.
    pub const FAMILY_UI: &str = "Inter";
    /// the data face — hashes, seqs, diffs, code, the log ring. The app
    /// draws it as IBM Plex Mono.
    pub const FAMILY_MONO: &str = "JetBrains Mono";
    /// The Hangul face behind [`FAMILY_UI`]: neither Latin family draws
    /// 한글, so every Korean run falls back, and the fallback is named
    /// rather than searched (`app/src/shell.rs`'s chains).
    pub const FAMILY_UI_HANGUL: &str = "Pretendard";
    /// The Hangul face behind [`FAMILY_MONO`]. A code face exists for its
    /// column grid, and [`FAMILY_UI_HANGUL`] is proportional — Korean in a
    /// terminal or a diff has to land on a monospaced Hangul face.
    pub const FAMILY_MONO_HANGUL: &str = "D2Coding";
    /// This crate's own files for the two names above, each the vendor's
    /// own released static face (see `assets/fonts/SOURCES`). The app
    /// bundles its own set instead (`crates/host/src/fonts.rs`
    /// `BUNDLED_FACES`); nothing it draws comes from here. The emoji face
    /// supplies fallback glyphs; it is not a separate product type role.
    ///
    /// ONE FILE PER FACE, and no variable font: the shell's text system
    /// shapes with the matched face's own weight and rasterizes with no
    /// variation settings, so a family holding one variable face draws every
    /// weight at 400 and slants for nothing. Hangul has no italic in any
    /// open font: an italic run slants its Latin and stays upright in 한글.
    pub const ASSETS: [&str; 13] = [
        "assets/fonts/Inter-Regular.ttf",
        "assets/fonts/Inter-Bold.ttf",
        "assets/fonts/Inter-Italic.ttf",
        "assets/fonts/Inter-BoldItalic.ttf",
        "assets/fonts/JetBrainsMono-Regular.ttf",
        "assets/fonts/JetBrainsMono-Bold.ttf",
        "assets/fonts/JetBrainsMono-Italic.ttf",
        "assets/fonts/JetBrainsMono-BoldItalic.ttf",
        "assets/fonts/Pretendard-Regular.otf",
        "assets/fonts/Pretendard-Bold.otf",
        "assets/fonts/D2Coding-Regular.ttf",
        "assets/fonts/D2Coding-Bold.ttf",
        "assets/fonts/NotoColorEmoji.ttf",
    ];
}

/// Text sizes, in pixels. One dense scale for the shell and every view: the
/// body is 13px and nothing in the chrome is louder than 16px.
pub mod type_scale {
    /// a page title — a view's own title row
    pub const TITLE: f64 = 16.;
    /// a section title inside a view
    pub const SECTION: f64 = 13.5;
    /// The native shell's default text size.
    pub const BODY: f64 = 13.;
    /// secondary copy beside body text
    pub const SECONDARY: f64 = 12.;
    /// a caption, a timestamp, a badge
    pub const CAPTION: f64 = 11.;
    /// identifiers in the data face
    pub const MONO: f64 = 12.;

    /// The scale under a finger (`window.touch()`): the Expo app's text
    /// roles, each a size and its line height (`*_LINE`), since the reader
    /// is further from a phone than from a desk, and for the three roles
    /// Expo spaces out or tightens, the space after each letter
    /// (`*_LETTER_SPACING`, in px). The body is 15. Each number is mobile
    /// `src/design/theme.ts` `type` (`theme.ts:40-62`), at the line named
    /// on it.
    pub mod touch {
        /// a code to read out or compare: the join code, a key's
        /// fingerprint (`theme.ts:61` `display`)
        pub const DISPLAY: f64 = 36.;
        /// [`DISPLAY`]'s line height
        pub const DISPLAY_LINE: f64 = 44.;
        /// [`DISPLAY`]'s letter spacing: 0.04 of its size apart
        /// (`theme.ts:61` `letterSpacing`)
        pub const DISPLAY_LETTER_SPACING: f64 = 1.44;
        /// a view's first page's title (`theme.ts:59` `bigTitle`)
        pub const BIG_TITLE: f64 = 22.;
        /// [`BIG_TITLE`]'s line height
        pub const BIG_TITLE_LINE: f64 = 27.;
        /// [`BIG_TITLE`]'s letter spacing: 0.01 of its size tighter
        /// (`theme.ts:59` `letterSpacing`)
        pub const BIG_TITLE_LETTER_SPACING: f64 = -0.22;
        /// a detail head's name, a fact's value (`theme.ts:57` `heading`)
        pub const HEADING: f64 = 20.;
        /// [`HEADING`]'s line height
        pub const HEADING_LINE: f64 = 24.;
        /// a page title in a view's own title row (`theme.ts:55` `title`)
        pub const TITLE: f64 = 17.;
        /// [`TITLE`]'s line height
        pub const TITLE_LINE: f64 = 21.;
        /// body text (`theme.ts:42` `body`)
        pub const BODY: f64 = 15.;
        /// [`BODY`]'s line height
        pub const BODY_LINE: f64 = 21.;
        /// strip items, key/value rows, dialog bodies (`theme.ts:44` `small`)
        pub const SMALL: f64 = 14.;
        /// [`SMALL`]'s line height
        pub const SMALL_LINE: f64 = 19.;
        /// secondary copy; a mono run at this size (`theme.ts:46`
        /// `secondary`)
        pub const SECONDARY: f64 = 13.;
        /// [`SECONDARY`]'s line height
        pub const SECONDARY_LINE: f64 = 18.;
        /// a caption, a timestamp, a count (`theme.ts:47` `caption`)
        pub const CAPTION: f64 = 12.;
        /// [`CAPTION`]'s line height
        pub const CAPTION_LINE: f64 = 16.;
        /// a badge, a chip's count, a tab label (`theme.ts:49` `tiny`)
        pub const TINY: f64 = 11.;
        /// [`TINY`]'s line height
        pub const TINY_LINE: f64 = 14.;
        /// a fact's key, a code box's label, in capitals (`theme.ts:51`
        /// `eyebrow`)
        pub const EYEBROW: f64 = 11.;
        /// [`EYEBROW`]'s line height
        pub const EYEBROW_LINE: f64 = 14.;
        /// [`EYEBROW`]'s letter spacing: 0.06 of its size apart
        /// (`theme.ts:51` `letterSpacing`)
        pub const EYEBROW_LETTER_SPACING: f64 = 0.66;
        /// code, in the data face (`theme.ts:53` `code`)
        pub const CODE: f64 = 12.;
        /// [`CODE`]'s line height
        pub const CODE_LINE: f64 = 20.;
    }
}

/// Where a layout changes shape, in logical px of the body a view is laid
/// out in: below [`COMPACT_BELOW`] one pane at a time (a phone, 360-430),
/// from [`WIDE_FROM`] every pane docks (explorer's stack point today);
/// between them the view's own `docks` arithmetic decides. 480 is above the
/// widest phone and at or below every first-party desktop minimum, so no
/// desktop layout changes class.
pub mod width {
    pub const COMPACT_BELOW: f64 = 480.;
    pub const WIDE_FROM: f64 = 960.;
}

/// Corner radii, in pixels. The shell's canvas is square: a control, a
/// card and a modal have corners, and only an avatar or a dot is a circle.
pub mod radius {
    /// a control: a button, an input, a list row
    pub const CONTROL: f64 = 0.;
    /// a card, a panel, a modal
    pub const CARD: f64 = 0.;
    /// a pill: an avatar
    pub const PILL: f64 = 999.;
}

/// Gaps and insets, in pixels. One ladder: every gap the kit's builders open
/// and every edge they pad is a step on it, so a view names the step instead
/// of the number. The Expo app's `gap` is this ladder, step for step (mobile
/// `src/design/tokens.gen.ts:94-103` `spacing`), so a touch layout names the
/// same steps.
pub mod spacing {
    /// two lines that read as one: a name over its caption
    pub const HAIR: f64 = 2.;
    /// the tightest gap: a row's own inset, two lines that belong together
    pub const XXS: f64 = 4.;
    /// a label over the thing it names: a field, a stacked key/value
    pub const XS: f64 = 6.;
    /// the default gap between siblings in a row or a column
    pub const SM: f64 = 8.;
    /// a gap that separates without opening a section
    pub const MD: f64 = 10.;
    /// a card's inset, and the gap between a label and its value
    pub const LG: f64 = 12.;
    /// between blocks of one page: wider than a card's inset, short of a section
    pub const BLOCK: f64 = 16.;
    /// a section inset: what an empty state or a centred block sits in
    pub const XL: f64 = 24.;
}

/// The heights a view fixes, in pixels, so rows line up across views and a
/// list can be reserved or virtualized without measuring text.
pub mod height {
    /// a list row: a chat name, a file, a forge item
    pub const ROW: f64 = 26.;
    /// a control on one line: a toolbar button, a picker, an input beside one
    pub const CONTROL: f64 = 28.;
    /// a person's initial inline with small text: a byline, a reply line
    pub const AVATAR_SM: f64 = 18.;
    /// a person's initial in a list row: a member, a dm
    pub const AVATAR: f64 = 20.;
    /// a person's initial heading a card or panel
    pub const AVATAR_LG: f64 = 24.;
    /// the smallest box a pointer presses, each way (the AX door's AX-017)
    pub const HIT: f64 = 24.;

    /// an icon on an icon button or in a title row: the desktop shell's own
    /// (the app's chrome draws its bell and settings icons at 16)
    pub const ICON: f64 = 16.;

    /// The heights under a finger (`window.touch()`): Apple's 44pt, Android's
    /// 48dp. Each number is mobile `src/design/theme.ts` `size`
    /// (`theme.ts:69-88`) at the line named on it; an avatar's initial is
    /// the Expo kit's `Avatar` table (`src/kit/Avatar.tsx:7` `TEXT`).
    pub mod touch {
        /// a page's title row (`theme.ts:70` `header`)
        pub const HEADER: f64 = 56.;
        /// a list row (`theme.ts:72` `row`)
        pub const ROW: f64 = 48.;
        /// a list row with a caption under its title (`theme.ts:73`
        /// `rowTall`)
        pub const ROW_TALL: f64 = 56.;
        /// a control on one line (`theme.ts:74` `control`)
        pub const CONTROL: f64 = 44.;
        /// the one action a screen leads with (`theme.ts:75` `primary`)
        pub const PRIMARY: f64 = 48.;
        /// a small button (`theme.ts:76` `small`)
        pub const SMALL: f64 = 36.;
        /// the smallest box a finger presses, each way (`theme.ts:71` `hit`)
        pub const HIT: f64 = 44.;
        /// a few choices in one box, its two hairlines included
        /// (`theme.ts:77` `segmented`)
        pub const SEGMENTED: f64 = 36.;
        /// a state word on a tag (`theme.ts:79` `badge`)
        pub const BADGE: f64 = 20.;
        /// a row of a sheet (`theme.ts:80` `sheetRow`)
        pub const SHEET_ROW: f64 = 52.;
        /// a bar of tabs, its hairline included (`theme.ts:81` `strip`)
        pub const STRIP: f64 = 44.;
        /// a person's initial in a list row or beside a message
        /// (`theme.ts:86` `avatar.list`)
        pub const AVATAR: f64 = 32.;
        /// [`AVATAR`]'s initial (the Expo kit's `Avatar` sizes its
        /// initials by a table, not one ratio)
        pub const AVATAR_TEXT: f64 = 13.;
        /// a person's initial inline: a title row, a mention (`theme.ts:86`
        /// `avatar.inline`)
        pub const AVATAR_INLINE: f64 = 24.;
        /// [`AVATAR_INLINE`]'s initial
        pub const AVATAR_INLINE_TEXT: f64 = 11.;
        /// a person's initial heading a sheet (`theme.ts:86` `avatar.sheet`)
        pub const AVATAR_SHEET: f64 = 40.;
        /// [`AVATAR_SHEET`]'s initial
        pub const AVATAR_SHEET_TEXT: f64 = 16.;
        /// a person's initial heading their own page (`theme.ts:86`
        /// `avatar.detail`)
        pub const AVATAR_DETAIL: f64 = 56.;
        /// [`AVATAR_DETAIL`]'s initial
        pub const AVATAR_DETAIL_TEXT: f64 = 22.;
        /// an icon on an icon button or in a title row (`theme.ts:87`
        /// `icon.hit`)
        pub const ICON: f64 = 22.;
        /// an icon leading a list row (`theme.ts:87` `icon.row`)
        pub const ICON_ROW: f64 = 18.;
        /// an icon leading a sheet row (`theme.ts:87` `icon.sheet`)
        pub const ICON_SHEET: f64 = 20.;
        /// an empty state's icon (`theme.ts:87` `icon.empty`)
        pub const ICON_EMPTY: f64 = 28.;
    }
}

/// One sRGB color as the wire carries it: `[r, g, b, a]` in `0.0..=1.0`.
pub type Color = [f32; 4];

/// The named colors of one appearance, the shell's calm set: warm-neutral
/// greys, a sidebar one step off the window (not ink), ink itself as the
/// accent for what is chosen, and the four status tones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// the sidebar rail: one step off the window, like any surface
    pub sidebar: Color,
    /// text on the sidebar
    pub sidebar_foreground: Color,
    /// secondary text on the sidebar: section names, the resting rows
    pub sidebar_muted: Color,
    /// the chosen or hovered sidebar row
    pub sidebar_raised: Color,
    /// hairlines on the sidebar
    pub sidebar_border: Color,
    /// the window
    pub background: Color,
    /// a sidebar, a pane, a card — one step off the window
    pub surface: Color,
    /// a raised surface: a hovered row, a code block
    pub surface_raised: Color,
    /// hairlines between regions
    pub border: Color,
    /// the border of a control
    pub border_strong: Color,
    /// body text
    pub foreground: Color,
    /// secondary text
    pub muted: Color,
    /// faint text: placeholders, disabled
    pub faint: Color,
    /// the accent itself: the live dot, the focus ring, the selection bar
    pub accent: Color,
    /// the accent as a wash behind a chosen row
    pub accent_soft: Color,
    /// text on the accent wash
    pub accent_foreground: Color,
    /// a primary action's fill (ink) and its text
    pub primary: Color,
    pub primary_foreground: Color,
    pub link: Color,
    pub success: Color,
    pub success_soft: Color,
    pub warning: Color,
    pub warning_soft: Color,
    pub danger: Color,
    pub danger_soft: Color,
    /// an agent's identity tint
    pub agent: Color,
    pub agent_soft: Color,
}

const fn hex(value: u32) -> Color {
    [
        ((value >> 16) & 0xff) as f32 / 255.,
        ((value >> 8) & 0xff) as f32 / 255.,
        (value & 0xff) as f32 / 255.,
        1.,
    ]
}

pub const LIGHT: Palette = Palette {
    sidebar: hex(0xF5F5F3),
    sidebar_foreground: hex(0x111111),
    sidebar_muted: hex(0x6B6B6B),
    sidebar_raised: hex(0xEFEFED),
    sidebar_border: hex(0xE6E6E6),
    background: hex(0xFFFFFF),
    surface: hex(0xF5F5F3),
    surface_raised: hex(0xEFEFED),
    border: hex(0xE6E6E6),
    border_strong: hex(0xCFCFCF),
    foreground: hex(0x111111),
    muted: hex(0x6B6B6B),
    faint: hex(0xA3A3A3),
    accent: hex(0x111111),
    accent_soft: hex(0xEFEFED),
    accent_foreground: hex(0x111111),
    primary: hex(0x111111),
    primary_foreground: hex(0xFFFFFF),
    link: hex(0x111111),
    success: hex(0x2E7D32),
    success_soft: hex(0xE6F2E7),
    warning: hex(0xB4700F),
    warning_soft: hex(0xFBF0DA),
    danger: hex(0xB42318),
    danger_soft: hex(0xFCE8E6),
    agent: hex(0x7A4BD8),
    agent_soft: hex(0xF0EAFC),
};

pub const DARK: Palette = Palette {
    sidebar: hex(0x1A1A1A),
    sidebar_foreground: hex(0xEDEDED),
    sidebar_muted: hex(0x8F8F8F),
    sidebar_raised: hex(0x222222),
    sidebar_border: hex(0x2A2A2A),
    background: hex(0x111111),
    surface: hex(0x1A1A1A),
    surface_raised: hex(0x222222),
    border: hex(0x2A2A2A),
    border_strong: hex(0x3A3A3A),
    foreground: hex(0xEDEDED),
    muted: hex(0x8F8F8F),
    faint: hex(0x5A5A5A),
    accent: hex(0xEDEDED),
    accent_soft: hex(0x222222),
    accent_foreground: hex(0xEDEDED),
    primary: hex(0xEDEDED),
    primary_foreground: hex(0x111111),
    link: hex(0xEDEDED),
    success: hex(0x6FCF97),
    success_soft: hex(0x16301F),
    warning: hex(0xE1A93F),
    warning_soft: hex(0x3A2C10),
    danger: hex(0xF97066),
    danger_soft: hex(0x3E1B14),
    agent: hex(0xA78BF5),
    agent_soft: hex(0x2A2340),
};

/// The palette of an appearance.
pub const fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// `#rrggbb` (or `#rrggbbaa` when translucent) — the notation the kit's
/// theme JSON and SVG both read.
pub fn css(color: Color) -> String {
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    let [r, g, b, a] = color;
    if a >= 1. {
        format!("#{:02x}{:02x}{:02x}", channel(r), channel(g), channel(b))
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            channel(r),
            channel(g),
            channel(b),
            channel(a)
        )
    }
}

/// The same palette as a gpui-kit theme set: two themes, one per mode, so
/// the kit's own controls (buttons, inputs, checkboxes, scrollbars) paint
/// with the colors the views paint with.
pub fn kit_theme_json() -> String {
    let theme = |name: &str, mode: &str, p: &Palette| {
        let c = css;
        format!(
            r##"{{
  "name": "{name}",
  "mode": "{mode}",
  "font.family": "{ui}",
  "font.size": {body},
  "mono_font.family": "{mono}",
  "mono_font.size": {mono_size},
  "radius": {radius},
  "radius.lg": {radius_lg},
  "shadow": false,
  "colors": {{
    "background": "{bg}",
    "foreground": "{fg}",
    "border": "{border}",
    "input.border": "{border_strong}",
    "ring": "{accent}",
    "caret": "{fg}",
    "selection.background": "{selection}",
    "muted.background": "{surface}",
    "muted.foreground": "{muted}",
    "accent.background": "{surface_raised}",
    "accent.foreground": "{fg}",
    "secondary.background": "{surface}",
    "secondary.hover.background": "{surface_raised}",
    "secondary.active.background": "{accent_soft}",
    "secondary.foreground": "{fg}",
    "primary.background": "{primary}",
    "primary.hover.background": "{primary}",
    "primary.active.background": "{primary}",
    "primary.foreground": "{primary_fg}",
    "danger.background": "{danger}",
    "danger.foreground": "{bg}",
    "success.background": "{success}",
    "success.foreground": "{bg}",
    "warning.background": "{warning}",
    "warning.foreground": "{bg}",
    "info.background": "{link}",
    "info.foreground": "{bg}",
    "link.foreground": "{link}",
    "link.hover.foreground": "{link}",
    "link.active.foreground": "{link}",
    "popover.background": "{bg}",
    "popover.foreground": "{fg}",
    "list.background": "{bg}",
    "list.hover.background": "{surface}",
    "list.active.background": "{accent_soft}",
    "list.active.border": "{accent}",
    "list.even.background": "{bg}",
    "list.head.background": "{surface}",
    "table.background": "{bg}",
    "table.hover.background": "{surface}",
    "table.active.background": "{accent_soft}",
    "table.active.border": "{accent}",
    "table.even.background": "{bg}",
    "table.head.background": "{surface}",
    "table.head.foreground": "{muted}",
    "table.row.border": "{border}",
    "sidebar.background": "{sidebar}",
    "sidebar.foreground": "{sidebar_fg}",
    "sidebar.border": "{sidebar_border}",
    "sidebar.accent.background": "{sidebar_raised}",
    "sidebar.accent.foreground": "{sidebar_fg}",
    "sidebar.primary.background": "{accent}",
    "sidebar.primary.foreground": "{primary_fg}",
    "tab_bar.background": "{surface}",
    "tab_bar.segmented.background": "{surface}",
    "tab.background": "{surface}",
    "tab.foreground": "{muted}",
    "tab.active.background": "{bg}",
    "tab.active.foreground": "{fg}",
    "group_box.background": "{surface}",
    "group_box.foreground": "{fg}",
    "description_list_label.background": "{surface}",
    "description_list_label.foreground": "{muted}",
    "switch.background": "{border_strong}",
    "slider.bar.background": "{accent}",
    "slider.thumb.background": "{bg}",
    "progress_bar.background": "{accent}",
    "skeleton.background": "{surface_raised}",
    "scrollbar.background": "{bg}00",
    "scrollbar.thumb.background": "{border_strong}",
    "scrollbar.thumb.hover.background": "{muted}",
    "drag_border": "{accent}",
    "drop_target.background": "{accent_soft}",
    "title_bar.background": "{bg}",
    "title_bar.border": "{border}",
    "window.border": "{border}"
  }}
}}"##,
            ui = fonts::FAMILY_UI,
            mono = fonts::FAMILY_MONO,
            body = type_scale::BODY,
            mono_size = type_scale::MONO,
            radius = radius::CONTROL as usize,
            radius_lg = radius::CARD as usize,
            sidebar = c(p.sidebar),
            sidebar_fg = c(p.sidebar_foreground),
            sidebar_border = c(p.sidebar_border),
            sidebar_raised = c(p.sidebar_raised),
            bg = c(p.background),
            fg = c(p.foreground),
            border = c(p.border),
            border_strong = c(p.border_strong),
            accent = c(p.accent),
            selection = c([p.accent[0], p.accent[1], p.accent[2], 0.35]),
            surface = c(p.surface),
            surface_raised = c(p.surface_raised),
            muted = c(p.muted),
            accent_soft = c(p.accent_soft),
            primary = c(p.primary),
            primary_fg = c(p.primary_foreground),
            danger = c(p.danger),
            success = c(p.success),
            warning = c(p.warning),
            link = c(p.link),
        )
    };
    format!(
        "{{\"name\": \"Ducktape\", \"themes\": [{}, {}]}}",
        theme(LIGHT_THEME, "light", &LIGHT),
        theme(DARK_THEME, "dark", &DARK)
    )
}

/// The theme names `kit_theme_json` registers.
pub const LIGHT_THEME: &str = "Ducktape Light";
pub const DARK_THEME: &str = "Ducktape Dark";

/// A key or hash as every view shows it: the first 8 and last 4 characters
/// (`9f3a1b2c…c21e`). A text short enough that cutting would not shorten it
/// stays whole. Cuts on character boundaries.
pub fn short_hex(text: &str) -> String {
    const HEAD: usize = 8;
    const TAIL: usize = 4;
    let count = text.chars().count();
    if count <= HEAD + TAIL + 1 {
        return text.to_owned();
    }
    let head: String = text.chars().take(HEAD).collect();
    let tail: String = text.chars().skip(count - TAIL).collect();
    format!("{head}…{tail}")
}

/// A key's fingerprint, to compare by eye with the device that shows it:
/// the first four bytes of its SHA-256 in upper-case hex, split in two
/// (`4354 C7BD`). It is the app's `noded-client` `join::fingerprint`
/// (`crates/noded-client/src/join.rs:51-58`), which the Expo app calls
/// (`app/settings.tsx:63`, `app/account/index.tsx:148`).
pub fn fingerprint(key: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(key)[..4]
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    format!("{} {}", &hex[..4], &hex[4..])
}

// `abi::hex` read back: `None` for an odd length or a non-hex digit.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let digit = |byte: u8| (byte as char).to_digit(16).map(|d| d as u8);
    let text = text.as_bytes();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

// Bytes as a person reads them: a key or hash up to 32 bytes as `short_hex`,
// else their count and the head and tail `short_hex` keeps, without hexing
// the whole payload.
pub fn preview(bytes: &[u8]) -> String {
    match bytes.len() {
        0 => "0 bytes".into(),
        1..=32 => short_hex(&abi::hex(bytes)),
        len => format!(
            "{len} bytes · {}",
            short_hex(&(abi::hex(&bytes[..8]) + &abi::hex(&bytes[len - 2..])))
        ),
    }
}

// The badge beside an account's name: "Agent · managed by <name>", or
// "Program · <program>" for a system account. A user wears none. `name_of`
// names the manager.
pub fn badge(
    kind: &abi::role::identity::Kind,
    name_of: impl FnOnce(abi::role::identity::AccountNumber) -> String,
) -> Option<String> {
    use abi::role::identity::{Category, Kind};
    match kind {
        Kind::User => None,
        Kind::Program {
            manager,
            category: Category::Agent,
            ..
        } => Some(format!("Agent · managed by {}", name_of(*manager))),
        Kind::System(program) => Some(format!("Program · {program}")),
    }
}

// Why an account does not act, noted beside its name: "suspended" or
// "revoked". `None` while it acts.
pub fn note(kind: &abi::role::identity::Kind) -> Option<&'static str> {
    use abi::role::identity::{Kind, Standing};
    match kind {
        Kind::Program {
            standing: Standing::Suspended,
            ..
        } => Some("suspended"),
        Kind::Program {
            standing: Standing::Revoked,
            ..
        } => Some("revoked"),
        Kind::User
        | Kind::System(_)
        | Kind::Program {
            standing: Standing::Active,
            ..
        } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_hex_keeps_head_and_tail() {
        assert_eq!(short_hex(&"ab".repeat(32)), "abababab…abab");
        assert_eq!(short_hex("0123456789abc"), "0123456789abc");
        assert_eq!(short_hex("0123456789abcd"), "01234567…abcd");
        assert_eq!(short_hex(""), "");
        assert_eq!(
            short_hex("오리테이프오리테이프오리테이프"),
            "오리테이프오리테…리테이프"
        );
    }
    #[test]
    fn unhex_reads_hex_back() {
        for bytes in [&[][..], &[0], &[0xab, 0x01, 0xff]] {
            assert_eq!(unhex(&abi::hex(bytes)).as_deref(), Some(bytes));
        }
        assert_eq!(unhex("ABff"), Some(vec![0xab, 0xff]));
        for bad in ["a", "zz", "é1", "0x00"] {
            assert_eq!(unhex(bad), None, "{bad}");
        }
    }
    #[test]
    fn a_preview_keeps_a_short_run_and_counts_a_long_one() {
        assert_eq!(preview(&[]), "0 bytes");
        assert_eq!(preview(&[0xab; 4]), "abababab");
        assert_eq!(preview(&[0xab; 32]), "abababab…abab");
        let mut long = vec![0x11; 40];
        long[39] = 0x22;
        assert_eq!(preview(&long), "40 bytes · 11111111…1122");
    }
    #[test]
    fn an_agent_wears_its_manager_and_a_held_one_its_standing() {
        use abi::role::identity::{Category, Kind, Standing};
        let agent = |standing| Kind::Program {
            manager: 7,
            category: Category::Agent,
            standing,
            script: None,
        };
        let named = |number: u64| format!("#{number}");
        let active = agent(Standing::Active);
        assert_eq!(
            badge(&active, named).as_deref(),
            Some("Agent · managed by #7")
        );
        assert_eq!(
            badge(&Kind::System("chat".into()), named).as_deref(),
            Some("Program · chat")
        );
        assert_eq!(badge(&Kind::User, named), None);
        assert_eq!(note(&active), None);
        assert_eq!(note(&agent(Standing::Suspended)), Some("suspended"));
        assert_eq!(note(&agent(Standing::Revoked)), Some("revoked"));
        assert_eq!(note(&Kind::User), None);
    }
    #[test]
    fn a_fingerprint_is_the_first_four_bytes_of_the_keys_sha256() {
        // SHA-256 of 32 zero bytes is 66687aad…
        assert_eq!(fingerprint(&[0; 32]), "6668 7AAD");
    }
    #[test]
    fn every_embedded_font_file_exists_and_is_truetype() {
        for asset in fonts::ASSETS {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(asset);
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("font asset {asset} unreadable: {error}"));
            let magic = &bytes[..4];
            assert!(
                magic == b"\x00\x01\x00\x00" || magic == b"OTTO" || magic == b"true",
                "{asset} is not a TrueType/OpenType file"
            );
        }
    }

    #[test]
    fn css_notation_round_trips_the_palette() {
        assert_eq!(css(hex(0x5B5FC7)), "#5b5fc7");
        assert_eq!(css([1., 1., 1., 0.5]), "#ffffff80");
    }

    #[test]
    fn the_kit_theme_json_names_both_modes_and_the_product_fonts() {
        let json = kit_theme_json();
        assert!(json.contains("\"Ducktape Light\""));
        assert!(json.contains("\"Ducktape Dark\""));
        assert!(json.contains("\"font.family\": \"Inter\""));
        assert!(json.contains("\"mode\": \"dark\""));
        let braces = json.matches('{').count();
        assert_eq!(braces, json.matches('}').count());
    }
}
